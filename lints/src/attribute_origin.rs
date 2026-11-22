//! Where an attribute of the expanded AST comes from, so the lints that read source text can also
//! check the attributes that macros write, which that text doesn't hold.
//!
//! A macro builds an attribute from tokens, and each token's span keeps where it came from: a
//! token written in a `macro_rules!` body or made by a proc macro has the expansion's syntax
//! context, and a token the macro's caller passed in keeps the caller's. An attribute is checked
//! as source text if its path starts a meta item the scan read and all its tokens are inside that
//! meta item, however a macro forwarded it. Otherwise the token from an expansion says which macro
//! wrote it.
//!
//! If none is from one, a proc macro built the attribute from tokens of the source that aren't
//! this attribute there, keeping their spans. If they all lie in a string literal, the macro
//! parsed the attribute from the string, as serde parses the type of `#[serde(into = "..")]`, so
//! the caller wrote it where no lint reads it: it is [`Origin::Unwritten`], however trusted the
//! macro. Otherwise the macro wrote it around the caller's code: `#[tokio::main]` and
//! `#[tracing::instrument]` put the spans of the function's tokens on the `#[allow(..)]` they
//! write, so errors point at the user's code, and `paste` does the same with the identifiers it
//! joins. Such an attribute is credited to the macros that wrote the innermost item around it,
//! which the compiler records for every item an expansion produced: it is trusted if each of them
//! from another crate is, and at least one is an attribute or derive macro other than the
//! compiler's built-in ones in `core`, which never build attributes from the caller's tokens. A
//! macro called inside such an item that writes the attribute's `#[` and `]` itself around the
//! caller's tokens, as a `macro_rules!` that forwards `#[$meta]` does, is credited to that macro
//! instead, so an untrusted macro can't borrow the item's trust. An attribute whose tokens all lie
//! inside a call of a function-like macro written in the source, such as `duplicate! { [quiet;
//! [allow]] #[quiet(..)] .. }`, which keeps every token's span while it swaps one for another,
//! isn't credited to the item's macros either: the call, not they, may have built it, so it stays
//! [`Origin::Unwritten`], as an attribute such a macro builds outside any item a macro wrote
//! does.
//!
//! A lint level that a macro from outside the workspace writes on an item holding a static in a
//! constructor section, which the same expansion writes, is `code_before_main`'s to judge, as
//! `inventory::submit!` writes `allow(non_upper_case_globals)` on the `const _` around its
//! constructor. If `constructor_macros` lets the static pass, the macro's crate counts as trusted
//! for it, and the attribute is [`Origin::Trusted`], so `trusted_macro_lints` still decides what it
//! may set over code written in the source. Otherwise it is [`Origin::Constructor`]:
//! `code_before_main` reports the macro call, and fixing that removes the attribute, so
//! `suppression_policy` checks it as the macro's only if `code_before_main` is off at one of the
//! statics, where it would report nothing. A level that names anything but lints, or a lint group
//! or `code_before_main` itself, is never judged so: it is checked as the macro's, as before.

use std::collections::HashMap;

use rustc_ast::tokenstream::{TokenStream, TokenTree};
use rustc_ast::visit::{self, AssocCtxt, Visitor};
use rustc_ast::{
    AssocItem, AttrArgs, AttrId, AttrKind, Attribute, Crate, ForeignItem, Item, LitKind, MetaItem,
    MetaItemInner, MetaItemKind, NodeId,
};
use rustc_lint::{EarlyContext, LintContext};
use rustc_middle::middle::resolve::PerOwnerResolverData;
use rustc_middle::ty::TyCtxt;
use rustc_span::def_id::{CrateNum, LOCAL_CRATE};
use rustc_span::hygiene::MacroKind;
use rustc_span::{ExpnId, ExpnKind, Span, Symbol, sym};

use crate::local_source::{LocalSource, Meta, MetaKind};

/// Where an attribute comes from.
pub(crate) enum Origin {
    /// Written in the crate's source, where the scan read it: as an attribute, inside a
    /// `cfg_attr`, or in a macro's body, and checked there.
    Source,
    /// Written by the compiler, such as the test harness or a desugaring, or by a macro from a
    /// crate that `trusted_macro_crates` in `dylint.toml` lists.
    Trusted {
        /// The trusted crates whose macros may have written it: the one whose macro wrote its
        /// tokens, or every one whose macros wrote the item around it. Empty for the compiler.
        crates: Vec<Symbol>,
    },
    /// Written by a macro the crate defines, or one from a crate `trusted_macro_crates` doesn't
    /// list.
    Macro {
        name: Symbol,
        /// The crate that defines the macro, or `None` for the crate being checked.
        defined_in: Option<Symbol>,
    },
    /// Built from tokens of the crate's source that aren't this attribute there, outside any item
    /// a macro from another crate wrote.
    Unwritten,
    /// A lint level that a macro from a crate `trusted_macro_crates` doesn't list wrote on an item
    /// holding a static in a constructor section that `constructor_macros` doesn't let pass,
    /// which the same expansion wrote.
    Constructor {
        name: Symbol,
        /// The crate that defines the macro.
        defined_in: Symbol,
        /// The statics, which `code_before_main` reports where it is on.
        statics: Vec<NodeId>,
    },
}

