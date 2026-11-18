use clippy_utils::diagnostics::span_lint_hir_and_then;
use rustc_ast::attr::AttributeExt;
use rustc_data_structures::fx::{FxHashMap, FxHashSet};
use rustc_hir::def::{DefKind, Res};
use rustc_hir::def_id::{CRATE_DEF_ID, DefId, LocalDefId};
use rustc_hir::intravisit::{
    Visitor, walk_block, walk_body, walk_expr, walk_impl_item, walk_item, walk_path,
};
use rustc_hir::{
    Block, BodyId, Expr, ExprKind, HirId, ImplItem, Item, ItemKind, Mod, Path, PathSegment, QPath,
    StmtKind,
};
use rustc_lint::{LateContext, LateLintPass};
use rustc_lint_defs::{declare_lint, declare_lint_pass};
use rustc_middle::hir::nested_filter;
use rustc_middle::ty::{TypeckResults, Visibility};
use rustc_span::def_id::LocalModId;
use rustc_span::hygiene::MacroKind;
use rustc_span::{ExpnKind, Span, Symbol, kw, sym};

use crate::imports::{Import, external_module, imports};
use crate::test_code::is_test_code;

declare_lint! {
    /// Checks for a private module-level `use` of a trait whose items are used in only one
    /// function of the module.
    ///
    /// The Google Rust Style Guide, "Prefer locally-scoped trait imports": "Import traits within
    /// function bodies, unless the trait's methods are used extensively throughout the file."
    ///
    /// A use is a method call or path that resolves to the trait or one of its items, including
    /// in closures and items nested in the function. The import isn't checked when the trait is
    /// named outside function bodies (a bound, an `impl` header, a field or signature type, a
    /// constant), or when another module could be relying on it through `use super::*` or
    /// `use super::Trait`, or when an intra-doc link in the docs of the module or an item in it
    /// starts with the import's name, as in ``[`FromStr::from_str`]``, which moving the import
    /// would leave unresolved. Macro output counts where it expands, except derive output, which
    /// names traits by absolute path. A derive that the import brings by the trait's name, as
    /// `use serde::Deserialize;` brings `#[derive(Deserialize)]`, is a use where the derive is
    /// written, unless the prelude brings the same derive by that name too, as it does `Debug`, or
    /// a block around the derive binds the name itself, with a `use` or a glob.
    ///
    /// Test-only code (`#[cfg(test)]` items and `#[test]` functions) is left out: its imports
    /// aren't checked, its uses don't count, and a test module's `use super::*` or
    /// `use super::Trait` doesn't keep the import. Library and test builds then agree, so an
    /// `expect` on the `use` holds in both.
    ///
    /// Code a macro writes isn't checked. Code a proc macro copies from its caller's tokens, such
    /// as the body of a function under an attribute macro, is: its author wrote it there.
    ///
    /// ```rust,ignore
    /// use std::fmt::Write as _;
    ///
    /// fn render(orders: &[u64]) -> String {
    ///     let mut out = String::new();
    ///     for order in orders {
    ///         let _ = writeln!(out, "{order}");
    ///     }
    ///     out
    /// }
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// fn render(orders: &[u64]) -> String {
    ///     use std::fmt::Write as _;
    ///
    ///     let mut out = String::new();
    ///     for order in orders {
    ///         let _ = writeln!(out, "{order}");
    ///     }
    ///     out
    /// }
    /// ```
    pub MODULE_LEVEL_TRAIT_IMPORT,
    Warn,
    "a trait imported for the whole module but used in one function"
}

declare_lint_pass!(ModuleLevelTraitImport => [MODULE_LEVEL_TRAIT_IMPORT]);

