use clippy_utils::diagnostics::span_lint_and_help;
use clippy_utils::res::MaybeDef as _;
use clippy_utils::sym;
use rustc_hir::def_id::DefId;
use rustc_hir::{FnSig, ImplItem, ImplItemKind, TraitItem, TraitItemKind};
use rustc_lint::{LateContext, LateLintPass};
use rustc_lint_defs::{declare_lint, declare_lint_pass};
use rustc_span::Ident;

declare_lint! {
    /// Checks constructors, the associated functions without `self` of inherent impls and trait
    /// declarations: one named `try_new` or `try_new_foo` must return a `Result` (or an
    /// `Option`), and one named `new` or `new_foo` must not return a `Result`. Trait impls take
    /// their names from the trait, so they aren't checked.
    ///
    /// The Google Rust Style Guide, "Naming initialization functions": constructors use `new()`,
    /// or `new_with_foo()` if there is more than one, and a constructor that returns a `Result`
    /// uses `try_new()`. Type aliases such as `io::Result` count as `Result`.
    ///
    /// Code a macro writes isn't checked. Code a proc macro copies from its caller's tokens, such
    /// as the body of a function under an attribute macro, is: its author wrote it there.
    ///
    /// ```rust,ignore
    /// impl Config {
    ///     fn new(path: &Path) -> Result<Self, ConfigError> { ... }
    ///     fn try_new_empty() -> Self { ... }
    /// }
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// impl Config {
    ///     fn try_new(path: &Path) -> Result<Self, ConfigError> { ... }
    ///     fn new_empty() -> Self { ... }
    /// }
    /// ```
    pub TRY_NEW_RESULT,
    Warn,
    "`try_new` that returns neither a `Result` nor an `Option`, or `new` that returns a `Result`"
}

declare_lint_pass!(TryNewResult => [TRY_NEW_RESULT]);

impl<'tcx> LateLintPass<'tcx> for TryNewResult {
    fn check_impl_item(&mut self, cx: &LateContext<'tcx>, item: &'tcx ImplItem<'tcx>) {
        if let ImplItemKind::Fn(signature, _) = item.kind
            && !item.span.from_expansion()
            && !crate::trait_impls::is_in_trait_impl(item)
        {
            check_constructor(cx, item.ident, signature, item.owner_id.to_def_id());
        }
    }

    fn check_trait_item(&mut self, cx: &LateContext<'tcx>, item: &'tcx TraitItem<'tcx>) {
        if let TraitItemKind::Fn(signature, _) = item.kind
            && !item.span.from_expansion()
        {
            check_constructor(cx, item.ident, signature, item.owner_id.to_def_id());
        }
    }
}

/// Checks the associated function `ident`, declared with `signature`, whose definition is
/// `function`. Methods taking `self` aren't constructors, and an `async fn` returns a future, so
/// the `Result` sits behind an opaque type this lint doesn't look through; both are left alone.
/// The guide names `Result`-returning constructors `try_new`; one returning `Option` may use the
/// name too.
fn check_constructor(cx: &LateContext<'_>, ident: Ident, signature: FnSig<'_>, function: DefId) {
    if signature.decl.implicit_self().has_implicit_self() || signature.header.is_async() {
        return;
    }
    let name = ident.name.as_str();
    let output = cx
        .tcx
        .fn_sig(function)
        .instantiate_identity()
        .skip_norm_wip()
        .skip_binder()
        .output();
    let returns_result = output.is_diag_item(cx, sym::Result);
    let returns_option = output.is_diag_item(cx, sym::Option);
    if is_named(name, "try_new") && !returns_result && !returns_option {
        span_lint_and_help(
            cx,
            TRY_NEW_RESULT,
            ident.span,
            format!("`{name}` doesn't return a `Result` or an `Option`"),
            None,
            format!(
                "return a `Result`, or name it `{}`, as the Google Rust Style Guide's \"Naming \
                 initialization functions\" asks",
                &name["try_".len()..]
            ),
        );
    } else if is_named(name, "new") && returns_result {
        span_lint_and_help(
            cx,
            TRY_NEW_RESULT,
            ident.span,
            format!("constructor `{name}` returns a `Result`"),
            None,
            format!(
                "name it `try_{name}`, as the Google Rust Style Guide's \"Naming initialization \
                 functions\" asks"
            ),
        );
    }
}

/// Whether `name` is `stem` or starts with `stem_`.
fn is_named(name: &str, stem: &str) -> bool {
    name.strip_prefix(stem)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('_'))
}

#[test]
fn ui() {
    crate::ui_harness::ui_test(TRY_NEW_RESULT);
}
