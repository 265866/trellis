use clippy_utils::diagnostics::span_lint_and_help;
use clippy_utils::sym;
use rustc_ast::{AttrKind, Attribute, Crate};
use rustc_lint::{EarlyContext, EarlyLintPass};
use rustc_lint_defs::{declare_lint, impl_lint_pass};

use crate::local_source::{LocalSource, Meta};

declare_lint! {
    /// Checks for every `#[rustfmt::skip]` and `#[rustfmt::skip::*(..)]`, outer or inner.
    ///
    /// The Google Rust Style Guide, "Opting out of automatic formatting": skip formatting only for
    /// manual alignment or generated code, never for personal style. Each skip carries an
    /// `#[expect]` on the same item whose reason says which of the two it is. (`code_comment` bans
    /// ordinary comments, so the reason can't be a `//` comment.)
    ///
    /// It also flags every `rustfmt::skip` inside a `cfg_attr`, whatever the predicate, reading
    /// the source text: rustfmt skips the item whether or not the predicate holds, so a
    /// `#[cfg_attr(target_os = "freebsd", rustfmt::skip)]` would skip formatting with no `expect`
    /// in any build the lints run. Write `#[rustfmt::skip]` itself, with its `expect`.
    ///
    /// ```rust,ignore
    /// #[rustfmt::skip]
    /// const PRICE_TABLE: [[u32; 3]; 2] = [
    ///     [  2,  5,  10],
    ///     [ 20, 50, 100],
    /// ];
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// #[cfg_attr(
    ///     dylint_lib = "trellis_lints",
    ///     expect(rustfmt_skip_justification, reason = "manual alignment: one column per shelf")
    /// )]
    /// #[rustfmt::skip]
    /// const PRICE_TABLE: [[u32; 3]; 2] = [
    ///     [  2,  5,  10],
    ///     [ 20, 50, 100],
    /// ];
    /// ```
    pub RUSTFMT_SKIP_JUSTIFICATION,
    Warn,
    "`#[rustfmt::skip]` without an `expect` saying why"
}

/// Reads the crate's source in `check_crate`, before any attribute, to tell a skip written as an
/// attribute from one a `cfg_attr` holds, which `check_crate` reports.
#[derive(Default)]
pub struct RustfmtSkipJustification {
    source: Option<&'static LocalSource>,
}

impl_lint_pass!(RustfmtSkipJustification => [RUSTFMT_SKIP_JUSTIFICATION]);

impl EarlyLintPass for RustfmtSkipJustification {
    fn check_crate(&mut self, cx: &EarlyContext<'_>, krate: &Crate) {
        let source = LocalSource::get(cx, krate);
        self.source = Some(source);
        for attribute in &source.attributes {
            if let Some((_, attributes)) = attribute.meta.cfg_attr()
                && attributes.iter().any(holds_skip)
            {
                span_lint_and_help(
                    cx,
                    RUSTFMT_SKIP_JUSTIFICATION,
                    attribute.span,
                    "`rustfmt::skip` inside `cfg_attr`",
                    None,
                    "rustfmt skips the item whatever the predicate, so the skip needs no \
                     `cfg_attr`: write `#[rustfmt::skip]` with its `expect` on the same item",
                );
            }
        }
    }

    fn check_attribute(&mut self, cx: &EarlyContext<'_>, attr: &Attribute) {
        if !matches!(attr.path().as_slice(), [sym::rustfmt, sym::skip, ..])
            || attr.span.from_expansion()
        {
            return;
        }
        let AttrKind::Normal(normal) = &attr.kind else {
            return;
        };
        let in_cfg_attr = self.source.and_then(|source| {
            source
                .written(
                    &crate::attribute_origin::path_text(attr),
                    normal.item.path.span,
                )
                .map(|written| written.in_cfg_attr)
        });
        if in_cfg_attr == Some(true) {
            return;
        }
        span_lint_and_help(
            cx,
            RUSTFMT_SKIP_JUSTIFICATION,
            attr.span,
            "`#[rustfmt::skip]` needs a reason",
            None,
            "skip formatting only for manual alignment or generated code, and say which with \
             `#[cfg_attr(dylint_lib = \"trellis_lints\", expect(rustfmt_skip_justification, \
             reason = \"...\"))]` on the same item",
        );
    }
}

/// Whether `meta`, an attribute inside a `cfg_attr`, is a skip or a `cfg_attr` holding one.
fn holds_skip(meta: &Meta) -> bool {
    match meta.cfg_attr() {
        Some((_, attributes)) => attributes.iter().any(holds_skip),
        None => meta.path == "rustfmt::skip" || meta.path.starts_with("rustfmt::skip::"),
    }
}

#[test]
fn ui() {
    crate::ui_harness::ui_test(RUSTFMT_SKIP_JUSTIFICATION);
}
