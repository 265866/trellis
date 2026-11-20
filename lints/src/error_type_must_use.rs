use clippy_utils::diagnostics::span_lint_and_help;
use rustc_attr_ir::find_attr;
use rustc_hir::{Item, ItemKind};
use rustc_lint::{LateContext, LateLintPass};
use rustc_lint_defs::{declare_lint, declare_lint_pass};

declare_lint! {
    /// Checks for structs and enums that implement `std::error::Error` but aren't
    /// `#[must_use]`.
    ///
    /// The Google Rust Style Guide, "`#[must_use]` on types": error types, like `Result`, are
    /// `#[must_use]`, so an error that a function returns can't be dropped unnoticed.
    ///
    /// Code a macro writes isn't checked. Code a proc macro copies from its caller's tokens, such
    /// as the body of a function under an attribute macro, is: its author wrote it there.
    ///
    /// ```rust,ignore
    /// #[derive(Debug)]
    /// struct AmountTooLarge;
    ///
    /// impl std::error::Error for AmountTooLarge {}
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// #[derive(Debug)]
    /// #[must_use]
    /// struct AmountTooLarge;
    ///
    /// impl std::error::Error for AmountTooLarge {}
    /// ```
    pub ERROR_TYPE_MUST_USE,
    Warn,
    "an error type without `#[must_use]`"
}

declare_lint_pass!(ErrorTypeMustUse => [ERROR_TYPE_MUST_USE]);

impl<'tcx> LateLintPass<'tcx> for ErrorTypeMustUse {
    fn check_item(&mut self, cx: &LateContext<'tcx>, item: &'tcx Item<'tcx>) {
        if item.span.from_expansion() {
            return;
        }
        let (ItemKind::Struct(ident, ..) | ItemKind::Enum(ident, ..)) = item.kind else {
            return;
        };
        let ty = cx
            .tcx
            .type_of(item.owner_id)
            .instantiate_identity()
            .skip_normalization();
        if !crate::trait_impls::implements_error(cx.tcx, ty)
            || find_attr!(cx.tcx, item.owner_id.to_def_id(), MustUse { .. })
        {
            return;
        }
        span_lint_and_help(
            cx,
            ERROR_TYPE_MUST_USE,
            ident.span,
            format!("error type `{ident}` isn't `#[must_use]`"),
            None,
            "add `#[must_use]`, as the Google Rust Style Guide's \"`#[must_use]` on types\" asks",
        );
    }
}

#[test]
fn ui() {
    crate::ui_harness::ui_test(ERROR_TYPE_MUST_USE);
}
