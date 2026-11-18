use clippy_utils::diagnostics::span_lint_hir_and_then;
use rustc_attr_ir::find_attr;
use rustc_data_structures::fx::{FxHashMap, FxHashSet};
use rustc_hir::def::{DefKind, Res};
use rustc_hir::def_id::DefId;
use rustc_hir::intravisit::{Visitor, walk_item, walk_path};
use rustc_hir::{HirId, Item, ItemKind, Mod, Path, PathSegment, UseKind, UseTree};
use rustc_lint::{LateContext, LateLintPass, LintContext};
use rustc_lint_defs::{declare_lint, declare_lint_pass};
use rustc_middle::hir::nested_filter;
use rustc_middle::ty::TyCtxt;
use rustc_span::def_id::LocalModId;
use rustc_span::{Span, Symbol, kw};

use crate::imports::imports;

use crate::derive_output::{derive_wrote, source_runs};

declare_lint! {
    /// Checks for types and traits written with a module qualifier, such as `regex::Regex`,
    /// where importing the name would be unambiguous.
    ///
    /// The Google Rust Style Guide, "Type names should be imported and used unqualified": code
    /// should use `Regex`, not `regex::Regex`, with a `use regex::Regex`.
    ///
    /// The guide accepts qualifiers that resolve an ambiguity, so a name isn't checked when it is
    /// in the `ambiguous_type_names` list, when the module defines or imports another type of
    /// that name, when the module qualifies another type of that name too (`a::Config` beside
    /// `b::Config`), or when it would shadow a different prelude type. Paths through types
    /// (`Self::Item`, `Enum::Variant`, `<T as Trait>::Output`) and macro output aren't checked,
    /// including the paths a derive writes at the span of the caller's tokens, as
    /// `#[derive(thiserror::Error)]` writes `::core::convert::From` at a `#[from]`. Paths the
    /// caller wrote are checked wherever a macro puts them, as a proc macro copies them or serde
    /// parses them from `#[serde(with = "...")]` and adds `deserialize`.
    /// A prelude type written through its module, such as `std::string::String`, needs no import,
    /// only its bare name. Each path is reported at the node that holds it, so an `expect` on the
    /// item or statement around it covers it.
    ///
    /// ### Configuration
    ///
    /// `ambiguous_type_names` in the `trellis_lints` table of `dylint.toml` lists names that may
    /// always be qualified, such as `io::Error`. It replaces the default, `["Error", "Result"]`.
    ///
    /// ```toml
    /// [trellis_lints]
    /// ambiguous_type_names = ["Error", "Result", "Context"]
    /// ```
    ///
    /// ```rust,ignore
    /// impl fmt::Display for Order { /* ... */ }
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// use std::fmt::Display;
    ///
    /// impl Display for Order { /* ... */ }
    /// ```
    pub QUALIFIED_TYPE_PATH,
    Warn,
    "a type named through its module where importing it would be unambiguous"
}

declare_lint_pass!(QualifiedTypePath => [QUALIFIED_TYPE_PATH]);

impl<'tcx> LateLintPass<'tcx> for QualifiedTypePath {
    /// Reports each type the module names through its module where importing it would be
    /// unambiguous. The report sits at the node that holds the path, so an `expect` on the item
    /// or expression around it applies; `check_mod` sits at the module.
    fn check_mod(&mut self, cx: &LateContext<'tcx>, module: &'tcx Mod<'tcx>, _: LocalModId) {
        let mut names = ModuleNames::new(cx);
        for &id in module.item_ids {
            names.visit_item(cx.tcx.hir_item(id));
        }
        if names.qualified.is_empty() {
            return;
        }
        let config = crate::config::get(cx.sess());
        for qualified in &names.qualified {
            let name = qualified.name;
            if config
                .ambiguous_type_names
                .iter()
                .any(|ambiguous| ambiguous == name.as_str())
                || names.is_ambiguous(name, qualified.def_id)
            {
                continue;
            }
            let fix = if names.prelude.get(&name) == Some(&qualified.def_id) {
                format!("write `{name}`, which the prelude already brings into scope")
            } else {
                format!(
                    "add `use {};` and write `{name}`",
                    import_path(cx, qualified.def_id)
                )
            };
            span_lint_hir_and_then(
                cx,
                QUALIFIED_TYPE_PATH,
                qualified.hir_id,
                qualified.span,
                format!("`{name}` is named through its module"),
                |diag| {
                    diag.help(format!(
                        "{fix}, as the Google Rust Style Guide's \"Type names should be imported \
                         and used unqualified\" asks"
                    ));
                },
            );
        }
    }
}