impl Origin {
    /// Where an attribute that isn't [`Origin::Source`] or [`Origin::Trusted`] comes from, as the
    /// end of a report's message.
    pub(crate) fn describe(&self) -> String {
        match self {
            Self::Source | Self::Trusted { .. } => String::new(),
            Self::Macro {
                name,
                defined_in: None,
            } => format!(", which the macro `{name}` defined in this crate writes"),
            Self::Macro {
                name,
                defined_in: Some(krate),
            }
            | Self::Constructor {
                name,
                defined_in: krate,
                ..
            } => format!(", which the macro `{name}` from `{krate}` writes"),
            Self::Unwritten => {
                ", which a macro built from tokens that aren't an attribute in the source"
                    .to_owned()
            }
        }
    }

    /// What to do about a lint level that a macro from another crate wrote, which the crate's code
    /// can't change: stop calling the macro, rather than trust its crate.
    pub(crate) fn untrusted_crate_help(&self) -> Option<String> {
        let (Self::Macro {
            name,
            defined_in: Some(krate),
        }
        | Self::Constructor {
            name,
            defined_in: krate,
            ..
        }) = self
        else {
            return None;
        };
        Some(format!(
            "write the code without `{name}`, as the idioms in `docs/style.md` show; list \
             `{krate}` in `trusted_macro_crates` only after reading every lint level its macros \
             write, which is a rule change"
        ))
    }

    /// Whether the lints check the attribute as one a macro wrote. `suppression_policy` checks an
    /// [`Origin::Constructor`] itself, once it knows where `code_before_main` is on.
    pub(crate) fn is_untrusted(&self) -> bool {
        matches!(self, Self::Macro { .. } | Self::Unwritten)
    }
}

/// Where `attribute` comes from.
pub(crate) fn origin(cx: &EarlyContext<'_>, source: &LocalSource, attribute: &Attribute) -> Origin {
    let normal = match &attribute.kind {
        AttrKind::Normal(normal) => normal,
        AttrKind::DocComment(..) => return Origin::Source,
        AttrKind::Synthetic(_) | AttrKind::Generated(_) => {
            return Origin::Trusted { crates: Vec::new() };
        }
    };
    let item = &normal.item;
    let path = path_text(attribute);
    let mut spans = vec![item.path.span];
    spans.extend(item.path.segments.iter().map(|segment| segment.ident.span));
    match &item.args {
        AttrArgs::Empty => {}
        AttrArgs::Delimited(args) => {
            spans.extend([args.dspan.open, args.dspan.close]);
            token_spans(&args.tokens, &mut spans);
        }
        AttrArgs::Eq { eq_span, expr } => spans.extend([*eq_span, expr.span]),
    }
    let start = item.path.span.lo();
    if let Some(written) = source.written(&path, item.path.span)
        && spans
            .iter()
            .all(|span| span.lo() >= start && span.hi() <= written.end)
    {
        return Origin::Source;
    }
    let Some(expanded) = spans.iter().find(|span| span.from_expansion()) else {
        if attribute.span.from_expansion()
            && let origin @ Origin::Macro { .. } = macro_origin(cx, attribute.span)
        {
            return origin;
        }
        if spans
            .iter()
            .all(|span| source.string_literal_around(*span).is_some())
        {
            return Origin::Unwritten;
        }
        return source
            .item_expansion(attribute)
            .filter(|_| !source.in_macro_call(attribute.span))
            .map_or(Origin::Unwritten, |expansion| item_origin(cx, expansion));
    };
    constructor_origin(cx, source, attribute, *expanded)
        .unwrap_or_else(|| macro_origin(cx, *expanded))
}

