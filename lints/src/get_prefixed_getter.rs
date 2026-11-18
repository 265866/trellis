use clippy_utils::diagnostics::span_lint_and_help;
use rustc_hir::{FnDecl, ImplItem, ImplItemKind, TraitItem, TraitItemKind};
use rustc_lint::{LateContext, LateLintPass};
use rustc_lint_defs::{declare_lint, declare_lint_pass};
use rustc_span::edition::ALL_EDITIONS;
use rustc_span::{Ident, Symbol};

declare_lint! {
    /// Checks for inherent methods and trait method declarations named `get_foo` that take only
    /// `self`. Names that couldn't drop the `get_` are left alone: `get_mut`, `get_ref` and other
    /// keywords, and `get_or_insert`-style `get_or_*` names, which say what happens when the
    /// value is missing. Trait impls take their names from the trait, so they aren't checked.
    ///
    /// The Google Rust Style Guide, "Getters and setters": a getter for `foo` is named `foo()`,
    /// following the standard library.
    ///
    /// Code a macro writes isn't checked. Code a proc macro copies from its caller's tokens, such
    /// as the body of a function under an attribute macro, is: its author wrote it there.
    ///
    /// ```rust,ignore
    /// impl Order {
    ///     fn get_price(&self) -> u64 { self.price }
    /// }
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// impl Order {
    ///     fn price(&self) -> u64 { self.price }
    /// }
    /// ```
    pub GET_PREFIXED_GETTER,
    Warn,
    "a getter named `get_foo` rather than `foo`"
}

declare_lint_pass!(GetPrefixedGetter => [GET_PREFIXED_GETTER]);

impl<'tcx> LateLintPass<'tcx> for GetPrefixedGetter {
    fn check_impl_item(&mut self, cx: &LateContext<'tcx>, item: &'tcx ImplItem<'tcx>) {
        if let ImplItemKind::Fn(signature, _) = item.kind
            && !item.span.from_expansion()
            && !crate::trait_impls::is_in_trait_impl(item)
        {
            check_method(cx, item.ident, signature.decl);
        }
    }

    fn check_trait_item(&mut self, cx: &LateContext<'tcx>, item: &'tcx TraitItem<'tcx>) {
        if let TraitItemKind::Fn(signature, _) = item.kind
            && !item.span.from_expansion()
        {
            check_method(cx, item.ident, signature.decl);
        }
    }
}

/// Checks the inherent or trait method `ident` declared with `decl`. Only a method whose one input
/// is `self` is a getter: `get_by_id(&self, id)` is a lookup.
fn check_method(cx: &LateContext<'_>, ident: Ident, decl: &FnDecl<'_>) {
    let name = ident.name.as_str();
    let Some(field) = name.strip_prefix("get_") else {
        return;
    };
    if field == "or" || field.starts_with("or_") || is_keyword(field) {
        return;
    }
    if !decl.implicit_self().has_implicit_self() || decl.inputs.len() != 1 {
        return;
    }
    span_lint_and_help(
        cx,
        GET_PREFIXED_GETTER,
        ident.span,
        format!("getter named `{name}`"),
        None,
        format!("name it `{field}`, as the Google Rust Style Guide's \"Getters and setters\" asks"),
    );
}

/// Whether `name` is a keyword in any edition, so a method can't be named it without `r#`.
fn is_keyword(name: &str) -> bool {
    let name = Symbol::intern(name);
    ALL_EDITIONS
        .iter()
        .any(|&edition| name.is_reserved(|| edition))
}

#[test]
fn ui() {
    crate::ui_harness::ui_test(GET_PREFIXED_GETTER);
}
