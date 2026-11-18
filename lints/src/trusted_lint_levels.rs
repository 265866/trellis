//! The lint levels that trusted macros write over code written in the crate's source, which
//! `suppression_policy` checks against `trusted_macro_lints` in `dylint.toml`.
//!
//! A macro from a crate `trusted_macro_crates` lists may write `allow`, `warn` and `expect`, and
//! most write them only on code they generate. Some put them around code their caller wrote:
//! `#[derive(clap::Parser)]` writes `allow(clippy::restriction, ..)` on the impl that holds the
//! expressions of each `#[arg(..)]`, and `#[tracing::instrument]` writes `allow`s around the
//! function's body. Such an attribute may name only lints `trusted_macro_lints` lists for the
//! macro's crate, and never a lint group; one of clippy's own attributes, such as
//! `#[clippy::msrv = ".."]`, may not cover such code at all. Any code counts, a literal or a path
//! too: a lint group reaches whatever the literal or path sits in, such as the generic arguments
//! of a path or the arguments of a call.
//!
//! Code counts as written in the source if the source holds it at its span. A proc macro that
//! gives its own code the caller's spans, as `#[tokio::main]` does so errors point at the body,
//! writes code whose text isn't there, so that code isn't the caller's. Code the caller wrote
//! always has a part whose text is there, a literal or a name at the least, so the macro's own
//! code around it doesn't hide it. A name that a match arm the macro wrote binds, read in that
//! arm, is the macro's local variable, whatever span it has, such as the `transparent` thiserror
//! binds and reads at `#[error(transparent)]`; no lint reports reading a local variable by a name
//! that doesn't start with `_`.
//!
//! A path a macro parses from a string the caller wrote, as serde parses the function of
//! `#[serde(default = "defaults::zero")]` or the bound of `#[serde(bound = "T: Default")]`, has
//! the string's span. It counts as the caller's code, a type or a generic argument as much as an
//! expression, unless it is a path expression of a name alone, not called, that a format
//! string's `{name}` reads, such as a field in thiserror's `#[error("no user {name}")]`.

use std::mem;

use rustc_ast::visit::{self, AssocCtxt, Visitor};
use rustc_ast::{
    Arm, AssocItem, Attribute, Block, Crate, Expr, ExprField, ExprKind, FieldDef, ForeignItem,
    GenericParam, Item, ItemKind, Local, ModKind, NodeId, Param, Pat, PatField, PatKind, Path,
    QSelf, StmtKind, Ty, TyKind, UseTree, UseTreeKind, Variant,
};
use rustc_data_structures::fx::{FxHashMap, FxHashSet};
use rustc_hir::def::{DefKind, Res};
use rustc_hir::def_id::DefId;
use rustc_lint::{EarlyContext, LintContext};
use rustc_middle::middle::resolve::{PartialRes, ResolverAstLowering};
use rustc_middle::ty::{TyCtxt, tls};
use rustc_span::hygiene::MacroKind;
use rustc_span::symbol::kw;
use rustc_span::{ExpnKind, Ident, Span, Symbol, sym};

use crate::attribute_origin::{self, Origin};
use crate::command_line_lint::span_lint_from_command_line;
use crate::imports;
use crate::local_source::{LocalSource, MetaKind};
use crate::string_literal::{self, Literal};
use crate::suppression_policy::SUPPRESSION_POLICY;

/// Checks every lint level a trusted macro wrote over code written in the source, and that
/// `trusted_macro_lints` lists no group and only crates `trusted_macro_crates` lists.
pub(crate) fn check(cx: &EarlyContext<'_>, source: &LocalSource, krate: &Crate, groups: &[String]) {
    let config = crate::config::get(cx.sess());
    for (crate_name, lints) in &config.trusted_macro_lints {
        if !config.trusted_macro_crates.contains(crate_name) {
            cx.sess().dcx().err(format!(
                "`trusted_macro_lints` in `dylint.toml` lists `{crate_name}`, which \
                 `trusted_macro_crates` doesn't: only a trusted crate's macros may set lint levels"
            ));
        }
        for lint in lints.iter().filter(|lint| groups.contains(lint)) {
            cx.sess().dcx().err(format!(
                "`trusted_macro_lints` in `dylint.toml` lists the lint group `{lint}` for \
                 `{crate_name}`: a group silences lints nobody named, so list each lint"
            ));
        }
    }
    let mut definitions = Definitions(FxHashSet::default());
    visit::walk_crate(&mut definitions, krate);
    let prelude = krate
        .items
        .iter()
        .find(|item| {
            item.attrs
                .iter()
                .any(|attribute| attribute.has_name(sym::prelude_import))
        })
        .and_then(|item| {
            let ItemKind::Use(tree) = &item.kind else {
                return None;
            };
            let module = tls::with(|tcx| {
                imports::external_module(
                    tcx,
                    tree.prefix
                        .segments
                        .iter()
                        .map(|segment| segment.ident.name),
                )
            });
            Some(Glob {
                module,
                importer: None,
                span: item.span,
                path: String::new(),
            })
        });
    let mut levels = Levels {
        cx,
        source,
        groups,
        definitions: &definitions.0,
        prelude: prelude.as_ref(),
        scopes: vec![Scope::of(krate.items.iter().map(|item| &**item), true)],
        reported: Vec::new(),
    };
    visit::walk_crate(&mut levels, krate);
}

/// Lints that never inspect a literal: they report names, paths, bindings, attributes, imports or
/// lifetimes. A lint level that names only these hides nothing in code that is only literals, such
/// as the `rename` strings serde writes into its impls or the message of thiserror's
/// `#[error("..")]`. A lint missing here counts as one that may, so the list can only be too short.
const NOT_ON_LITERALS: &[&str] = &[
    "deprecated",
    "non_camel_case_types",
    "non_exhaustive_omitted_patterns",
    "non_upper_case_globals",
    "unused_attributes",
    "unused_extern_crates",
    "unused_imports",
    "unused_qualifications",
    "unused_variables",
    "clippy::absolute_paths",
    "clippy::elidable_lifetime_names",
    "clippy::needless_lifetimes",
    "clippy::redundant_field_names",
    "clippy::used_underscore_binding",
    "clippy::useless_attribute",
];

