use clippy_utils::diagnostics::span_lint_and_help;
use rustc_ast::Crate;
use rustc_lint::{EarlyContext, EarlyLintPass};
use rustc_lint_defs::{declare_lint, declare_lint_pass};

use crate::local_source::LocalSource;

declare_lint! {
    /// Checks for an outer doc comment, `///` or `/** */`, written after an outer attribute of
    /// the same item.
    ///
    /// The Rust Style Guide, which the Google Rust Style Guide follows, puts doc comments before
    /// attributes, so an item reads as its docs first and its attributes next to the code they
    /// change.
    ///
    /// The lint reads source text, like `code_comment`, so it reports from the crate root and an
    /// attribute on an item can't silence it.
    ///
    /// ```rust,ignore
    /// #[derive(Debug)]
    /// /// An order of the shelves.
    /// struct Order;
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// /// An order of the shelves.
    /// #[derive(Debug)]
    /// struct Order;
    /// ```
    pub DOC_AFTER_ATTRIBUTE,
    Warn,
    "a doc comment written after an attribute of the same item"
}

declare_lint_pass!(DocAfterAttribute => [DOC_AFTER_ATTRIBUTE]);

impl EarlyLintPass for DocAfterAttribute {
    fn check_crate(&mut self, cx: &EarlyContext<'_>, krate: &Crate) {
        for &span in &LocalSource::get(cx, krate).docs_after_attributes {
            span_lint_and_help(
                cx,
                DOC_AFTER_ATTRIBUTE,
                span,
                "doc comment after an attribute",
                None,
                "move the doc comment above the item's attributes, as the Rust Style Guide asks",
            );
        }
    }
}

#[test]
fn ui() {
    crate::ui_harness::ui_test(DOC_AFTER_ATTRIBUTE);
}