/// The origin of `attribute`, whose token at `expanded` an expansion produced, if that expansion
/// is a macro from outside the workspace that also wrote a static in a constructor section on the
/// item the attribute is on, as the module docs explain.
fn constructor_origin(
    cx: &EarlyContext<'_>,
    source: &LocalSource,
    attribute: &Attribute,
    expanded: Span,
) -> Option<Origin> {
    let expansion = expanded.ctxt().outer_expn();
    let constructors = source.constructors_under(attribute, expansion);
    if constructors.is_empty() {
        return None;
    }
    let data = expansion.expn_data();
    let ExpnKind::Macro(_, name) = data.kind else {
        return None;
    };
    let id = data.macro_def_id.filter(|id| id.krate != LOCAL_CRATE)?;
    let krate = crate_name(id.krate);
    let in_workspace = crate::config::get(cx.sess())
        .macro_crates
        .iter()
        .any(|listed| listed.replace('-', "_") == krate.as_str());
    if in_workspace || is_trusted(cx, krate) || !names_only_other_lints(cx, attribute) {
        return None;
    }
    Some(
        if constructors.iter().all(|constructor| constructor.allowed) {
            Origin::Trusted {
                crates: vec![krate],
            }
        } else {
            Origin::Constructor {
                name,
                defined_in: krate,
                statics: constructors
                    .iter()
                    .map(|constructor| constructor.id)
                    .collect(),
            }
        },
    )
}

/// Whether `attribute` is a lint level that names only lints, none of them a lint group, a
/// deprecated alias of one included, or `code_before_main`, by its last segment, so a tool prefix
/// can't hide the name.
fn names_only_other_lints(cx: &EarlyContext<'_>, attribute: &Attribute) -> bool {
    let meta = meta(attribute);
    if !matches!(
        meta.path.as_str(),
        "allow" | "warn" | "expect" | "deny" | "forbid"
    ) {
        return false;
    }
    let groups = crate::suppression_policy::lint_groups(cx);
    meta.items().iter().all(|item| match &item.kind {
        MetaKind::Word => {
            !groups.contains(&item.path)
                && item.path.rsplit("::").next() != Some("code_before_main")
        }
        MetaKind::Value(_) => item.path == "reason",
        _ => false,
    })
}

/// The expansion that wrote the innermost item around each attribute of `krate` that is inside an
/// item a macro wrote, keyed by the attribute's id and span.
pub(crate) fn item_expansions(krate: &Crate) -> HashMap<(AttrId, Span), ExpnId> {
    rustc_middle::ty::tls::with(|tcx| {
        let (resolver, _) = tcx.resolver_for_lowering();
        let resolver = resolver.borrow();
        let mut items = ItemExpansions {
            tcx,
            owners: &resolver.owners,
            enclosing: vec![ExpnId::root()],
            found: HashMap::new(),
        };
        visit::walk_crate(&mut items, krate);
        items.found
    })
}

/// Walks the expanded crate, keeping the expansion that wrote each enclosing item.
struct ItemExpansions<'a, 'tcx> {
    tcx: TyCtxt<'tcx>,
    owners: &'a rustc_ast::node_id::NodeMap<PerOwnerResolverData<'tcx>>,
    /// The expansion of each item around the node being visited, innermost last; the root
    /// expansion for an item written in the source.
    enclosing: Vec<ExpnId>,
    found: HashMap<(AttrId, Span), ExpnId>,
}

impl ItemExpansions<'_, '_> {
    /// Walks the item `id` with `walk`, inside the expansion that wrote it.
    fn within(&mut self, id: NodeId, walk: impl FnOnce(&mut Self)) {
        let expansion = self.owners.get(&id).map_or_else(
            || *self.enclosing.last().unwrap_or(&ExpnId::root()),
            |owner| self.tcx.expn_that_defined(owner.def_id),
        );
        self.enclosing.push(expansion);
        walk(self);
        self.enclosing.pop();
    }
}

impl<'ast> Visitor<'ast> for ItemExpansions<'_, '_> {
    fn visit_item(&mut self, item: &'ast Item) {
        self.within(item.id, |items| visit::walk_item(items, item));
    }

    fn visit_assoc_item(&mut self, item: &'ast AssocItem, ctxt: AssocCtxt) {
        self.within(item.id, |items| visit::walk_assoc_item(items, item, ctxt));
    }

    fn visit_foreign_item(&mut self, item: &'ast ForeignItem) {
        self.within(item.id, |items| visit::walk_item(items, item));
    }

    fn visit_attribute(&mut self, attribute: &'ast Attribute) {
        if let Some(&expansion) = self.enclosing.last()
            && expansion != ExpnId::root()
        {
            self.found.insert((attribute.id, attribute.span), expansion);
        }
    }
}

