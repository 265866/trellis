use clippy_utils::diagnostics::span_lint_and_help;
use clippy_utils::sym;
use rustc_ast::{Item, ItemKind};
use rustc_lint::{EarlyContext, EarlyLintPass, LintContext};
use rustc_lint_defs::{declare_lint, declare_lint_pass};

use crate::config;

declare_lint! {
    /// Checks for macro definitions outside the crates `macro_crates` in `dylint.toml` lists:
    /// every `macro_rules!` (and unstable `macro`) item, and every function marked
    /// `#[proc_macro]`, `#[proc_macro_derive]` or `#[proc_macro_attribute]`.
    ///
    /// The Google Rust Style Guide, "Macros": prefer ordinary functions, types, traits and modules,
    /// then `macro_rules!`, and procedural macros last, weighing what a macro costs readers and
    /// tools. The lints in this library also skip the code a macro writes, so a macro would let
    /// that code past them. A crate may define macros only once `macro_crates` lists it, which is a
    /// rule change, and `suppression_policy` rejects an `expect` of this lint.
    ///
    /// ```rust,ignore
    /// macro_rules! price {
    ///     ($amount:expr, $factor:expr) => {
    ///         $amount * $factor
    ///     };
    /// }
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// fn price(amount: u64, factor: u64) -> u64 {
    ///     amount * factor
    /// }
    /// ```
    pub MACRO_DEFINITION,
    Warn,
    "a `macro_rules!` or procedural macro definition outside the crates `macro_crates` lists"
}

declare_lint_pass!(MacroDefinition => [MACRO_DEFINITION]);

impl EarlyLintPass for MacroDefinition {
    fn check_item(&mut self, cx: &EarlyContext<'_>, item: &Item) {
        if item.span.from_expansion() {
            return;
        }
        let (span, message) = match &item.kind {
            ItemKind::MacroDef(name, definition) => (
                item.span.with_hi(name.span.hi()),
                if definition.macro_rules {
                    "`macro_rules!` definition"
                } else {
                    "`macro` definition"
                },
            ),
            ItemKind::Fn(function)
                if item.attrs.iter().any(|attr| {
                    attr.has_any_name(&[
                        sym::proc_macro,
                        sym::proc_macro_derive,
                        sym::proc_macro_attribute,
                    ])
                }) =>
            {
                (function.ident.span, "procedural macro definition")
            }
            _ => return,
        };
        let session = cx.sess();
        if config::lists_current_crate(session, &config::get(session).macro_crates) {
            return;
        }
        span_lint_and_help(
            cx,
            MACRO_DEFINITION,
            span,
            format!("{message} in a crate not listed in `macro_crates`"),
            None,
            "use a function, type or trait. If only a macro will do, ask for a crate of its own \
             listed in `macro_crates` under `[trellis_lints]` in `dylint.toml`, which is a rule \
             change: `trellis_lints` skips the code a macro writes",
        );
    }
}

#[test]
fn ui() {
    crate::ui_harness::ui_test(MACRO_DEFINITION);
}

/// Runs the same fixture with `dylint.toml` listing its crate, from a copy without the expected
/// warnings, so the test passes only if nothing is reported.
#[test]
fn ui_listed_crate() {
    let fixture = crate::ui_harness::temp_dir().join("macro_definition_listed");
    let file = "macro_definition.rs";
    std::fs::create_dir_all(&fixture).expect("the temporary directory should be writable");
    std::fs::copy(
        std::path::Path::new("ui/macro_definition").join(file),
        fixture.join(file),
    )
    .expect("the fixture should be copyable");
    crate::ui_harness::ui_test_of(MACRO_DEFINITION, &fixture)
        .dylint_toml(crate::ui_harness::dylint_toml_with(
            "macro_crates",
            Some(r#"["macro-definition"]"#),
        ))
        .run();
    std::fs::remove_dir_all(&fixture).expect("the fixture copy should be removable");
}