/// Lints that never fire on a short path, as `Written::short` says, or on a call of one: a bare
/// name, or a name through one module or type, such as `defaults::zero` or `Option::is_none`,
/// written as plain text with no attributes, leading `::`, `crate`, `self` or `super`, whose last
/// segment alone wouldn't name the same item, or that names an associated item, as `T::Err` and
/// `<T as FromStr>::Err` do; in a string, every path of a type or bound such as `Vec<u8>` or
/// `T: Deserialize<'de>` is one, its generic arguments included. A lint level that
/// names only these hides nothing in a function serde names that way, as in
/// `#[serde(deserialize_with = "defaults::present")]`, in a type or bound serde parses from a
/// string, or in the `limits::WIDTH` of a field type serde copies into its impls. A lint missing
/// here counts as one that may fire, and a group never qualifies.
///
/// - `unused_qualifications` reports a qualifier that could be dropped because the last segment
///   alone names the same item, which a short path's last segment doesn't, and never reports an
///   associated item, which the compiler resolves only once it knows the types.
/// - `clippy::absolute_paths` reports a path of more segments than the two it allows by default,
///   counting the `deserialize` serde adds to the module of `#[serde(with = "flag")]`.
/// - `unused_attributes` reports attributes, and a short path carries none.
/// - `non_upper_case_globals` reports a global's name where it is defined, or a constant a pattern
///   names; it only suggests renaming the global where an expression names it. A path in a pattern
///   isn't a short path.
const NOT_ON_SHORT_PATHS: &[&str] = &[
    "non_upper_case_globals",
    "unused_attributes",
    "unused_qualifications",
    "clippy::absolute_paths",
];

/// Walks the expanded crate, checking the lint levels on each node that can carry attributes.
struct Levels<'a, 'cx> {
    cx: &'a EarlyContext<'cx>,
    source: &'a LocalSource,
    groups: &'a [String],
    /// The span of the name of every item, field, variant and generic parameter.
    definitions: &'a FxHashSet<Span>,
    /// The glob import of the prelude the compiler adds to the crate's root, if it adds one. The
    /// compiler's resolution doesn't hold the prelude's module, so `check` finds it by name.
    prelude: Option<&'a Glob>,
    /// The names in scope in each module and block the walk is inside, innermost last.
    scopes: Vec<Scope>,
    /// The spans already reported, so a node under several such attributes is reported once.
    reported: Vec<Span>,
}

impl Levels<'_, '_> {
    /// Checks the lint levels in `attributes`, on a node that `walk` walks with a visitor, and
    /// reports the code written in the source under them once, with every lint they set that
    /// isn't listed for the macros' crates.
    fn check(&mut self, attributes: &[Attribute], walk: impl Fn(&mut Written<'_, '_>)) {
        let mut unlisted: Vec<(String, bool)> = Vec::new();
        let mut writers: Vec<Symbol> = Vec::new();
        for attribute in attributes {
            let path = attribute_origin::path_text(attribute);
            let tool = path.starts_with("clippy::");
            if !(tool || matches!(path.as_str(), "allow" | "warn" | "expect")) {
                continue;
            }
            let Origin::Trusted { crates } =
                attribute_origin::origin(self.cx, self.source, attribute)
            else {
                continue;
            };
            let found = if tool {
                vec![(path.clone(), true)]
            } else {
                self.unlisted(&attribute_origin::meta(attribute).items_owned(), &crates)
            };
            if crates.is_empty() || found.is_empty() {
                continue;
            }
            for lint in found {
                if !unlisted.contains(&lint) {
                    unlisted.push(lint);
                }
            }
            for krate in crates {
                if !writers.contains(&krate) {
                    writers.push(krate);
                }
            }
        }
        if unlisted.is_empty() {
            return;
        }
        let module = self
            .scopes
            .iter()
            .rposition(|scope| scope.module)
            .unwrap_or_default();
        let mut written = Written {
            cx: self.cx,
            definitions: self.definitions,
            prelude: self.prelude,
            scopes: self.scopes[module..].to_vec(),
            found: None,
            in_string: false,
            glob: None,
            path: None,
            in_pattern: false,
            qualified_self: false,
            macro_bindings: Vec::new(),
            literal: None,
            callee: None,
        };
        walk(&mut written);
        let may_fire = |carved_out: &[&str]| {
            unlisted
                .iter()
                .any(|(lint, group)| *group || !carved_out.contains(&lint.as_str()))
        };
        let (code, in_string, glob) = match (written.found, written.path, written.literal) {
            (Some(code), ..) => (code, written.in_string, written.glob),
            (None, Some(path), _) if may_fire(NOT_ON_SHORT_PATHS) => (path, false, None),
            (None, _, Some(literal)) if may_fire(NOT_ON_LITERALS) => (literal, false, None),
            _ => return,
        };
        if self.reported.contains(&code) {
            return;
        }
        self.reported.push(code);
        self.report(code, &unlisted, &writers, in_string, glob.as_ref());
    }

    /// The lints of `items`, a lint level's, that no crate of `crates` may set over the source,
    /// and whether each is a group.
    fn unlisted(&self, items: &[(String, bool)], crates: &[Symbol]) -> Vec<(String, bool)> {
        let config = crate::config::get(self.cx.sess());
        items
            .iter()
            .filter(|(_, is_word)| *is_word)
            .filter_map(|(lint, _)| {
                let group = self.groups.contains(lint);
                let listed = crates.iter().any(|krate| {
                    config
                        .trusted_macro_lints
                        .get(krate.as_str())
                        .is_some_and(|lints| lints.contains(lint))
                });
                (group || !listed).then(|| (lint.clone(), group))
            })
            .collect()
    }

