use clippy_utils::diagnostics::span_lint_and_help;
use clippy_utils::res::MaybeDef as _;
use clippy_utils::sym;
use rustc_data_structures::fx::FxHashSet;
use rustc_hir::def_id::LocalDefId;
use rustc_hir::{FnDecl, ImplItem, ImplItemKind, Item, ItemKind, TraitItem, TraitItemKind};
use rustc_lint::{LateContext, LateLintPass};
use rustc_lint_defs::{declare_lint, declare_lint_pass};
use rustc_middle::ty::print::with_forced_trimmed_paths;
use rustc_middle::ty::{self, Clause, ClauseKind, GenericArg, Ty};

declare_lint! {
    /// Checks for function and method parameters whose type holds `impl Into<T>` or a generic
    /// parameter that the function bounds by `Into<T>`, directly or nested, as in `Vec<T>` or
    /// `impl IntoIterator<Item = impl Into<String>>`. Trait impls aren't checked, since the
    /// trait fixes their signatures.
    ///
    /// The Google Rust Style Guide, "Accepting `Into`": application code should almost never use
    /// `impl Into<T>` parameters, which trade a `.into()` at the call site for a more complex
    /// signature, more generated code, and harder type inference. trellis applies the rule to
    /// library APIs too, where the guide lets you consider `impl Into<T>` when call-site
    /// ergonomics are paramount; such a parameter takes an `expect` saying so.
    ///
    /// Code a macro writes isn't checked. Code a proc macro copies from its caller's tokens, such
    /// as the body of a function under an attribute macro, is: its author wrote it there.
    ///
    /// ```rust,ignore
    /// fn set_title(&mut self, title: impl Into<String>) {
    ///     self.title = title.into();
    /// }
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// fn set_title(&mut self, title: String) {
    ///     self.title = title;
    /// }
    /// ```
    pub IMPL_INTO_PARAMS,
    Warn,
    "a function parameter that takes `impl Into<T>`"
}

declare_lint_pass!(ImplIntoParams => [IMPL_INTO_PARAMS]);

impl<'tcx> LateLintPass<'tcx> for ImplIntoParams {
    fn check_item(&mut self, cx: &LateContext<'tcx>, item: &'tcx Item<'tcx>) {
        if let ItemKind::Fn { sig, .. } = item.kind {
            check_params(cx, item.owner_id.def_id, sig.decl);
        }
    }

    fn check_impl_item(&mut self, cx: &LateContext<'tcx>, item: &'tcx ImplItem<'tcx>) {
        let ImplItemKind::Fn(sig, _) = item.kind else {
            return;
        };
        if crate::trait_impls::is_in_trait_impl(item) {
            return;
        }
        check_params(cx, item.owner_id.def_id, sig.decl);
    }

    fn check_trait_item(&mut self, cx: &LateContext<'tcx>, item: &'tcx TraitItem<'tcx>) {
        if let TraitItemKind::Fn(sig, _) = item.kind {
            check_params(cx, item.owner_id.def_id, sig.decl);
        }
    }
}

/// Reports each parameter of `function`, declared with `decl`, whose type reaches a type parameter
/// with an `Into` bound. `impl Into<T>` in argument position is a generic parameter too, so both
/// spellings show up as an `Into` bound on a type parameter among the function's own where
/// clauses.
fn check_params(cx: &LateContext<'_>, function: LocalDefId, decl: &FnDecl<'_>) {
    let signature = cx
        .tcx
        .fn_sig(function)
        .instantiate_identity()
        .skip_normalization()
        .skip_binder();
    let clauses: Vec<Clause<'_>> = cx
        .tcx
        .explicit_clauses_of(function)
        .clauses
        .iter()
        .map(|&(clause, _)| clause)
        .collect();
    let into_bounds: Vec<_> = clauses
        .iter()
        .filter_map(|clause| clause.as_trait_clause())
        .filter(|bound| bound.def_id().is_diag_item(cx, sym::Into))
        .map(|bound| bound.skip_binder().trait_ref)
        .collect();
    if into_bounds.is_empty() {
        return;
    }
    for (hir_ty, &ty) in decl.inputs.iter().zip(signature.inputs()) {
        if hir_ty.span.from_expansion() {
            continue;
        }
        let params = reachable_params(ty, &clauses);
        let Some(bound) = into_bounds
            .iter()
            .find(|bound| params.contains(&bound.self_ty()))
        else {
            continue;
        };
        let target = with_forced_trimmed_paths!(bound.args.type_at(1).to_string());
        span_lint_and_help(
            cx,
            IMPL_INTO_PARAMS,
            hir_ty.span,
            format!("parameter accepts any `Into<{target}>`"),
            None,
            format!(
                "take `{target}` and call `.into()` at the call site, as the Google Rust Style \
                 Guide's \"Accepting `Into`\" asks of application code; trellis asks it of \
                 library APIs too, where a parameter whose call-site ergonomics are paramount \
                 takes an `expect` saying so"
            ),
        );
    }
}

/// The type parameters in `ty`, and in the bounds of those, so `Vec<T>` reaches `T` and
/// `impl IntoIterator<Item = impl Into<String>>` reaches the inner `impl Into<String>`.
fn reachable_params<'tcx>(ty: Ty<'tcx>, clauses: &[Clause<'tcx>]) -> FxHashSet<Ty<'tcx>> {
    let mut params = FxHashSet::default();
    add_params(&mut params, ty.walk());
    loop {
        let before = params.len();
        for clause in clauses {
            match clause.kind().skip_binder() {
                ClauseKind::Trait(bound) if params.contains(&bound.self_ty()) => {
                    add_params(
                        &mut params,
                        bound.trait_ref.args.iter().flat_map(GenericArg::walk),
                    );
                }
                ClauseKind::Projection(projection)
                    if params.contains(&projection.projection_term.self_ty()) =>
                {
                    add_params(&mut params, projection.term.walk());
                }
                _ => {}
            }
        }
        if params.len() == before {
            return params;
        }
    }
}

fn add_params<'tcx>(
    params: &mut FxHashSet<Ty<'tcx>>,
    args: impl IntoIterator<Item = GenericArg<'tcx>>,
) {
    params.extend(
        args.into_iter()
            .filter_map(GenericArg::as_type)
            .filter(|ty| matches!(ty.kind(), ty::Param(_))),
    );
}

#[test]
fn ui() {
    crate::ui_harness::ui_test(IMPL_INTO_PARAMS);
}