/// The origin of an attribute built from tokens of the source inside an item that `expansion`
/// wrote: the macros of that expansion and the ones it is inside, as the module docs explain.
fn item_origin(cx: &EarlyContext<'_>, expansion: ExpnId) -> Origin {
    let mut written = false;
    let mut crates = Vec::new();
    let mut current = expansion;
    while current != ExpnId::root() {
        let data = current.expn_data();
        if let ExpnKind::Macro(kind, name) = data.kind
            && let Some(id) = data.macro_def_id.filter(|id| id.krate != LOCAL_CRATE)
        {
            let krate = crate_name(id.krate);
            if !is_trusted(cx, krate) {
                return Origin::Macro {
                    name,
                    defined_in: Some(krate),
                };
            }
            written |= matches!(kind, MacroKind::Attr | MacroKind::Derive) && krate != sym::core;
            if !crates.contains(&krate) {
                crates.push(krate);
            }
        }
        current = data.parent;
    }
    if written {
        Origin::Trusted { crates }
    } else {
        Origin::Unwritten
    }
}

/// Where to report something at `span`: `span` itself, or, if a macro from another crate wrote
/// it, the call of the macro in this crate, since rustc drops reports inside other crates' macros.
pub(crate) fn report_span(cx: &EarlyContext<'_>, span: Span) -> Span {
    if span.in_external_macro(cx.sess().source_map()) {
        span.source_callsite()
    } else {
        span
    }
}

/// The path of `attribute`, with segments joined by `::`, as the scan writes paths.
pub(crate) fn path_text(attribute: &Attribute) -> String {
    match &attribute.kind {
        AttrKind::Normal(normal) => normal
            .item
            .path
            .segments
            .iter()
            .map(|segment| segment.ident.as_str())
            .collect::<Vec<_>>()
            .join("::"),
        AttrKind::DocComment(..) => "doc".to_owned(),
        AttrKind::Synthetic(_) => String::new(),
        AttrKind::Generated(generated) => generated
            .path()
            .iter()
            .map(Symbol::as_str)
            .collect::<Vec<_>>()
            .join("::"),
    }
}

/// `attribute` as the meta item the scan would have read had it been written out; its arguments
/// are `MetaKind::Other` if they aren't meta items.
pub(crate) fn meta(attribute: &Attribute) -> Meta {
    attribute.meta().map_or_else(
        || Meta {
            path: path_text(attribute),
            kind: MetaKind::Other,
            span: attribute.span,
        },
        |item| meta_item(&item),
    )
}

fn meta_item(item: &MetaItem) -> Meta {
    let path = item
        .path
        .segments
        .iter()
        .map(|segment| segment.ident.as_str())
        .collect::<Vec<_>>()
        .join("::");
    let kind = match &item.kind {
        MetaItemKind::Word => MetaKind::Word,
        MetaItemKind::List(items) => MetaKind::List(
            items
                .iter()
                .map(|inner| match inner {
                    MetaItemInner::MetaItem(item) => meta_item(item),
                    MetaItemInner::Lit(literal) => Meta {
                        path: String::new(),
                        kind: MetaKind::Other,
                        span: literal.span,
                    },
                })
                .collect(),
        ),
        MetaItemKind::NameValue(literal) => MetaKind::Value(match literal.kind {
            LitKind::Str(value, _) => Some(value.to_string()),
            _ => None,
        }),
    };
    Meta {
        path,
        kind,
        span: item.span,
    }
}

/// Adds the span of every token in `tokens` to `spans`, delimiters included.
fn token_spans(tokens: &TokenStream, spans: &mut Vec<Span>) {
    for tree in tokens.iter() {
        match tree {
            TokenTree::Token(token, _) => spans.push(token.span),
            TokenTree::Delimited(delimiters, _, _, inner) => {
                spans.extend([delimiters.open, delimiters.close]);
                token_spans(inner, spans);
            }
        }
    }
}

/// The origin of a token at `span`, which an expansion produced.
fn macro_origin(cx: &EarlyContext<'_>, span: Span) -> Origin {
    let data = span.ctxt().outer_expn_data();
    let ExpnKind::Macro(_, name) = data.kind else {
        return Origin::Trusted { crates: Vec::new() };
    };
    let defined_in = data
        .macro_def_id
        .filter(|id| id.krate != LOCAL_CRATE)
        .map(|id| crate_name(id.krate));
    match defined_in {
        Some(krate) if is_trusted(cx, krate) => Origin::Trusted {
            crates: vec![krate],
        },
        _ => Origin::Macro { name, defined_in },
    }
}

pub(crate) fn crate_name(krate: CrateNum) -> Symbol {
    rustc_middle::ty::tls::with(|tcx| tcx.crate_name(krate))
}

/// Whether `trusted_macro_crates` in `dylint.toml` lists `krate`.
fn is_trusted(cx: &EarlyContext<'_>, krate: Symbol) -> bool {
    crate::config::get(cx.sess())
        .trusted_macro_crates
        .iter()
        .any(|trusted| trusted == krate.as_str())
}