    /// Reports `code`, written in the source under lint levels or clippy attributes that a macro
    /// from `crates` wrote, naming each of `unlisted`, the lints and attributes no configuration
    /// allows there. `in_string` says the code is a path a macro parsed from a string, and `glob`
    /// the glob import that brings the name, if that is why the path isn't short.
    fn report(
        &self,
        code: Span,
        unlisted: &[(String, bool)],
        crates: &[Symbol],
        in_string: bool,
        glob: Option<&(Glob, Symbol)>,
    ) {
        let names = unlisted
            .iter()
            .map(|(lint, _)| format!("`{lint}`"))
            .collect::<Vec<_>>()
            .join(", ");
        let writers = crates
            .iter()
            .map(|krate| format!("`{krate}`"))
            .collect::<Vec<_>>()
            .join(" or ");
        let help = if unlisted.iter().any(|(_, group)| *group) {
            "a lint group, or one of clippy's own attributes such as `clippy::msrv`, changes what \
             clippy checks in all of this code, so no configuration can allow it: move the code \
             into a function or a constant with a name, and refer to it here by that name, so \
             clippy checks it"
        } else if in_string {
            "write each path in the string so these lints have nothing to report: at most two \
             plain segments, such as `defaults::retries`, `Vec<u8>` or `T: Deserialize<'de>`, \
             with no `crate::`, `self::`, `super::` or leading `::`, no comment, escape, raw \
             identifier or turbofish, and no last segment that names the same item by itself; \
             or, once you have read that the macro writes the level and that no code following \
             the style guide trips the lint, list the lint for the crate in \
             `trusted_macro_lints` in `dylint.toml`, which is a rule change"
        } else {
            "move the code into a function or a constant with a name, and refer to it here by \
             that name, so clippy checks it; or, once you have read that the macro writes the \
             level and that no code following the style guide trips the lint, list the lint for \
             the crate in `trusted_macro_lints` in `dylint.toml`, which is a rule change"
        };
        let source_map = self.cx.sess().source_map();
        let through_glob = glob.map(|(glob, name)| {
            let line = source_map.lookup_char_pos(glob.span.lo()).line;
            format!(
                "`{name}` is also in scope by itself through the glob import `{}` on line \
                 {line}, so `unused_qualifications` would report the path: import the names the \
                 module uses one by one instead of the glob",
                glob.path
            )
        });
        let helps: Vec<&str> = std::iter::once(help)
            .chain(through_glob.as_deref())
            .collect();
        span_lint_from_command_line(
            self.cx,
            SUPPRESSION_POLICY,
            attribute_origin::report_span(self.cx, code),
            &format!(
                "code written in the source under the lint levels of {names} that a macro from \
                 {writers} writes, which `trusted_macro_lints` in `dylint.toml` doesn't allow there"
            ),
            &helps,
        );
    }
}

impl<'ast> Visitor<'ast> for Levels<'_, '_> {
    fn visit_item(&mut self, item: &'ast Item) {
        self.check(&item.attrs, |written| written.visit_item(item));
        if let ItemKind::Mod(_, _, ModKind::Loaded(items, ..)) = &item.kind {
            self.scopes
                .push(Scope::of(items.iter().map(|item| &**item), true));
            visit::walk_item(self, item);
            self.scopes.pop();
        } else {
            visit::walk_item(self, item);
        }
    }

    fn visit_block(&mut self, block: &'ast Block) {
        self.scopes.push(Scope::of_block(block));
        visit::walk_block(self, block);
        self.scopes.pop();
    }

    fn visit_assoc_item(&mut self, item: &'ast AssocItem, ctxt: AssocCtxt) {
        self.check(&item.attrs, |written| written.visit_assoc_item(item, ctxt));
        visit::walk_assoc_item(self, item, ctxt);
    }

    fn visit_foreign_item(&mut self, item: &'ast ForeignItem) {
        self.check(&item.attrs, |written| written.visit_foreign_item(item));
        visit::walk_item(self, item);
    }

    fn visit_expr(&mut self, expr: &'ast Expr) {
        self.check(&expr.attrs, |written| written.visit_expr(expr));
        visit::walk_expr(self, expr);
    }

    fn visit_local(&mut self, local: &'ast Local) {
        self.check(&local.attrs, |written| written.visit_local(local));
        visit::walk_local(self, local);
    }

    fn visit_arm(&mut self, arm: &'ast Arm) {
        self.check(&arm.attrs, |written| written.visit_arm(arm));
        visit::walk_arm(self, arm);
    }

    fn visit_field_def(&mut self, field: &'ast FieldDef) {
        self.check(&field.attrs, |written| written.visit_field_def(field));
        visit::walk_field_def(self, field);
    }

    fn visit_variant(&mut self, variant: &'ast Variant) {
        self.check(&variant.attrs, |written| written.visit_variant(variant));
        visit::walk_variant(self, variant);
    }

    fn visit_param(&mut self, param: &'ast Param) {
        self.check(&param.attrs, |written| written.visit_param(param));
        visit::walk_param(self, param);
    }

    fn visit_generic_param(&mut self, param: &'ast GenericParam) {
        self.check(&param.attrs, |written| written.visit_generic_param(param));
        visit::walk_generic_param(self, param);
    }

    fn visit_expr_field(&mut self, field: &'ast ExprField) {
        self.check(&field.attrs, |written| written.visit_expr_field(field));
        visit::walk_expr_field(self, field);
    }

    fn visit_pat_field(&mut self, field: &'ast PatField) {
        self.check(&field.attrs, |written| written.visit_pat_field(field));
        visit::walk_pat_field(self, field);
    }
}

/// The names a module or a block brings into scope by itself, each with what it may resolve to:
/// those of its items and imports, and its glob imports.
#[derive(Clone)]
struct Scope {
    names: FxHashMap<Symbol, Vec<Binding>>,
    globs: Vec<Glob>,
    /// Whether this is a module's scope, which the scopes around it don't reach into.
    module: bool,
}

/// What a name in a scope resolves to: a definition, or `None` where the compiler's resolution
/// isn't known, which may be anything.
type Binding = Option<Res<NodeId>>;

