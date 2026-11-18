use clippy_utils::diagnostics::span_lint_hir_and_then;
use rustc_data_structures::fx::FxHashSet;
use rustc_hir::PolyTraitRef;
use rustc_hir::def::DefKind;
use rustc_hir::def_id::{DefId, LocalDefId};
use rustc_hir::intravisit::{Visitor, walk_poly_trait_ref};
use rustc_lint::{LateContext, LateLintPass};
use rustc_lint_defs::{declare_lint, declare_lint_pass};
use rustc_middle::hir::nested_filter;
use rustc_middle::ty::{self, TyCtxt};

declare_lint! {
    /// Checks for a trait with exactly one impl, for a type of this crate, that nothing uses as a
    /// bound: no generic bound or `where` clause, `impl Trait`, `dyn Trait` or supertrait names
    /// it.
    ///
    /// The Google Rust Style Guide, "When to define a trait versus an inherent implementation":
    /// define a trait for generic programming, for `impl Trait` or `dyn Trait`, or as an extension
    /// trait. "Don't use traits merely to organize code." A trait like this only groups methods,
    /// which an inherent impl does without the extra import.
    ///
    /// Traits other crates can see are skipped, since those crates may implement or bound on
    /// them.
    ///
    /// Code a macro writes isn't checked. Code a proc macro copies from its caller's tokens, such
    /// as the body of a function under an attribute macro, is: its author wrote it there.
    ///
    /// ```rust,ignore
    /// trait Scoring {
    ///     fn score(&self) -> u64;
    /// }
    ///
    /// impl Scoring for Order {
    ///     fn score(&self) -> u64 { self.price * 2 }
    /// }
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// impl Order {
    ///     fn score(&self) -> u64 { self.price * 2 }
    /// }
    /// ```
    pub SINGLE_IMPL_TRAIT,
    Warn,
    "a trait with one impl that is never used generically"
}

declare_lint_pass!(SingleImplTrait => [SINGLE_IMPL_TRAIT]);

impl<'tcx> LateLintPass<'tcx> for SingleImplTrait {
    fn check_crate_post(&mut self, cx: &LateContext<'tcx>) {
        let tcx = cx.tcx;
        let traits: Vec<LocalDefId> = tcx
            .hir_crate_items(())
            .definitions()
            .filter(|&id| tcx.def_kind(id) == DefKind::Trait)
            .filter(|&id| {
                !tcx.def_span(id).from_expansion() && !cx.effective_visibilities.is_exported(id)
            })
            .collect();
        if traits.is_empty() {
            return;
        }
        let mut bounds = BoundTraits {
            tcx,
            used: FxHashSet::default(),
        };
        tcx.hir_walk_toplevel_module(&mut bounds);
        for trait_id in traits {
            if bounds.used.contains(&trait_id.to_def_id()) {
                continue;
            }
            let Some(implementation) = only_impl(tcx, trait_id.to_def_id()) else {
                continue;
            };
            let ty::Adt(adt, _) = tcx
                .type_of(implementation)
                .instantiate_identity()
                .skip_normalization()
                .kind()
            else {
                continue;
            };
            if !implementation.is_local()
                || !adt.did().is_local()
                || tcx.def_span(implementation).from_expansion()
            {
                continue;
            }
            let trait_name = tcx.item_name(trait_id.to_def_id());
            let type_name = tcx.item_name(adt.did());
            span_lint_hir_and_then(
                cx,
                SINGLE_IMPL_TRAIT,
                tcx.local_def_id_to_hir_id(trait_id),
                tcx.def_ident_span(trait_id)
                    .unwrap_or_else(|| tcx.def_span(trait_id)),
                format!("trait `{trait_name}` has one impl and is never used as a bound"),
                |diag| {
                    diag.span_note(tcx.def_span(implementation), "the only impl");
                    diag.help(format!(
                        "move its items into an inherent `impl {type_name}`, as the Google Rust Style Guide's \
                         \"When to define a trait versus an inherent implementation\" asks"
                    ));
                },
            );
        }
    }
}

/// The trait's one impl, unless it has a blanket impl or a number of impls other than one.
fn only_impl(tcx: TyCtxt<'_>, trait_id: DefId) -> Option<DefId> {
    let impls = tcx.trait_impls_of(trait_id);
    if !impls.blanket_impls().is_empty() {
        return None;
    }
    let mut all = impls.non_blanket_impls().values().flatten();
    match (all.next(), all.next()) {
        (Some(&only), None) => Some(only),
        _ => None,
    }
}

/// Collects every trait named in a bound: generic bounds, `where` clauses, `impl Trait`,
/// `dyn Trait` and supertraits all reach `visit_poly_trait_ref`, while an impl's trait does not.
struct BoundTraits<'tcx> {
    tcx: TyCtxt<'tcx>,
    used: FxHashSet<DefId>,
}

impl<'tcx> Visitor<'tcx> for BoundTraits<'tcx> {
    type NestedFilter = nested_filter::All;

    fn maybe_tcx(&mut self) -> Self::MaybeTyCtxt {
        self.tcx
    }

    fn visit_poly_trait_ref(&mut self, trait_ref: &'tcx PolyTraitRef<'tcx>) {
        if let Some(id) = trait_ref.trait_ref.trait_def_id() {
            self.used.insert(id);
        }
        walk_poly_trait_ref(self, trait_ref);
    }
}

#[test]
fn ui() {
    crate::ui_harness::ui_test(SINGLE_IMPL_TRAIT);
}
