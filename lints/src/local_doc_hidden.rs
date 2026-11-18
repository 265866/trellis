use clippy_utils::diagnostics::span_lint_and_help;
use rustc_ast::{Attribute, Crate};
use rustc_lint::{EarlyContext, EarlyLintPass};
use rustc_lint_defs::{declare_lint, impl_lint_pass};
use rustc_span::sym;

use crate::attribute_origin::{self, Origin};
use crate::local_source::LocalSource;

declare_lint! {
    /// Checks for `#[doc(hidden)]` and `#![doc(hidden)]` on the crate's own items, written in the
    /// source or by a macro the crate defines.
    ///
    /// A hidden item is left out of the docs and out of `missing_docs`, so hiding one is a way to
    /// skip documenting it. The Google Rust Style Guide treats hidden items as private to the
    /// crate that owns them: hide an item only when a macro needs it public, and say so with an
    /// `expect`. Attributes that macros from other crates write, such as those `#[test]` and
    /// serde's derives add to the items they generate, are left alone.
    ///
    /// ```rust,ignore
    /// #[doc(hidden)]
    /// pub fn price_table() -> &'static [u32] { &PRICES }
    /// ```
    ///
    /// Use instead: document the item, or make it private. When a macro needs it public:
    ///
    /// ```rust,ignore
    /// #[cfg_attr(
    ///     dylint_lib = "trellis_lints",
    ///     expect(local_doc_hidden, reason = "`order!` expands to a call to it")
    /// )]
    /// #[doc(hidden)]
    /// pub fn order_unchecked(shelves: &mut Shelves) { ... }
    /// ```
    pub LOCAL_DOC_HIDDEN,
    Warn,
    "`#[doc(hidden)]` on an item of the crate"
}

/// Reads the crate's source in `check_crate`, before any attribute, to tell the attributes
/// written there from those macros write.
#[derive(Default)]
pub struct LocalDocHidden {
    source: Option<&'static LocalSource>,
}

impl_lint_pass!(LocalDocHidden => [LOCAL_DOC_HIDDEN]);

impl EarlyLintPass for LocalDocHidden {
    fn check_crate(&mut self, cx: &EarlyContext<'_>, krate: &Crate) {
        self.source = Some(LocalSource::get(cx, krate));
    }

    fn check_attribute(&mut self, cx: &EarlyContext<'_>, attribute: &Attribute) {
        let hides = attribute.has_name(sym::doc)
            && attribute
                .meta_item_list()
                .is_some_and(|items| items.iter().any(|item| item.has_name(sym::hidden)));
        let Some(source) = self.source.filter(|_| hides) else {
            return;
        };
        let local = match attribute_origin::origin(cx, source, attribute) {
            Origin::Source | Origin::Unwritten => true,
            Origin::Macro { defined_in, .. } => defined_in.is_none(),
            Origin::Trusted { .. } => false,
        };
        if local {
            span_lint_and_help(
                cx,
                LOCAL_DOC_HIDDEN,
                attribute.span,
                "`#[doc(hidden)]` on an item of this crate",
                None,
                "a hidden item skips `missing_docs`: document it, or make it private. If a macro \
                 needs it public, say so with `#[cfg_attr(dylint_lib = \"trellis_lints\", \
                 expect(local_doc_hidden, reason = \"...\"))]` on the item",
            );
        }
    }
}

#[test]
fn ui() {
    crate::ui_harness::ui_test(LOCAL_DOC_HIDDEN);
}

/// A test build, where `#[test]` generates hidden items, draws no report.
#[test]
fn ui_tests() {
    crate::ui_harness::ui_test_of(LOCAL_DOC_HIDDEN, "ui/local_doc_hidden_tests")
        .rustc_flags(["--test"])
        .run();
}