impl<'tcx> LateLintPass<'tcx> for ModuleLevelTraitImport {
    /// Reports each trait the module imports but uses in only one function. The report sits at
    /// the `use` itself, so an `expect` on it applies; `check_mod` sits at the module.
    fn check_mod(
        &mut self,
        cx: &LateContext<'tcx>,
        module: &'tcx Mod<'tcx>,
        module_id: LocalModId,
    ) {
        let module_def_id = module_id.to_def_id();
        let imports: Vec<(Import<'_>, DefId)> = module
            .item_ids
            .iter()
            .map(|&id| cx.tcx.hir_item(id))
            .filter(|item| !item.span.from_expansion() && !is_test_code(cx.tcx, item.hir_id()))
            .flat_map(|item| imports(cx.tcx, item))
            .filter_map(|import| {
                import.name?;
                let Some(Res::Def(DefKind::Trait, trait_id)) = import.res.type_ns else {
                    return None;
                };
                let is_private = cx.tcx.visibility(import.def_id)
                    == Visibility::Restricted(
                        cx.tcx.parent_module_from_def_id(import.def_id).to_mod_id(),
                    );
                is_private.then_some((import, trait_id))
            })
            .collect();
        if imports.is_empty() {
            return;
        }
        let reimported = reimported_traits(cx, module_def_id);
        let prelude = prelude_macros(cx);
        let mut usage = TraitUsage {
            cx,
            traits: imports
                .iter()
                .map(|&(_, trait_id)| (trait_id, Uses::default()))
                .collect(),
            derives: imports
                .iter()
                .filter_map(|(import, trait_id)| {
                    let name = import.name?.name;
                    let Some(Res::Def(DefKind::Macro(_), derive)) = import.res.macro_ns else {
                        return None;
                    };
                    (name != kw::Underscore && !prelude.contains(&(name, derive))).then_some(
                        ImportedDerive {
                            name,
                            derive,
                            trait_id: *trait_id,
                        },
                    )
                })
                .collect(),
            site: Site::Outside,
            typeck_results: None,
            block_macros: Vec::new(),
        };
        for &id in module.item_ids {
            usage.visit_item(cx.tcx.hir_item(id));
        }
        let linked = doc_link_names(cx, module_id.to_local_def_id());
        for (import, trait_id) in imports {
            if reimported
                .as_ref()
                .is_none_or(|traits| traits.contains(&trait_id))
            {
                continue;
            }
            if import.name.is_some_and(|name| linked.contains(&name.name)) {
                continue;
            }
            let uses = &usage.traits[&trait_id];
            if uses.outside_functions {
                continue;
            }
            let mut functions = uses.functions.iter();
            let (Some(&function), None) = (functions.next(), functions.next()) else {
                continue;
            };
            let name = cx.tcx.item_name(trait_id);
            let function = cx.tcx.item_name(function.to_def_id());
            span_lint_hir_and_then(
                cx,
                MODULE_LEVEL_TRAIT_IMPORT,
                import.hir_id,
                import.span,
                format!(
                    "trait `{name}` is imported for the whole module but used only in `{function}`"
                ),
                |diag| {
                    diag.help(format!(
                        "move the `use` into `{function}`, as the Google Rust Style Guide's \
                         \"Prefer locally-scoped trait imports\" asks"
                    ));
                },
            );
        }
    }
}

/// The names that intra-doc links in the docs of `module` and of the items in it start with. A
/// link resolves through the module's imports, so moving an import a link names into a function
/// would break `just docs`.
fn doc_link_names(cx: &LateContext<'_>, module: LocalDefId) -> FxHashSet<Symbol> {
    let tcx = cx.tcx;
    let mut names = FxHashSet::default();
    let owners = tcx.hir_crate_items(()).owners().filter(|owner| {
        owner.def_id == module
            || tcx
                .parent_module_from_def_id(owner.def_id)
                .to_local_def_id()
                == module
    });
    for owner in owners {
        for attributes in tcx.hir_attr_map(owner).map.values() {
            for (doc, ..) in attributes
                .iter()
                .filter_map(AttributeExt::doc_str_and_fragment_kind)
            {
                link_names(doc.as_str(), &mut names);
            }
        }
    }
    names
}

/// Adds to `names` the first path segment of each link in the Markdown `doc`: the text of
/// `[Name]` or ``[`Name::method`]``, and the target of `[text](Name)` or `[text][Name]`.
fn link_names(doc: &str, names: &mut FxHashSet<Symbol>) {
    let mut rest = doc;
    while let Some(open) = rest.find('[') {
        rest = &rest[open + 1..];
        let Some(close) = rest.find(']') else {
            break;
        };
        add_link_name(&rest[..close], names);
        rest = &rest[close + 1..];
        if let Some(target) = rest.strip_prefix('(')
            && let Some(end) = target.find(')')
        {
            add_link_name(&target[..end], names);
        }
    }
}

/// Adds the first path segment of the link target `link`, such as `Write` for
/// ``trait@Write::write_str()``, to `names`.
fn add_link_name(link: &str, names: &mut FxHashSet<Symbol>) {
    let link = link.trim().trim_matches('`');
    let link = link.split_once('@').map_or(link, |(_, path)| path);
    let first = link.split("::").next().unwrap_or_default();
    let first = first.trim_end_matches("()").trim_end_matches('!');
    let is_identifier = first
        .chars()
        .next()
        .is_some_and(|character| character.is_alphabetic() || character == '_')
        && first
            .chars()
            .all(|character| character.is_alphanumeric() || character == '_');
    if is_identifier {
        names.insert(Symbol::intern(first));
    }
}

/// The macros the prelude of the crate's edition brings, each by name, which a `#[derive(..)]`
/// finds without an import.
fn prelude_macros(cx: &LateContext<'_>) -> FxHashSet<(Symbol, DefId)> {
    let tcx = cx.tcx;
    let edition = Symbol::intern(&format!("rust_{}", tcx.sess.edition()));
    let mut macros = FxHashSet::default();
    for krate in [sym::std, sym::core] {
        let Some(prelude) = external_module(tcx, [krate, sym::prelude, edition]) else {
            continue;
        };
        for child in tcx.module_children(prelude) {
            if let Res::Def(DefKind::Macro(_), id) = child.res {
                macros.insert((child.ident.name, id));
            }
        }
    }
    macros
}

/// Traits that other modules import through `module` (`use super::Trait`), which moving the
/// import would break. `None` when some module glob-imports `module`, which could rely on any of
/// its imports.
fn reimported_traits(cx: &LateContext<'_>, module: DefId) -> Option<FxHashSet<DefId>> {
    let mut traits = FxHashSet::default();
    for id in cx.tcx.hir_free_items() {
        let item = cx.tcx.hir_item(id);
        if is_test_code(cx.tcx, item.hir_id()) {
            continue;
        }
        for import in imports(cx.tcx, item) {
            match (import.name, import.res.type_ns) {
                (None, Some(Res::Def(DefKind::Mod, glob))) if glob == module => return None,
                (Some(_), Some(Res::Def(DefKind::Trait, trait_id)))
                    if import_parent(cx, item, &import.segments) == Some(module) =>
                {
                    traits.insert(trait_id);
                }
                _ => {}
            }
        }
    }
    Some(traits)
}

/// The module a single-item `use` imports from, following `crate`, `self` and `super`.
fn import_parent(
    cx: &LateContext<'_>,
    item: &Item<'_>,
    segments: &[&PathSegment<'_>],
) -> Option<DefId> {
    let importer = cx
        .tcx
        .parent_module_from_def_id(item.owner_id.def_id)
        .to_def_id();
    let (_, prefix) = segments.split_last()?;
    prefix
        .iter()
        .try_fold(importer, |current, segment| match segment.ident.name {
            kw::Crate => Some(CRATE_DEF_ID.to_def_id()),
            kw::SelfLower | kw::PathRoot => Some(current),
            kw::Super => current
                .as_local()
                .map(|local| cx.tcx.parent_module_from_def_id(local).to_def_id()),
            _ => segment.res.opt_def_id(),
        })
}

/// Where a use of a trait sits.
#[derive(Clone, Copy)]
enum Site {
    /// In an item signature, bound, field, `impl` header or other non-body position.
    Outside,
    /// In the body of this module-level function or method, or anything nested in it.
    Function(LocalDefId),
    /// In the body of a constant, static or other non-function item.
    OtherBody,
}

/// A derive an import brings along with its trait, by the name the import binds.
struct ImportedDerive {
    name: Symbol,
    derive: DefId,
    trait_id: DefId,
}

#[derive(Default)]
struct Uses {
    functions: FxHashSet<LocalDefId>,
    outside_functions: bool,
}

/// Records where the module uses each imported trait.
struct TraitUsage<'a, 'tcx> {
    cx: &'a LateContext<'tcx>,
    traits: FxHashMap<DefId, Uses>,
    derives: Vec<ImportedDerive>,
    site: Site,
    typeck_results: Option<&'tcx TypeckResults<'tcx>>,
    /// The names each block around the walk binds in the macro namespace by a `use`, innermost
    /// last. A derive written by such a name resolves through the block, not the module's import.
    block_macros: Vec<FxHashSet<Symbol>>,
}

impl TraitUsage<'_, '_> {
    /// Whether a block around the walk binds `name` in the macro namespace.
    fn block_binds_macro(&self, name: Symbol) -> bool {
        self.block_macros.iter().any(|names| names.contains(&name))
    }

    /// The names the imports of `block` bind in the macro namespace: by a `use` that imports a
    /// macro, and by a glob of a module with a macro of that name.
    fn macro_names(&self, block: &Block<'_>) -> FxHashSet<Symbol> {
        let tcx = self.cx.tcx;
        let mut names = FxHashSet::default();
        for statement in block.stmts {
            let StmtKind::Item(id) = statement.kind else {
                continue;
            };
            let item = tcx.hir_item(id);
            for import in imports(tcx, item) {
                match (import.name, import.res.type_ns) {
                    (Some(name), _) if import.res.macro_ns.is_some() => {
                        names.insert(name.name);
                    }
                    (None, Some(Res::Def(DefKind::Mod, module))) => {
                        let children = match module.as_local() {
                            Some(local) => tcx.module_children_local(local),
                            None => tcx.module_children(module),
                        };
                        names.extend(children.iter().filter_map(|child| {
                            matches!(child.res, Res::Def(DefKind::Macro(_), _))
                                .then_some(child.ident.name)
                        }));
                    }
                    _ => {}
                }
            }
        }
        names
    }

    /// Records a use of `def_id` if it is one of the traits or an item of one. Derives write
    /// absolute paths and don't need the import, so a derive's output isn't a use. Other macro
    /// output counts where it expands, since that is where the trait must be in scope.
    fn record(&mut self, def_id: DefId, span: Span) {
        let tcx = self.cx.tcx;
        let trait_id = if matches!(tcx.def_kind(def_id), DefKind::Trait) {
            def_id
        } else if let Some(trait_id) = tcx.trait_of_assoc(def_id) {
            trait_id
        } else {
            return;
        };
        if !span.in_derive_expansion() {
            self.record_trait(trait_id);
        }
    }

    /// Records a use of `trait_id` where the walk is, if it is one of the traits.
    fn record_trait(&mut self, trait_id: DefId) {
        let site = self.site;
        let Some(uses) = self.traits.get_mut(&trait_id) else {
            return;
        };
        match site {
            Site::Function(function) => {
                uses.functions.insert(function);
            }
            _ => uses.outside_functions = true,
        }
    }
}

impl<'tcx> Visitor<'tcx> for TraitUsage<'_, 'tcx> {
    type NestedFilter = nested_filter::All;

    fn maybe_tcx(&mut self) -> Self::MaybeTyCtxt {
        self.cx.tcx
    }

    /// Records the uses in `item`, unless it is a child module, which has its own scope, an
    /// import, which isn't a use, or test-only code, whose uses don't count. An item a derive
    /// wrote uses the derive's trait, since that is where the derive's name resolves, through the
    /// module's import unless a block around it binds the name itself.
    fn visit_item(&mut self, item: &'tcx Item<'tcx>) {
        if !matches!(item.kind, ItemKind::Mod(..) | ItemKind::Use(..))
            && !is_test_code(self.cx.tcx, item.hir_id())
        {
            for expansion in item.span.macro_backtrace() {
                let ExpnKind::Macro(MacroKind::Derive, written) = expansion.kind else {
                    continue;
                };
                if self.block_binds_macro(written) {
                    continue;
                }
                let used: Vec<DefId> = self
                    .derives
                    .iter()
                    .filter(|derive| {
                        derive.name == written && expansion.macro_def_id == Some(derive.derive)
                    })
                    .map(|derive| derive.trait_id)
                    .collect();
                for trait_id in used {
                    self.record_trait(trait_id);
                }
            }
            walk_item(self, item);
        }
    }

    fn visit_impl_item(&mut self, item: &'tcx ImplItem<'tcx>) {
        if !is_test_code(self.cx.tcx, item.hir_id()) {
            walk_impl_item(self, item);
        }
    }

    fn visit_nested_body(&mut self, body_id: BodyId) {
        let tcx = self.cx.tcx;
        let previous = (self.site, self.typeck_results);
        if matches!(self.site, Site::Outside) {
            let owner = tcx.hir_body_owner_def_id(body_id);
            self.site = if matches!(tcx.def_kind(owner), DefKind::Fn | DefKind::AssocFn) {
                Site::Function(owner)
            } else {
                Site::OtherBody
            };
        }
        self.typeck_results = Some(tcx.typeck_body(body_id));
        walk_body(self, tcx.hir_body(body_id));
        (self.site, self.typeck_results) = previous;
    }

    fn visit_block(&mut self, block: &'tcx Block<'tcx>) {
        let names = self.macro_names(block);
        self.block_macros.push(names);
        walk_block(self, block);
        self.block_macros.pop();
    }

    fn visit_expr(&mut self, expr: &'tcx Expr<'tcx>) {
        if matches!(
            expr.kind,
            ExprKind::MethodCall(..) | ExprKind::Path(QPath::TypeRelative(..))
        ) && let Some(def_id) = self
            .typeck_results
            .and_then(|results| results.type_dependent_def_id(expr.hir_id))
        {
            self.record(def_id, expr.span);
        }
        walk_expr(self, expr);
    }

    fn visit_path(&mut self, path: &Path<'tcx>, _: HirId) {
        let resolutions = path
            .segments
            .iter()
            .map(|segment| segment.res)
            .chain([path.res]);
        for res in resolutions {
            if let Some(def_id) = res.opt_def_id() {
                self.record(def_id, path.span);
            }
        }
        walk_path(self, path);
    }
}

#[test]
fn ui() {
    crate::ui_harness::ui_test(MODULE_LEVEL_TRAIT_IMPORT);
}