impl Scope {
    /// The scope of a module or block whose items are `items`, the prelude the compiler imports
    /// aside. It reads the resolver's results from the type context, which holds them: the early
    /// lint passes run inside the compiler's `early_lint_checks` query, after name resolution.
    fn of<'a>(items: impl IntoIterator<Item = &'a Item>, module: bool) -> Self {
        let mut scope = Self {
            names: FxHashMap::default(),
            globs: Vec::new(),
            module,
        };
        tls::with(|tcx| {
            let resolver = tcx.resolver_for_lowering().0.borrow();
            for item in items {
                match &item.kind {
                    ItemKind::Use(_)
                        if item
                            .attrs
                            .iter()
                            .any(|attribute| attribute.has_name(sym::prelude_import)) => {}
                    ItemKind::Use(tree) => {
                        let resolutions = Resolutions {
                            resolver: &resolver,
                            owner: item.id,
                        };
                        scope.import(tree, item.id, &[], &resolutions);
                    }
                    kind => {
                        if let Some(ident) = kind.ident() {
                            let definition = resolver.owners.get(&item.id).map(|owner| {
                                let def_id = owner.def_id.to_def_id();
                                Res::Def(tcx.def_kind(def_id), def_id)
                            });
                            scope.names.entry(ident.name).or_default().push(definition);
                        }
                    }
                }
            }
        });
        scope
    }

    /// The scope of the items in `block`, such as the imports in a function's body.
    fn of_block(block: &Block) -> Self {
        Self::of(
            block
                .stmts
                .iter()
                .filter_map(|statement| match &statement.kind {
                    StmtKind::Item(item) => Some(&**item),
                    _ => None,
                }),
            false,
        )
    }

    /// Adds the names `tree`, a `use` tree whose node is `id` inside trees whose paths are
    /// `outer`, imports, each with what `resolutions` says it resolves to.
    fn import(
        &mut self,
        tree: &UseTree,
        id: NodeId,
        outer: &[Symbol],
        resolutions: &Resolutions<'_, '_>,
    ) {
        let mut path = outer.to_vec();
        path.extend(
            tree.prefix
                .segments
                .iter()
                .map(|segment| segment.ident.name),
        );
        match &tree.kind {
            UseTreeKind::Simple(rename) => {
                let mut segments = tree.prefix.segments.iter().rev();
                let last = segments.next().map(|segment| segment.ident.name);
                let name = match rename {
                    Some(rename) => Some(rename.name),
                    None if last == Some(kw::SelfLower) => {
                        segments.next().map(|segment| segment.ident.name)
                    }
                    None => last,
                };
                if let Some(name) = name {
                    self.names
                        .entry(name)
                        .or_default()
                        .extend(resolutions.import(id));
                }
            }
            UseTreeKind::Nested { items, .. } => {
                for nested in items {
                    self.import(&nested.inner, nested.id, &path, resolutions);
                }
            }
            UseTreeKind::Glob(_) => self.globs.push(Glob {
                module: resolutions.glob(id),
                importer: resolutions.importer(),
                span: tree.span(),
                path: path
                    .iter()
                    .map(|name| {
                        if *name == kw::PathRoot {
                            ""
                        } else {
                            name.as_str()
                        }
                    })
                    .chain(["*"])
                    .collect::<Vec<_>>()
                    .join("::"),
            }),
        }
    }
}

/// What the compiler resolved the trees of the `use` item `owner` to.
struct Resolutions<'r, 'tcx> {
    resolver: &'r ResolverAstLowering<'tcx>,
    owner: NodeId,
}

impl Resolutions<'_, '_> {
    /// What the import of the tree `id` may resolve to, in each namespace it imports into.
    fn import(&self, id: NodeId) -> Vec<Binding> {
        self.resolver
            .owners
            .get(&self.owner)
            .and_then(|owner| owner.import_res.get(&id))
            .map_or_else(
                || vec![None],
                |per_ns| per_ns.present_items().map(Some).collect(),
            )
    }

    /// The module the `use` item is in, whose view of other modules' names its globs import.
    fn importer(&self) -> Option<DefId> {
        let owner = self.resolver.owners.get(&self.owner)?;
        Some(tls::with(|tcx| {
            tcx.parent_module_from_def_id(owner.def_id).to_def_id()
        }))
    }

    /// The module, enum or trait the glob import of the tree `id` imports from.
    fn glob(&self, id: NodeId) -> Option<DefId> {
        match self
            .resolver
            .partial_res_map
            .get(&id)
            .and_then(PartialRes::full_res)
        {
            Some(Res::Def(DefKind::Mod | DefKind::Enum | DefKind::Trait, module)) => Some(module),
            _ => None,
        }
    }
}

/// A glob import, `use path::*;`.
#[derive(Clone)]
struct Glob {
    /// The module, enum or trait it imports from, or `None` if the compiler didn't resolve it to
    /// one, so it may bring any name.
    module: Option<DefId>,
    /// The module the import is in, which sees only the names visible to it, or `None` for the
    /// prelude, whose names are all public.
    importer: Option<DefId>,
    span: Span,
    /// The whole path of the import, as `super::defaults::*` for the `*` of
    /// `use super::defaults::{self, *};`.
    path: String,
}

impl Glob {
    /// What the glob brings into scope by `name` in the namespace of `target`: `None` if nothing,
    /// else whether it brings `target`. A glob of a module the compiler's resolution doesn't say
    /// may bring `target`.
    fn brings(&self, name: Symbol, target: Res<NodeId>) -> Option<bool> {
        let Some(glob) = self.module else {
            return Some(true);
        };
        tls::with(|tcx| {
            let children = match glob.as_local() {
                Some(local) => tcx.module_children_local(local),
                None => tcx.module_children(glob),
            };
            children
                .iter()
                .filter(|child| {
                    child.ident.name == name
                        && same_namespace(child.res.expect_non_local(), target)
                        && self
                            .importer
                            .is_none_or(|importer| child.vis.is_accessible_from(importer, tcx))
                })
                .map(|child| same_item(tcx, child.res.expect_non_local(), target))
                .reduce(|any, brings| any || brings)
        })
    }
}