/// A type or trait written with a module qualifier.
struct Qualified {
    /// The node that holds the path, whose lint levels apply.
    hir_id: HirId,
    span: Span,
    def_id: DefId,
    name: Symbol,
}

/// The type names one module defines, imports and qualifies. Child modules are separate.
struct ModuleNames<'a, 'tcx> {
    cx: &'a LateContext<'tcx>,
    /// Types and traits the module defines or imports by name, including in function bodies.
    in_scope: FxHashMap<Symbol, FxHashSet<DefId>>,
    /// Modules the module glob-imports.
    globs: Vec<DefId>,
    /// The types and traits the prelude brings into every module, by name.
    prelude: FxHashMap<Symbol, DefId>,
    qualified: Vec<Qualified>,
}

impl<'a, 'tcx> ModuleNames<'a, 'tcx> {
    fn new(cx: &'a LateContext<'tcx>) -> Self {
        Self {
            cx,
            in_scope: FxHashMap::default(),
            globs: Vec::new(),
            prelude: prelude_types(cx.tcx),
            qualified: Vec::new(),
        }
    }

    /// Whether importing `name` as `def_id` could be confused with, or clash with, another type.
    /// Importing a different type of a prelude name would shadow the prelude's.
    fn is_ambiguous(&self, name: Symbol, def_id: DefId) -> bool {
        let other = |&other: &DefId| other != def_id;
        if self
            .in_scope
            .get(&name)
            .is_some_and(|defs| defs.iter().any(other))
        {
            return true;
        }
        if self
            .qualified
            .iter()
            .any(|qualified| qualified.name == name && qualified.def_id != def_id)
        {
            return true;
        }
        if self.prelude.get(&name).is_some_and(other) {
            return true;
        }
        let tcx = self.cx.tcx;
        self.globs.iter().any(|&glob| {
            let children = match glob.as_local() {
                Some(local) => tcx.module_children_local(local),
                None => tcx.module_children(glob),
            };
            children.iter().any(|child| {
                child.ident.name == name
                    && child
                        .res
                        .opt_def_id()
                        .is_some_and(|child| child != def_id && is_type(tcx.def_kind(child)))
            })
        })
    }
}

fn is_type(kind: DefKind) -> bool {
    matches!(
        kind,
        DefKind::Struct
            | DefKind::Enum
            | DefKind::Union
            | DefKind::Trait
            | DefKind::TraitAlias
            | DefKind::TyAlias
            | DefKind::ForeignTy
    )
}

impl<'tcx> Visitor<'tcx> for ModuleNames<'_, 'tcx> {
    type NestedFilter = nested_filter::All;

    fn maybe_tcx(&mut self) -> Self::MaybeTyCtxt {
        self.cx.tcx
    }

    /// Records the names `item` brings into scope and the qualified paths in it. Imports are
    /// names in scope, not uses of a qualified path. Child modules, including ones inside
    /// function bodies, are checked on their own, so the walk skips them.
    fn visit_item(&mut self, item: &'tcx Item<'tcx>) {
        match item.kind {
            ItemKind::Use(..) => {
                for import in imports(self.cx.tcx, item) {
                    match (import.name, import.res.type_ns) {
                        (Some(ident), Some(Res::Def(kind, def_id))) if is_type(kind) => {
                            self.in_scope.entry(ident.name).or_default().insert(def_id);
                        }
                        (None, Some(Res::Def(DefKind::Mod, module))) => self.globs.push(module),
                        _ => {}
                    }
                }
            }
            ItemKind::Mod(..) => {}
            _ => {
                if is_type(self.cx.tcx.def_kind(item.owner_id))
                    && let Some(ident) = item.kind.ident()
                {
                    self.in_scope
                        .entry(ident.name)
                        .or_default()
                        .insert(item.owner_id.to_def_id());
                }
                walk_item(self, item);
            }
        }
    }

    fn visit_path(&mut self, path: &Path<'tcx>, hir_id: HirId) {
        if let Some(written) = written_whole(path)
            && let Some(qualified) = qualified_type(&written, hir_id)
            && !derive_wrote(self.cx, hir_id, &written)
        {
            self.qualified.push(qualified);
        }
        walk_path(self, path);
    }
}

