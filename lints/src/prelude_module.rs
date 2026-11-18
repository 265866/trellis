use clippy_utils::diagnostics::span_lint_and_help;
use rustc_ast::{Item, ItemKind};
use rustc_lint::{EarlyContext, EarlyLintPass};
use rustc_lint_defs::{declare_lint, declare_lint_pass};

declare_lint! {
    /// Checks for modules named `prelude`, inline or loaded from a file.
    ///
    /// The Google Rust Style Guide, "Don't use glob imports": "defining preludes should be
    /// avoided". A prelude exists to be glob-imported, and glob imports hide where each name comes
    /// from.
    ///
    /// ```rust,ignore
    /// pub mod prelude {
    ///     pub use crate::shelf::Shelf;
    ///     pub use crate::order::Order;
    /// }
    /// ```
    ///
    /// Use instead: let callers import the items they use by name.
    ///
    /// ```rust,ignore
    /// use inventory::shelf::Shelf;
    /// use inventory::order::Order;
    /// ```
    pub PRELUDE_MODULE,
    Warn,
    "a module named `prelude`"
}

declare_lint_pass!(PreludeModule => [PRELUDE_MODULE]);

impl EarlyLintPass for PreludeModule {
    fn check_item(&mut self, cx: &EarlyContext<'_>, item: &Item) {
        if item.span.from_expansion() {
            return;
        }
        let ItemKind::Mod(_, ident, _) = item.kind else {
            return;
        };
        if ident.name.as_str() != "prelude" {
            return;
        }
        span_lint_and_help(
            cx,
            PRELUDE_MODULE,
            ident.span,
            "module named `prelude`",
            None,
            "remove it and have callers import items by name, as the Google Rust Style Guide's \
             \"Don't use glob imports\" asks",
        );
    }
}

#[test]
fn ui() {
    crate::ui_harness::ui_test(PRELUDE_MODULE);
}