/// Whether `binding` may be in the namespace of `target`, so that it shadows or is `target`'s
/// binding: a resolution in another namespace binds the same name independently.
fn same_namespace(binding: Res<NodeId>, target: Res<NodeId>) -> bool {
    match (binding.ns(), target.ns()) {
        (Some(binding), Some(target)) => binding == target,
        _ => true,
    }
}

/// Whether `a` and `b` are the same item, a tuple or unit struct or variant the same as its
/// constructor.
fn same_item(tcx: TyCtxt<'_>, a: Res<NodeId>, b: Res<NodeId>) -> bool {
    let item = |res: Res<NodeId>| match res {
        Res::Def(DefKind::Ctor(..), ctor) => Some(tcx.parent(ctor)),
        res => res.opt_def_id(),
    };
    a == b || item(a).is_some_and(|a| item(b) == Some(a))
}

/// Whether a name in scope by itself resolves to an item, and how.
enum InScope<'a> {
    No,
    Directly,
    /// Through the glob import.
    Glob(&'a Glob),
}

/// Whether a path is short, as `Written::short` says.
#[derive(Clone)]
enum Short {
    Yes,
    No,
    /// It would be, but for the name, whose glob import brings it into scope by itself.
    Glob(Glob, Symbol),
}

/// What a string the caller wrote holds, at the span of a path a macro parsed from it.
enum InString {
    /// Plain paths only, as `string_literal::paths` reads them, the path's first segments among
    /// them: each path's segments.
    Paths(Vec<Vec<Symbol>>),
    /// A format string with a `{name}` that reads the path, a single name. Only a path
    /// expression that isn't called is a placeholder's; anywhere else the path is code.
    Placeholder,
    /// Anything else, which may spell any path.
    Other,
}

/// Finds the first code written in the source inside a node, as the module docs say.
struct Written<'a, 'cx> {
    cx: &'a EarlyContext<'cx>,
    definitions: &'a FxHashSet<Span>,
    /// The glob import of the prelude the compiler adds to the crate's root, if it adds one.
    prelude: Option<&'a Glob>,
    /// The names in scope in the innermost module the walk is inside, and in each block inside it,
    /// innermost last.
    scopes: Vec<Scope>,
    /// The first code written in the source that is neither a literal nor a short path.
    found: Option<Span>,
    /// Whether `found` is a path a macro parsed from a string.
    in_string: bool,
    /// The glob import, and the name it brings, that keeps `found` from being a short path.
    glob: Option<(Glob, Symbol)>,
    /// The first short path written in the source, or call of one, as `short` says.
    path: Option<Span>,
    /// Whether the walk is inside a pattern, where a path of one segment may name a constant.
    in_pattern: bool,
    /// Whether the next path the walk visits follows a qualified self, as `Trait::Assoc` follows
    /// the `<T as` of `<T as Trait>::Assoc`.
    qualified_self: bool,
    /// The names that patterns the macro wrote bind in the match arms the walk is inside, with
    /// the hygiene a local variable resolves by.
    macro_bindings: Vec<Ident>,
    /// The first literal written in the source.
    literal: Option<Span>,
    /// The callee of the last call the walk visited, which the walk visits next. A name a macro
    /// calls is code even if a `{..}` in its string spells it, as serde calls the function a
    /// `deserialize_with` string names.
    callee: Option<NodeId>,
}

impl Written<'_, '_> {
    /// The source text at `span`, if `span` is in a file of the source.
    fn text(&self, span: Span) -> Option<String> {
        if span.from_expansion() || span.is_dummy() {
            return None;
        }
        self.cx.sess().source_map().span_to_snippet(span).ok()
    }

    /// Whether the source holds `name` at `span`, as written or as a raw identifier, in the
    /// composed form the compiler interns names in.
    fn holds(&self, span: Span, name: Symbol) -> bool {
        self.text(span).is_some_and(|text| {
            let text = string_literal::normalized(&text);
            text.strip_prefix("r#").unwrap_or(&text) == name.as_str()
        })
    }

    /// Whether the source holds `name` at `span` in an expression: a name at the span of a
    /// definition's is one a macro reused, such as the type and field names a derive writes into
    /// its impls.
    fn reads(&self, span: Span, name: Symbol) -> bool {
        !self.definitions.contains(&span) && self.holds(span, name)
    }

