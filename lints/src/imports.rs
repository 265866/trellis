//! The names and globs a `use` item imports, each with its whole path. The compiler keeps a `use`
//! with braces as one item whose nested trees hold only their own segments, so
//! `use std::{fs, io::Write}` is one item; this flattens it into `std::fs` and `std::io::Write`.

use rustc_hir::def::{DefKind, PerNS, Res};
use rustc_hir::def_id::DefId;
use rustc_hir::{HirId, Item, ItemKind, PathSegment, UseKind, UseTree};
use rustc_middle::ty::TyCtxt;
use rustc_span::def_id::LocalDefId;
use rustc_span::{Ident, Span, Symbol, kw};

/// One name or glob a `use` item imports.
pub(crate) struct Import<'hir> {
    /// The segments from the `use` item's root to the imported name or glob's module, a trailing
    /// `self` dropped.
    pub(crate) segments: Vec<&'hir PathSegment<'hir>>,
    /// What the path resolves to in each namespace.
    pub(crate) res: PerNS<Option<Res>>,
    /// The span of the import: the whole item for a `use` without braces, else the tree in them.
    pub(crate) span: Span,
    /// The span of the path the import's own tree writes: in braces, only the part after them.
    pub(crate) path_span: Span,
    /// The name the import binds, or `None` for a glob.
    pub(crate) name: Option<Ident>,
    /// The import's node, where lint levels apply.
    pub(crate) hir_id: HirId,
    /// The import's definition, which has its visibility.
    pub(crate) def_id: LocalDefId,
}

/// The imports of `item`, empty if it isn't a `use`.
pub(crate) fn imports<'hir>(tcx: TyCtxt<'hir>, item: &'hir Item<'hir>) -> Vec<Import<'hir>> {
    let ItemKind::Use(tree) = &item.kind else {
        return Vec::new();
    };
    let mut imports = Vec::new();
    let root = Location {
        hir_id: item.hir_id(),
        def_id: item.owner_id.def_id,
        span: item.span,
    };
    flatten(tcx, tree, &[], root, &mut imports);
    imports
}

/// Where a tree of a `use` item sits.
#[derive(Clone, Copy)]
struct Location {
    hir_id: HirId,
    def_id: LocalDefId,
    span: Span,
}

/// Adds the imports of `tree`, whose enclosing trees' segments are `outer`, to `imports`.
fn flatten<'hir>(
    tcx: TyCtxt<'hir>,
    tree: &'hir UseTree<'hir>,
    outer: &[&'hir PathSegment<'hir>],
    location: Location,
    imports: &mut Vec<Import<'hir>>,
) {
    let mut segments = outer.to_vec();
    segments.extend(tree.prefix.segments.iter());
    let name = match tree.kind {
        UseKind::Single(mut ident) => {
            if segments.len() > 1
                && segments
                    .last()
                    .is_some_and(|segment| segment.ident.name == kw::SelfLower)
            {
                segments.pop();
                if ident.name == kw::SelfLower
                    && let Some(module) = segments.last()
                {
                    ident = module.ident;
                }
            }
            Some(ident)
        }
        UseKind::Glob => None,
        UseKind::Nested { items } => {
            for (nested, hir_id, def_id) in items {
                let location = Location {
                    hir_id: *hir_id,
                    def_id: *def_id,
                    span: tcx.def_span(*def_id),
                };
                flatten(tcx, nested, &segments, location, imports);
            }
            return;
        }
    };
    imports.push(Import {
        segments,
        res: tree.prefix.res,
        span: location.span,
        path_span: tree.prefix.span,
        name,
        hir_id: location.hir_id,
        def_id: location.def_id,
    });
}

/// The module of another crate that the path of `names` names, such as the
/// `std::prelude::rust_2024` the compiler imports the prelude from, found by name through each
/// module's children.
pub(crate) fn external_module(
    tcx: TyCtxt<'_>,
    names: impl IntoIterator<Item = Symbol>,
) -> Option<DefId> {
    let mut names = names.into_iter().filter(|name| *name != kw::PathRoot);
    let first = names.next()?;
    let krate = tcx
        .crates(())
        .iter()
        .find(|&&krate| tcx.crate_name(krate) == first)?;
    names.try_fold(krate.as_def_id(), |module, name| {
        tcx.module_children(module)
            .iter()
            .find_map(|child| match child.res {
                Res::Def(DefKind::Mod, child_module) if child.ident.name == name => {
                    Some(child_module)
                }
                _ => None,
            })
    })
}
