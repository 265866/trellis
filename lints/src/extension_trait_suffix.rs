use core::ops::ControlFlow;

use clippy_utils::diagnostics::span_lint_and_help;
use rustc_hir::def_id::DefId;
use rustc_hir::intravisit::{self, Visitor};
use rustc_hir::{Item, ItemKind, PolyTraitRef, TraitItemKind};
use rustc_lint::{LateContext, LateLintPass};
use rustc_lint_defs::{declare_lint, declare_lint_pass};
use rustc_middle::hir::nested_filter;
use rustc_middle::ty::{self, GenericArg, Ty, TyCtxt};

declare_lint! {
    /// Checks for extension traits whose names don't end in `Ext`. An extension trait here is a
    /// trait with methods whose every impl in the crate is for a type from another crate, or is a
    /// blanket impl, and that the crate never names as a bound, `impl Trait` or `dyn Trait`.
    ///
    /// The Google Rust Style Guide, "Extension traits" and "Naming extension traits": a trait
    /// that only adds methods to a type from a different crate carries the suffix `Ext`, so
    /// readers can tell it from a trait meant for generics or dynamic dispatch.
    ///
    /// Code a macro writes isn't checked. Code a proc macro copies from its caller's tokens, such
    /// as the body of a function under an attribute macro, is: its author wrote it there.
    ///
    /// ```rust,ignore
    /// trait Prices {
    ///     fn total_price(&self) -> u64;
    /// }
    ///
    /// impl Prices for [u64] {
    ///     fn total_price(&self) -> u64 { self.iter().sum() }
    /// }
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// trait PricesExt {
    ///     fn total_price(&self) -> u64;
    /// }
    ///
    /// impl PricesExt for [u64] {
    ///     fn total_price(&self) -> u64 { self.iter().sum() }
    /// }
    /// ```
    pub EXTENSION_TRAIT_SUFFIX,
    Warn,
    "an extension trait whose name doesn't end in `Ext`"
}

declare_lint_pass!(ExtensionTraitSuffix => [EXTENSION_TRAIT_SUFFIX]);

impl<'tcx> LateLintPass<'tcx> for ExtensionTraitSuffix {
    /// Reports a trait not named `..Ext` that has a method and is implemented only for foreign
    /// types, unless the crate uses it in a bound or a trait object. Extension traits add methods;
    /// a marker trait such as a sealed supertrait adds none. A trait the crate uses in bounds or
    /// trait objects is an input trait, such as the guide's `DateLike`, even when only foreign
    /// types implement it.
    fn check_item(&mut self, cx: &LateContext<'tcx>, item: &'tcx Item<'tcx>) {
        if item.span.from_expansion() {
            return;
        }
        let ItemKind::Trait { ident, items, .. } = item.kind else {
            return;
        };
        if ident.name.as_str().ends_with("Ext") {
            return;
        }
        let has_method = items
            .iter()
            .any(|id| matches!(cx.tcx.hir_trait_item(*id).kind, TraitItemKind::Fn(..)));
        if !has_method {
            return;
        }
        let trait_id = item.owner_id.to_def_id();
        let mut impls = cx.tcx.all_impls(trait_id).peekable();
        if impls.peek().is_none() || !impls.all(|impl_id| extends_foreign_type(cx.tcx, impl_id)) {
            return;
        }
        let mut finder = BoundFinder {
            tcx: cx.tcx,
            trait_id,
        };
        if cx
            .tcx
            .hir_visit_all_item_likes_in_crate(&mut finder)
            .is_break()
        {
            return;
        }
        span_lint_and_help(
            cx,
            EXTENSION_TRAIT_SUFFIX,
            ident.span,
            format!("extension trait `{ident}` doesn't end in `Ext`"),
            None,
            format!(
                "name it `{ident}Ext`, as the Google Rust Style Guide's \"Naming extension \
                 traits\" asks"
            ),
        );
    }
}

/// Whether the impl adds methods to another crate's type or to every type that meets bounds,
/// rather than to a type or trait this crate defines.
fn extends_foreign_type(tcx: TyCtxt<'_>, impl_id: DefId) -> bool {
    let self_ty = tcx.type_of(impl_id).instantiate_identity().skip_norm_wip();
    let bounded_by_local_trait =
        tcx.explicit_clauses_of(impl_id)
            .clauses
            .iter()
            .any(|(clause, _)| {
                clause
                    .as_trait_clause()
                    .is_some_and(|bound| bound.def_id().is_local())
            });
    !bounded_by_local_trait
        && self_ty
            .walk()
            .filter_map(GenericArg::as_type)
            .all(is_foreign_part)
}

/// Whether a type nested in an impl's self type is defined outside this crate, or is a primitive
/// or type parameter.
fn is_foreign_part(ty: Ty<'_>) -> bool {
    match ty.kind() {
        ty::Adt(adt, _) => !adt.did().is_local(),
        ty::Foreign(id) => !id.is_local(),
        ty::Dynamic(predicates, _) => predicates
            .principal_def_id()
            .is_some_and(|id| !id.is_local()),
        ty::FnDef(..)
        | ty::Closure(..)
        | ty::CoroutineClosure(..)
        | ty::Coroutine(..)
        | ty::CoroutineWitness(..) => false,
        _ => true,
    }
}

/// Finds a use of the trait in a bound, `impl Trait` or `dyn Trait`; `impl Trait for` isn't one.
struct BoundFinder<'tcx> {
    tcx: TyCtxt<'tcx>,
    trait_id: DefId,
}

impl<'tcx> Visitor<'tcx> for BoundFinder<'tcx> {
    type NestedFilter = nested_filter::OnlyBodies;
    type Result = ControlFlow<()>;

    fn maybe_tcx(&mut self) -> Self::MaybeTyCtxt {
        self.tcx
    }

    fn visit_poly_trait_ref(&mut self, trait_ref: &'tcx PolyTraitRef<'tcx>) -> Self::Result {
        if trait_ref.trait_ref.trait_def_id() == Some(self.trait_id) {
            return ControlFlow::Break(());
        }
        intravisit::walk_poly_trait_ref(self, trait_ref)
    }
}

#[test]
fn ui() {
    crate::ui_harness::ui_test(EXTENSION_TRAIT_SUFFIX);
}