    /// Whether the source holds `expr` at its span: its text starts or ends the way the
    /// expression's kind does, and the names in it are there.
    fn is_written(&self, expr: &Expr) -> bool {
        let Some(text) = self.text(expr.span) else {
            return false;
        };
        let text = text.trim();
        let starts = |prefixes: &[&str]| prefixes.iter().any(|prefix| text.starts_with(prefix));
        match &expr.kind {
            ExprKind::MethodCall(call) => self.reads(call.seg.ident.span, call.seg.ident.name),
            ExprKind::Call(callee, _) => self.is_written(callee),
            ExprKind::Path(_, path) => {
                path.segments
                    .last()
                    .is_some_and(|segment| self.reads(segment.ident.span, segment.ident.name))
                    || self.in_string(expr.span, path).is_some_and(|held| {
                        !matches!(held, InString::Placeholder) || self.callee == Some(expr.id)
                    })
            }
            ExprKind::Struct(literal) => literal
                .path
                .segments
                .last()
                .is_some_and(|segment| self.reads(segment.ident.span, segment.ident.name)),
            ExprKind::Field(_, ident) => self.reads(ident.span, ident.name),
            ExprKind::Lit(literal) => text == literal.to_string(),
            ExprKind::Binary(operator, ..) => self
                .text(operator.span)
                .is_some_and(|written| written == operator.node.as_str()),
            ExprKind::AssignOp(operator, ..) => self
                .text(operator.span)
                .is_some_and(|written| written == operator.node.as_str()),
            ExprKind::Unary(operator, inner) => {
                text.starts_with(operator.as_str()) && self.is_written(inner)
            }
            ExprKind::Closure(closure) => {
                starts(&["|", "move", "async", "static", "for"]) && self.is_written(&closure.body)
            }
            ExprKind::Block(block, _) => {
                starts(&["{", "unsafe", "'"]) && self.text(block.span).is_some()
            }
            ExprKind::If(condition, ..) => starts(&["if"]) && self.is_written(condition),
            ExprKind::Match(scrutinee, ..) => {
                (starts(&["match"]) || text.contains(".match")) && self.is_written(scrutinee)
            }
            ExprKind::While(condition, ..) => starts(&["while", "'"]) && self.is_written(condition),
            ExprKind::ForLoop(for_loop) => starts(&["for", "'"]) && self.is_written(&for_loop.iter),
            ExprKind::Let(_, scrutinee, ..) => starts(&["let"]) && self.is_written(scrutinee),
            ExprKind::Loop(..) => starts(&["loop", "'"]),
            ExprKind::Ret(value) => {
                starts(&["return"]) && value.as_ref().is_none_or(|value| self.is_written(value))
            }
            ExprKind::Break(_, value) => {
                starts(&["break"]) && value.as_ref().is_none_or(|value| self.is_written(value))
            }
            ExprKind::Continue(_) => starts(&["continue"]),
            ExprKind::Gen(..) => starts(&["async", "gen"]),
            ExprKind::TryBlock(..) => starts(&["try"]),
            ExprKind::Paren(inner) => starts(&["("]) && self.is_written(inner),
            ExprKind::Tup(items) | ExprKind::Array(items) => {
                starts(&["(", "["]) && items.first().is_none_or(|item| self.is_written(item))
            }
            ExprKind::Repeat(item, _) => starts(&["["]) && self.is_written(item),
            ExprKind::AddrOf(_, _, inner) => starts(&["&"]) && self.is_written(inner),
            ExprKind::Await(inner, keyword) => {
                self.text(*keyword)
                    .is_some_and(|written| written == "await")
                    && self.is_written(inner)
            }
            ExprKind::Try(inner) => text.ends_with('?') && self.is_written(inner),
            ExprKind::Index(base, _, brackets) => {
                self.text(*brackets)
                    .is_some_and(|written| written.starts_with('['))
                    && self.is_written(base)
            }
            ExprKind::Range(start, end, _) => {
                text.contains("..")
                    && (start.is_none() && end.is_none()
                        || start.iter().chain(end).any(|bound| self.is_written(bound)))
            }
            ExprKind::Assign(target, _, equals) => {
                self.text(*equals).is_some_and(|written| written == "=") && self.is_written(target)
            }
            ExprKind::Cast(inner, ty) => {
                self.text(inner.span.between(ty.span))
                    .is_some_and(|keyword| keyword.trim() == "as")
                    && self.is_written(inner)
            }
            _ => true,
        }
    }

    /// Whether `expr` reads a variable that a match arm the macro wrote binds, by a name that
    /// doesn't start with `_`: the name, with its hygiene, resolves to that binding.
    fn reads_macro_binding(&self, expr: &Expr) -> bool {
        let ExprKind::Path(None, path) = &expr.kind else {
            return false;
        };
        let [segment] = path.segments.as_slice() else {
            return false;
        };
        segment.args.is_none()
            && expr.attrs.is_empty()
            && !segment.ident.name.as_str().starts_with('_')
            && self
                .macro_bindings
                .contains(&segment.ident.normalize_to_macro_rules())
    }

    /// Whether `path`, written in the source at `span`, is a short path: one segment, or two whose
    /// last wouldn't resolve to the same item by itself, as `unused_qualifications` checks, with
    /// no `crate`, `self` or `super`, and no `Self` but as the first segment, written as plain
    /// text with no generic arguments, as in `limits::WIDTH`, or as plain paths in a string: as in
    /// `#[serde(default = "defaults::zero")]`, the first segment of one, as the `flag` of the
    /// `flag::deserialize` serde makes of `with = "flag"`, or one of the paths of a type or a
    /// bound, such as the `Deserialize<'de>` of `bound = "T: Deserialize<'de>"`, if every path in
    /// the string has at most two segments too, generic arguments and all.
    ///
    /// A path after a qualified self, as `FromStr::Err` in `<T as FromStr>::Err`, or whose last
    /// segment the compiler didn't resolve by name, as the associated type of `T::Err` or
    /// `Self::Err`, names an associated item, so `unused_qualifications` has nothing to report.
    fn short(&self, path: &Path, span: Span, qualified_self: bool) -> Short {
        let segments = path.segments.as_slice();
        let written = match self.in_string(span, path) {
            Some(InString::Paths(written)) => Some(written),
            Some(_) => return Short::No,
            None => None,
        };
        if segments.is_empty()
            || segments.len() > 2
            || segments.iter().enumerate().any(|(index, segment)| {
                is_qualifying_keyword(index, segment.ident.name)
                    || (written.is_none() && segment.args.is_some())
            })
        {
            return Short::No;
        }
        if written.is_none()
            && segments.len() == 2
            && !self.text(span).is_some_and(|text| {
                string_literal::normalized(text.trim())
                    == format!("{}::{}", segments[0].ident, segments[1].ident)
            })
        {
            return Short::No;
        }
        for written in written.iter().flatten() {
            if written.len() > 2
                || written
                    .iter()
                    .enumerate()
                    .any(|(index, name)| is_qualifying_keyword(index, *name))
            {
                return Short::No;
            }
        }
        let Some(last) = segments.get(1).filter(|_| !qualified_self) else {
            return Short::Yes;
        };
        let Some(target) = resolution(last.id) else {
            return Short::Yes;
        };
        match self.in_scope(last.ident.name, target) {
            InScope::No => Short::Yes,
            InScope::Directly => Short::No,
            InScope::Glob(glob) => Short::Glob(glob.clone(), last.ident.name),
        }
    }

