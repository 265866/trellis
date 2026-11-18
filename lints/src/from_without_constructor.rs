use clippy_utils::diagnostics::span_lint_and_help;
use rustc_hir::{Item, ItemKind};
use rustc_lint::{LateContext, LateLintPass};
use rustc_lint_defs::{declare_lint, declare_lint_pass};
use rustc_middle::ty::fast_reject::DeepRejectCtxt;
use rustc_middle::ty::{self, AdtDef, Ty, TyCtxt};
use rustc_span::sym;

declare_lint! {
    /// Checks for `impl From<A> for T`, where `T` is a type of this crate, that has no inherent
    /// constructor named `new*` or `from_*` taking just an `A`.
    ///
    /// The Google Rust Style Guide, "Pair important `From` impls with explicit constructor
    /// methods": a type whose `From` impl is a primary way to build it also gets an inherent
    /// `::new()`, `::new_from_foo()` or `::from_foo()`, which callers can find and name.
    ///
    /// Error types are skipped, since their `From` impls exist for `?`. The lint can't tell an
    /// important conversion from a minor one, so allow it on a `From` impl that isn't one.
    ///
    /// Code a macro writes isn't checked. Code a proc macro copies from its caller's tokens, such
    /// as the body of a function under an attribute macro, is: its author wrote it there.
    ///
    /// ```rust,ignore
    /// impl From<u64> for Funds {
    ///     fn from(cents: u64) -> Self { Self { cents } }
    /// }
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// impl Funds {
    ///     fn from_cents(cents: u64) -> Self { Self { cents } }
    /// }
    ///
    /// impl From<u64> for Funds {
    ///     fn from(cents: u64) -> Self { Self::from_cents(cents) }
    /// }
    /// ```
    pub FROM_WITHOUT_CONSTRUCTOR,
    Warn,
    "a `From` impl without a matching named constructor"
}

declare_lint_pass!(FromWithoutConstructor => [FROM_WITHOUT_CONSTRUCTOR]);

impl<'tcx> LateLintPass<'tcx> for FromWithoutConstructor {
    fn check_item(&mut self, cx: &LateContext<'tcx>, item: &'tcx Item<'tcx>) {
        if item.span.from_expansion() {
            return;
        }
        let ItemKind::Impl(implementation) = item.kind else {
            return;
        };
        if implementation.of_trait.is_none() {
            return;
        }
        let tcx = cx.tcx;
        let impl_id = item.owner_id.to_def_id();
        let trait_ref = tcx
            .impl_trait_ref(impl_id)
            .instantiate_identity()
            .skip_normalization();
        if !tcx.is_diagnostic_item(sym::From, trait_ref.def_id) {
            return;
        }
        let ty::Adt(adt, _) = trait_ref.self_ty().kind() else {
            return;
        };
        if !adt.did().is_local() || crate::trait_impls::implements_error(tcx, trait_ref.self_ty()) {
            return;
        }
        let source = trait_ref.args.type_at(1);
        if has_constructor_from(tcx, *adt, source) {
            return;
        }
        let type_name = tcx.item_name(adt.did());
        let source = rustc_middle::ty::print::with_forced_trimmed_paths!(source.to_string());
        span_lint_and_help(
            cx,
            FROM_WITHOUT_CONSTRUCTOR,
            tcx.def_span(impl_id),
            format!("`From<{source}>` for `{type_name}` has no matching constructor"),
            None,
            format!(
                "if this conversion is a main way to build a `{type_name}`, add an inherent `new`, \
                 `new_from_*` or `from_*` fn taking a `{source}`, as the Google Rust Style Guide's \"Pair \
                 important `From` impls with explicit constructor methods\" asks"
            ),
        );
    }
}

/// Whether an inherent fn named `new*` or `from_*` takes one parameter whose type could be
/// `source`. Generic parameters on either side match anything, so a generic `From` impl or
/// constructor never raises a false alarm.
fn has_constructor_from<'tcx>(tcx: TyCtxt<'tcx>, adt: AdtDef<'tcx>, source: Ty<'tcx>) -> bool {
    let source = tcx.erase_and_anonymize_regions(source);
    tcx.inherent_impls(adt.did())
        .iter()
        .flat_map(|&impl_id| tcx.associated_items(impl_id).in_definition_order())
        .filter(|item| item.is_fn())
        .filter(|item| {
            let name = item.name();
            name.as_str().starts_with("new") || name.as_str().starts_with("from_")
        })
        .any(|item| {
            let signature = tcx.instantiate_bound_regions_with_erased(
                tcx.fn_sig(item.def_id)
                    .instantiate_identity()
                    .skip_normalization(),
            );
            let [parameter] = signature.inputs() else {
                return false;
            };
            let parameter = tcx.erase_and_anonymize_regions(*parameter);
            DeepRejectCtxt::relate_infer_infer(tcx).types_may_unify(parameter, source)
        })
}

#[test]
fn ui() {
    crate::ui_harness::ui_test(FROM_WITHOUT_CONSTRUCTOR);
}
