use clippy_utils::diagnostics::span_lint_and_help;
use rustc_ast::{AttrStyle, Attribute};
use rustc_lint::{EarlyContext, EarlyLintPass, LintContext};
use rustc_lint_defs::{declare_lint, declare_lint_pass};

declare_lint! {
    /// Checks for doc comments (`///`, `//!`, `/** */`, `/*! */` and `#[doc = ".."]`) whose first
    /// word starts with a lowercase letter. Only the first line of each item's outer docs, and of
    /// its inner docs, is checked. Docs that start with a backtick, a link, a digit or
    /// punctuation are left alone, as are first words with a capital inside, such as `iOS`, and
    /// the words in `lowercase_doc_words` in `dylint.toml`, which are written lowercase even at the
    /// start of a sentence. A listed word also covers its possessive, such as `trellis's`. The
    /// list has no default; the workspace's `dylint.toml` sets it, as trellis's does:
    ///
    /// ```toml
    /// [trellis_lints]
    /// lowercase_doc_words = ["trellis", "e.g.", "i.e."]
    /// ```
    ///
    /// The Google Rust Style Guide, "When and how to write documentation": use capitalization
    /// and punctuation.
    ///
    /// ```rust,ignore
    /// /// returns the price for one order.
    /// fn price(order: &Order) -> u64 { /* ... */ }
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// /// Returns the price for one order.
    /// fn price(order: &Order) -> u64 { /* ... */ }
    /// ```
    pub DOC_STARTS_LOWERCASE,
    Warn,
    "a doc comment that starts with a lowercase letter"
}

declare_lint_pass!(DocStartsLowercase => [DOC_STARTS_LOWERCASE]);

impl EarlyLintPass for DocStartsLowercase {
    /// `check_attributes` sees an item's doc lines together, so this checks only the first line of
    /// its outer docs and of its inner docs.
    fn check_attributes(&mut self, cx: &EarlyContext<'_>, attrs: &[Attribute]) {
        for style in [AttrStyle::Outer, AttrStyle::Inner] {
            let first_line = attrs
                .iter()
                .filter(|attr| attr.style == style)
                .find_map(|attr| {
                    Some((attr, attr.doc_str()?))
                        .filter(|(_, text)| !text.as_str().trim().is_empty())
                });
            let Some((attr, text)) = first_line else {
                continue;
            };
            if attr.span.from_expansion() || !starts_lowercase(cx, text.as_str()) {
                continue;
            }
            span_lint_and_help(
                cx,
                DOC_STARTS_LOWERCASE,
                attr.span,
                "doc comment starts with a lowercase letter",
                None,
                "start it with a capital letter, or put code in backticks",
            );
        }
    }
}

/// Whether the first word of `text` starts with a lowercase letter, has no capitals, and isn't
/// one of `lowercase_doc_words`.
fn starts_lowercase(cx: &EarlyContext<'_>, text: &str) -> bool {
    let first_word = text.split_whitespace().next().unwrap_or_default();
    if !first_word.chars().next().is_some_and(char::is_lowercase)
        || first_word.chars().any(char::is_uppercase)
    {
        return false;
    }
    !crate::config::get(cx.sess())
        .lowercase_doc_words
        .iter()
        .any(|word| is_word_or_its_form(first_word, word))
}

/// Whether `first_word` is `word`, or `word` followed by punctuation such as `'s` or `,`.
fn is_word_or_its_form(first_word: &str, word: &str) -> bool {
    first_word
        .strip_prefix(word)
        .is_some_and(|rest| !rest.starts_with(char::is_alphanumeric))
}

#[test]
fn ui() {
    crate::ui_harness::ui_test(DOC_STARTS_LOWERCASE);
}

/// Runs a fixture with `lowercase_doc_words` set to other words than trellis's `dylint.toml` sets.
#[test]
fn ui_configured_words() {
    crate::ui_harness::ui_test_of(DOC_STARTS_LOWERCASE, "ui/doc_starts_lowercase_configured")
        .dylint_toml(crate::ui_harness::dylint_toml_with(
            "lowercase_doc_words",
            Some(r#"["npm"]"#),
        ))
        .run();
}