    /// Whether `name` would resolve to `target` by itself where the walk is, or may, in the order
    /// the compiler resolves a bare name in: the blocks the walk is inside, innermost first, then
    /// the innermost module, each by the items and imports it defines in the namespace of
    /// `target`, then by its glob imports; then an external crate named `name` whose root is
    /// `target`, the prelude, and the primitive type `target`. The first scope that binds the
    /// name decides, so a block's function or a module's own `Result` hides an import or the
    /// prelude. A binding the compiler's resolution doesn't say may be `target`.
    fn in_scope(&self, name: Symbol, target: Res<NodeId>) -> InScope<'_> {
        for scope in self.scopes.iter().rev() {
            let bindings: Vec<Binding> = scope
                .names
                .get(&name)
                .into_iter()
                .flatten()
                .copied()
                .filter(|binding| binding.is_none_or(|res| same_namespace(res, target)))
                .collect();
            if !bindings.is_empty() {
                let is_target = tls::with(|tcx| {
                    bindings
                        .iter()
                        .any(|binding| binding.is_none_or(|res| same_item(tcx, res, target)))
                });
                return if is_target {
                    InScope::Directly
                } else {
                    InScope::No
                };
            }
            let mut shadowed = false;
            for glob in &scope.globs {
                match glob.brings(name, target) {
                    Some(true) => return InScope::Glob(glob),
                    Some(false) => shadowed = true,
                    None => {}
                }
            }
            if shadowed {
                return InScope::No;
            }
        }
        let primitive = matches!(target, Res::PrimTy(primitive) if primitive.name() == name);
        let external = self.cx.sess().opts.externs.get(name.as_str()).is_some()
            && matches!(target, Res::Def(DefKind::Mod, root) if root.is_crate_root());
        if primitive
            || external
            || self
                .prelude
                .is_some_and(|prelude| prelude.brings(name, target) == Some(true))
        {
            InScope::Directly
        } else {
            InScope::No
        }
    }

    /// Visits the type of `qself`, the `T` of `<T as Trait>::Assoc`.
    fn visit_qself_type(&mut self, qself: &QSelf) {
        self.visit_ty(&qself.ty);
    }

    /// Records `span` as the first code written in the source that isn't short, a path a macro
    /// parsed from a string if `in_string` holds, and the glob import that kept it from being
    /// short, if `short` says one did.
    fn find(&mut self, span: Span, in_string: bool, short: Option<Short>) {
        self.found = Some(span);
        self.in_string = in_string;
        if let Some(Short::Glob(glob, name)) = short {
            self.glob = Some((glob, name));
        }
    }

    /// What the string literal at `span` holds of `path`, a path a macro parsed from it, or `None`
    /// if the source at `span` isn't a string literal or can't spell the path, so the macro wrote
    /// the path itself. Where `span` has no text, as when a macro joins a string's segments with
    /// one of its own, such as the `deserialize` serde adds to `with = "flag"`, the string is the
    /// first segment's that has text.
    fn in_string(&self, span: Span, path: &Path) -> Option<InString> {
        let text = self.text(span).or_else(|| {
            path.segments
                .iter()
                .find_map(|segment| self.text(segment.ident.span))
        })?;
        let literal = string_literal::literal(&text)?;
        let names: Vec<&str> = path
            .segments
            .iter()
            .filter(|segment| segment.ident.name != kw::PathRoot)
            .map(|segment| segment.ident.name.as_str())
            .collect();
        if !literal.may_spell(&names) {
            return None;
        }
        let Literal::Value(value) = literal else {
            return Some(InString::Other);
        };
        if let Some(written) = string_literal::paths(&value)
            && written.iter().any(|written| names.starts_with(written))
        {
            return Some(InString::Paths(
                written
                    .iter()
                    .map(|path| path.iter().map(|name| Symbol::intern(name)).collect())
                    .collect(),
            ));
        }
        if let [segment] = path.segments.as_slice()
            && segment.args.is_none()
            && string_literal::reads_placeholder(&value, segment.ident.name.as_str())
        {
            return Some(InString::Placeholder);
        }
        Some(InString::Other)
    }

    /// The call in the source of the function-like macro that wrote `expr`, if the source holds
    /// it: its text names the macro and `!`. A proc macro that writes a macro call at the
    /// caller's spans, as clap's derives write `value_parser!` at a field's type, has none.
    fn macro_call(&self, expr: &Expr) -> Option<Span> {
        let data = expr.span.ctxt().outer_expn_data();
        let ExpnKind::Macro(MacroKind::Bang, name) = data.kind else {
            return None;
        };
        let text = self.text(data.call_site)?;
        let (path, _) = text.split_once('!')?;
        let path: String = string_literal::normalized(path)
            .split_whitespace()
            .collect();
        let written = path.rsplit("::").next()?;
        (written.strip_prefix("r#").unwrap_or(written) == last_segment(name))
            .then_some(data.call_site)
    }
}

/// The span of `path` in the source: its own, or, if a macro joined segments of the source with its
/// own, the first segment's that the source holds.
fn written_span(path: &Path) -> Span {
    if path.span.from_expansion() {
        path.segments
            .iter()
            .map(|segment| segment.ident.span)
            .find(|span| !span.from_expansion())
            .unwrap_or(path.span)
    } else {
        path.span
    }
}

/// Whether `name`, the segment at `index` of a path, makes the path qualified in a way
/// `unused_qualifications` or `clippy::absolute_paths` may report: `crate`, `self`, `super`, a
/// leading `::`, or `Self` anywhere but first, where it names the type the path's items are
/// associated with.
fn is_qualifying_keyword(index: usize, name: Symbol) -> bool {
    name.is_path_segment_keyword() && !(index == 0 && name == kw::SelfUpper)
}

/// What the compiler resolved the path segment `id` to by name, if it resolved it fully, as it
/// does every segment `unused_qualifications` can report.
fn resolution(id: NodeId) -> Option<Res<NodeId>> {
    tls::with(|tcx| {
        tcx.resolver_for_lowering()
            .0
            .borrow()
            .partial_res_map
            .get(&id)
            .and_then(PartialRes::full_res)
    })
}

/// The last segment of `path`, a macro's name as its call wrote it, such as `clap::value_parser`.
fn last_segment(path: Symbol) -> String {
    path.as_str()
        .rsplit("::")
        .next()
        .unwrap_or_default()
        .to_owned()
}

