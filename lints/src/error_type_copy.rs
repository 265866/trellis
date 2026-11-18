use clippy_utils::diagnostics::span_lint_and_help;
use clippy_utils::ty::is_copy;
use rustc_hir::{Item, ItemKind};
use rustc_lint::{LateContext, LateLintPass};
use rustc_lint_defs::{declare_lint, declare_lint_pass};
use rustc_middle::ty::GenericParamDefKind;

declare_lint! {
    /// Checks for structs and enums that implement `std::error::Error` and aren't `Copy`,
    /// although every field is `Copy`, they have no type or const parameters, and they don't
    /// implement `Drop`.
    ///
    /// The Google Rust Style Guide, "Specific guidance for important traits": error types
    /// implement `Copy` whenever possible, because `Copy` types pass by value without moving
    /// ownership and so are simpler to manage and propagate.
    ///
    /// Code a macro writes isn't checked. Code a proc macro copies from its caller's tokens, such
    /// as the body of a function under an attribute macro, is: its author wrote it there.
    ///
    /// ```rust,ignore
    /// #[derive(Debug)]
    /// enum OrderError {
    ///     Offline,
    ///     AmountTooLarge { limit: u64 },
    /// }
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// #[derive(Clone, Copy, Debug)]
    /// enum OrderError {
    ///     Offline,
    ///     AmountTooLarge { limit: u64 },
    /// }
    /// ```
    pub ERROR_TYPE_COPY,
    Warn,
    "an error type that could be `Copy` but isn't"
}

declare_lint_pass!(ErrorTypeCopy => [ERROR_TYPE_COPY]);

impl<'tcx> LateLintPass<'tcx> for ErrorTypeCopy {
    /// Reports an error type that isn't `Copy` though all its fields are. A type with a type
    /// parameter is left alone, since the parameter makes `Copy` depend on the caller's type;
    /// lifetimes don't.
    fn check_item(&mut self, cx: &LateContext<'tcx>, item: &'tcx Item<'tcx>) {
        if item.span.from_expansion() {
            return;
        }
        let (ItemKind::Struct(ident, ..) | ItemKind::Enum(ident, ..)) = item.kind else {
            return;
        };
        let id = item.owner_id.to_def_id();
        let has_type_params = cx
            .tcx
            .generics_of(id)
            .own_params
            .iter()
            .any(|param| !matches!(param.kind, GenericParamDefKind::Lifetime));
        let ty = cx.tcx.type_of(id).instantiate_identity().skip_norm_wip();
        let adt = cx.tcx.adt_def(id);
        if has_type_params
            || !crate::trait_impls::implements_error(cx.tcx, ty)
            || is_copy(cx, ty)
            || adt.has_dtor(cx.tcx)
        {
            return;
        }
        let fields_are_copy = adt.all_fields().all(|field| {
            is_copy(
                cx,
                cx.tcx
                    .type_of(field.did)
                    .instantiate_identity()
                    .skip_norm_wip(),
            )
        });
        if !fields_are_copy {
            return;
        }
        span_lint_and_help(
            cx,
            ERROR_TYPE_COPY,
            ident.span,
            format!("error type `{ident}` could be `Copy`"),
            None,
            "derive `Clone` and `Copy`, as the Google Rust Style Guide's \"Specific guidance for \
             important traits\" asks of error types",
        );
    }
}

#[test]
fn ui() {
    crate::ui_harness::ui_test(ERROR_TYPE_COPY);
}