/// The types and traits the crate's prelude brings into every module, found through the
/// `#[prelude_import]` glob the compiler injects into the crate root, so the edition and
/// `no_std` pick the right prelude.
fn prelude_types(tcx: TyCtxt<'_>) -> FxHashMap<Symbol, DefId> {
    let prelude = tcx.hir_root_module().item_ids.iter().find_map(|&id| {
        let item = tcx.hir_item(id);
        match item.kind {
            ItemKind::Use(UseTree {
                prefix,
                kind: UseKind::Glob,
            }) if find_attr!(tcx, item.hir_id(), PreludeImport) => {
                extern_module(tcx, prefix.segments)
            }
            _ => None,
        }
    });
    prelude
        .map_or(&[][..], |prelude| tcx.module_children(prelude))
        .iter()
        .filter_map(|child| match child.res {
            Res::Def(kind, def_id) if is_type(kind) => Some((child.ident.name, def_id)),
            _ => None,
        })
        .collect()
}

/// The module `std::prelude::rust_2024` and the like name in another crate, looked up by name:
/// the resolver records no resolution on the injected prelude import's path.
fn extern_module(tcx: TyCtxt<'_>, segments: &[PathSegment<'_>]) -> Option<DefId> {
    let (krate, path) = segments.split_first()?;
    let krate = tcx
        .crates(())
        .iter()
        .find(|&&krate_num| tcx.crate_name(krate_num) == krate.ident.name)?;
    path.iter().try_fold(krate.as_def_id(), |module, segment| {
        tcx.module_children(module)
            .iter()
            .find_map(|child| match child.res {
                Res::Def(DefKind::Mod, child_module) if child.ident.name == segment.ident.name => {
                    Some(child_module)
                }
                _ => None,
            })
    })
}

/// The path to import `def_id` by, such as `std::fmt::Display` or `crate::shelf::Shelf`.
fn import_path(cx: &LateContext<'_>, def_id: DefId) -> String {
    let path = cx.tcx.def_path_str(def_id);
    if def_id.is_local() {
        format!("crate::{path}")
    } else {
        path
    }
}

/// `path` spanning just its segments if no macro wrote any of them, since a macro that writes the
/// module chose the qualifier. Its own span can still come from a macro, as serde gives the `a::B`
/// of `#[serde(with = "a::B")]` its derive's span.
fn written_whole<'hir>(path: &Path<'hir>) -> Option<Path<'hir>> {
    source_runs(path)
        .next()
        .filter(|run| run.segments.len() == path.segments.len())
}

/// `path` if it names a type or trait through one or more modules, as in `fmt::Display`.
fn qualified_type(path: &Path<'_>, hir_id: HirId) -> Option<Qualified> {
    let Res::Def(kind, def_id) = path.res else {
        return None;
    };
    let [first, .., last] = path.segments else {
        return None;
    };
    if !is_type(kind) {
        return None;
    }
    let is_module = |segment: &PathSegment<'_>| {
        matches!(segment.res, Res::Def(DefKind::Mod, _))
            || [kw::PathRoot, kw::Crate, kw::Super, kw::SelfLower].contains(&segment.ident.name)
    };
    if !path.segments[..path.segments.len() - 1]
        .iter()
        .all(is_module)
    {
        return None;
    }
    Some(Qualified {
        hir_id,
        span: first.ident.span.to(last.ident.span),
        def_id,
        name: last.ident.name,
    })
}

#[test]
fn ui() {
    crate::ui_harness::ui_test(QUALIFIED_TYPE_PATH);
}

/// The fixture under `auxiliary`, which the default test doesn't search, runs with
/// `ambiguous_type_names` configured.
#[test]
fn ui_configured() {
    crate::ui_harness::ui_test_of(QUALIFIED_TYPE_PATH, "ui/qualified_type_path/auxiliary")
        .dylint_toml(crate::ui_harness::dylint_toml_with(
            "ambiguous_type_names",
            Some(r#"["Config"]"#),
        ))
        .run();
}

/// A path that a proc macro writes from the caller's tokens isn't checked.
#[test]
fn ui_proc_macros() {
    crate::ui_harness::ui_test_of(QUALIFIED_TYPE_PATH, "ui/qualified_type_path_proc_macros").run();
}