impl<'ast> Visitor<'ast> for Written<'_, '_> {
    fn visit_attribute(&mut self, _: &'ast Attribute) {}

    fn visit_pat(&mut self, pattern: &'ast Pat) {
        let outer = mem::replace(&mut self.in_pattern, true);
        visit::walk_pat(self, pattern);
        self.in_pattern = outer;
    }

    fn visit_arm(&mut self, arm: &'ast Arm) {
        let outer = self.macro_bindings.len();
        arm.pat.walk(&mut |pattern| {
            if let PatKind::Ident(_, ident, _) = &pattern.kind
                && ident.span.from_expansion()
            {
                self.macro_bindings.push(ident.normalize_to_macro_rules());
            }
            true
        });
        visit::walk_arm(self, arm);
        self.macro_bindings.truncate(outer);
    }

    fn visit_expr(&mut self, expr: &'ast Expr) {
        if self.found.is_some() || self.reads_macro_binding(expr) {
            return;
        }
        if let ExprKind::Call(callee, _) = &expr.kind {
            self.callee = Some(callee.id);
        }
        if expr.span.from_expansion() {
            if let Some(call) = self.macro_call(expr) {
                self.found = Some(call);
                return;
            }
        } else if self.is_written(expr) {
            if let ExprKind::Lit(_) = expr.kind {
                self.literal.get_or_insert(expr.span);
                return;
            }
            let (callee, arguments) = match &expr.kind {
                ExprKind::Call(callee, arguments) => (&**callee, &arguments[..]),
                _ => (expr, &[][..]),
            };
            let named = match &callee.kind {
                ExprKind::Path(qself, path)
                    if !self.in_pattern && expr.attrs.is_empty() && callee.attrs.is_empty() =>
                {
                    Some((path, qself))
                }
                _ => None,
            };
            match named.map(|(path, qself)| self.short(path, callee.span, qself.is_some())) {
                Some(Short::Yes) => {
                    self.path.get_or_insert(expr.span);
                    if let Some((_, Some(qself))) = named {
                        self.visit_qself_type(qself);
                    }
                    for argument in arguments {
                        self.visit_expr(argument);
                    }
                }
                short => {
                    let in_string =
                        named.is_some_and(|(path, _)| self.in_string(callee.span, path).is_some());
                    self.find(expr.span, in_string, short);
                }
            }
            return;
        }
        if let ExprKind::Path(None, path) = &expr.kind
            && self.callee != Some(expr.id)
            && matches!(self.in_string(expr.span, path), Some(InString::Placeholder))
        {
            return;
        }
        visit::walk_expr(self, expr);
    }

    fn visit_local(&mut self, local: &'ast Local) {
        if self.found.is_some() {
            return;
        }
        if self
            .text(local.span)
            .is_some_and(|text| text.trim_start().starts_with("let"))
        {
            self.found = Some(local.span);
            return;
        }
        visit::walk_local(self, local);
    }

    fn visit_item(&mut self, item: &'ast Item) {
        if self.found.is_some() {
            return;
        }
        if item
            .kind
            .ident()
            .is_some_and(|ident| self.holds(ident.span, ident.name))
            && self.text(item.span).is_some()
        {
            self.found = Some(item.span);
            return;
        }
        if let ItemKind::Mod(_, _, ModKind::Loaded(items, ..)) = &item.kind {
            let outer = mem::replace(
                &mut self.scopes,
                vec![Scope::of(items.iter().map(|item| &**item), true)],
            );
            visit::walk_item(self, item);
            self.scopes = outer;
        } else {
            visit::walk_item(self, item);
        }
    }

    fn visit_block(&mut self, block: &'ast Block) {
        self.scopes.push(Scope::of_block(block));
        visit::walk_block(self, block);
        self.scopes.pop();
    }

    fn visit_ty(&mut self, ty: &'ast Ty) {
        let TyKind::Path(qself, path) = &ty.kind else {
            visit::walk_ty(self, ty);
            return;
        };
        if let Some(qself) = qself {
            self.visit_qself_type(qself);
        }
        self.qualified_self = qself.is_some();
        self.visit_path(path);
    }

    fn visit_path(&mut self, path: &'ast Path) {
        let qualified_self = mem::take(&mut self.qualified_self);
        if self.found.is_some() {
            return;
        }
        let Some(held) = self.in_string(path.span, path) else {
            visit::walk_path(self, path);
            return;
        };
        let span = written_span(path);
        match held {
            _ if self.in_pattern => self.find(span, true, None),
            _ => match self.short(path, path.span, qualified_self) {
                Short::Yes => {
                    self.path.get_or_insert(span);
                    visit::walk_path(self, path);
                }
                short => self.find(span, true, Some(short)),
            },
        }
    }
}

/// Collects the span of the name of every item, field, variant and generic parameter.
struct Definitions(FxHashSet<Span>);

impl<'ast> Visitor<'ast> for Definitions {
    fn visit_item(&mut self, item: &'ast Item) {
        self.0.extend(item.kind.ident().map(|ident| ident.span));
        visit::walk_item(self, item);
    }

    fn visit_assoc_item(&mut self, item: &'ast AssocItem, ctxt: AssocCtxt) {
        self.0.extend(item.kind.ident().map(|ident| ident.span));
        visit::walk_assoc_item(self, item, ctxt);
    }

    fn visit_field_def(&mut self, field: &'ast FieldDef) {
        self.0.extend(field.ident.map(|ident| ident.span));
        visit::walk_field_def(self, field);
    }

    fn visit_variant(&mut self, variant: &'ast Variant) {
        self.0.insert(variant.ident.span);
        visit::walk_variant(self, variant);
    }

    fn visit_generic_param(&mut self, param: &'ast GenericParam) {
        self.0.insert(param.ident.span);
        visit::walk_generic_param(self, param);
    }
}

impl crate::local_source::Meta {
    /// The items of this meta item's list, each with whether it is a bare path.
    fn items_owned(&self) -> Vec<(String, bool)> {
        self.items()
            .iter()
            .map(|item| (item.path.clone(), matches!(item.kind, MetaKind::Word)))
            .collect()
    }
}
