use clippy_utils::diagnostics::span_lint_and_help;
use clippy_utils::sym;
use clippy_utils::ty::implements_trait;
use rustc_hir::{Item, ItemKind, VariantData};
use rustc_lint::{LateContext, LateLintPass};
use rustc_lint_defs::{declare_lint, declare_lint_pass};
use rustc_middle::ty::TypeVisitableExt as _;

declare_lint! {
    /// Checks for structs with named fields whose name ends in `Options` and that don't implement
    /// `Default`, derived or by hand. A struct with a field whose type has no `Default`, such as a
    /// required URL, isn't a bag of options and isn't checked.
    ///
    /// The Google Rust Style Guide, "Options structs should implement Default": a bag of options
    /// for a function implements `Default`, so callers can write
    /// `OrderOptions { express: true, ..Default::default() }`.
    ///
    /// Code a macro writes isn't checked. Code a proc macro copies from its caller's tokens, such
    /// as the body of a function under an attribute macro, is: its author wrote it there.
    ///
    /// ```rust,ignore
    /// struct OrderOptions {
    ///     express: bool,
    ///     receipts: u32,
    /// }
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// #[derive(Default)]
    /// struct OrderOptions {
    ///     express: bool,
    ///     receipts: u32,
    /// }
    /// ```
    pub OPTIONS_STRUCT_DEFAULT,
    Warn,
    "an `Options` struct that doesn't implement `Default`"
}

declare_lint_pass!(OptionsStructDefault => [OPTIONS_STRUCT_DEFAULT]);

impl<'tcx> LateLintPass<'tcx> for OptionsStructDefault {
    /// Struct update syntax needs named fields, so unit and tuple structs are out of scope. Any
    /// `Default` impl counts rather than a proof of `Self: Default`, so a generic struct whose
    /// impl needs bounds on its parameters counts too.
    fn check_item(&mut self, cx: &LateContext<'tcx>, item: &'tcx Item<'tcx>) {
        if item.span.from_expansion() {
            return;
        }
        let ItemKind::Struct(ident, _, VariantData::Struct { fields, .. }) = item.kind else {
            return;
        };
        if fields.is_empty() || !ident.name.as_str().ends_with("Options") {
            return;
        }
        let Some(default) = cx.tcx.get_diagnostic_item(sym::Default) else {
            return;
        };
        let defaultable = fields.iter().all(|field| {
            let ty = cx
                .tcx
                .type_of(field.def_id)
                .instantiate_identity()
                .skip_normalization();
            ty.has_param() || implements_trait(cx, ty, default, &[])
        });
        if !defaultable {
            return;
        }
        let ty = cx
            .tcx
            .type_of(item.owner_id)
            .instantiate_identity()
            .skip_normalization();
        if cx
            .tcx
            .non_blanket_impls_for_ty(default, ty)
            .next()
            .is_some()
        {
            return;
        }
        span_lint_and_help(
            cx,
            OPTIONS_STRUCT_DEFAULT,
            ident.span,
            format!("options struct `{ident}` doesn't implement `Default`"),
            None,
            "derive or implement `Default`, as the Google Rust Style Guide's \"Options structs \
             should implement Default\" asks",
        );
    }
}

#[test]
fn ui() {
    crate::ui_harness::ui_test(OPTIONS_STRUCT_DEFAULT);
}
