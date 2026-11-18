use clippy_utils::diagnostics::span_lint_and_help;
use rustc_ast::visit::{FnCtxt, FnKind};
use rustc_ast::{
    AttrKind, Attribute, BlockCheckMode, Expr, ExprKind, Item, ItemKind, NodeId, Safety,
    UnsafeSource,
};
use rustc_lint::{EarlyContext, EarlyLintPass, LintContext};
use rustc_lint_defs::{declare_lint, declare_lint_pass};
use rustc_span::{BytePos, Span};

declare_lint! {
    /// Checks for `unsafe` blocks, `unsafe fn`s, `unsafe impl`s, `unsafe trait`s, `extern`
    /// blocks and `#[unsafe(..)]` attributes in a crate that the `unsafe_crates` list in
    /// `dylint.toml` doesn't name. The list is empty by default:
    ///
    /// ```toml
    /// [trellis_lints]
    /// unsafe_crates = ["app_sys"]
    /// ```
    ///
    /// The Google Rust Style Guide, "Unsafe code": encapsulate unsafe code in a safe abstraction
    /// isolated from business logic, ideally in a separate crate with few dependencies.
    ///
    /// ```rust,ignore
    /// // In a crate of business logic:
    /// let window = unsafe { GetForegroundWindow() };
    /// ```
    ///
    /// Use instead a small crate listed in `unsafe_crates` that wraps the call in a safe API:
    ///
    /// ```rust,ignore
    /// // In `app_sys`:
    /// pub fn foreground_window() -> HWND {
    ///     // SAFETY: `GetForegroundWindow` takes no arguments and has no preconditions.
    ///     unsafe { GetForegroundWindow() }
    /// }
    /// ```
    pub UNSAFE_OUTSIDE_UNSAFE_CRATE,
    Warn,
    "unsafe code in a crate not listed in `dylint.toml`"
}

declare_lint_pass!(UnsafeOutsideUnsafeCrate => [UNSAFE_OUTSIDE_UNSAFE_CRATE]);

impl EarlyLintPass for UnsafeOutsideUnsafeCrate {
    /// Reports an `unsafe` block at its `unsafe` keyword, or at the whole block if the source
    /// there isn't `unsafe`.
    fn check_expr(&mut self, cx: &EarlyContext<'_>, expr: &Expr) {
        if let ExprKind::Block(block, _) = &expr.kind
            && block.rules == BlockCheckMode::Unsafe(UnsafeSource::UserProvided)
        {
            let keyword = block.span.with_hi(block.span.lo() + BytePos(6));
            let keyword_written = cx
                .sess()
                .source_map()
                .span_to_snippet(keyword)
                .is_ok_and(|text| text == "unsafe");
            lint(
                cx,
                if keyword_written { keyword } else { block.span },
                "`unsafe` block",
            );
        }
    }

    /// Reports an `unsafe fn`. A function in an `extern` block is reported with the block.
    fn check_fn(&mut self, cx: &EarlyContext<'_>, kind: FnKind<'_>, _: Span, _: NodeId) {
        if let FnKind::Fn(context, _, function) = kind
            && context != FnCtxt::Foreign
            && let Safety::Unsafe(keyword) = function.sig.header.safety
        {
            lint(cx, keyword, "`unsafe fn`");
        }
    }

    /// Reports an `unsafe impl`, an `unsafe trait` and every `extern` block. Before the 2024
    /// edition, an `extern` block needs no `unsafe` but is just as unsafe.
    fn check_item(&mut self, cx: &EarlyContext<'_>, item: &Item) {
        if item.span.from_expansion() {
            return;
        }
        match &item.kind {
            ItemKind::Impl(implementation) => {
                if let Some(header) = &implementation.of_trait
                    && let Safety::Unsafe(keyword) = header.safety
                {
                    lint(cx, keyword, "`unsafe impl`");
                }
            }
            ItemKind::Trait(definition) => {
                if let Safety::Unsafe(keyword) = definition.safety {
                    lint(cx, keyword, "`unsafe trait`");
                }
            }
            ItemKind::ForeignMod(block) => {
                let span = match block.safety {
                    Safety::Unsafe(keyword) => keyword.to(block.extern_span),
                    Safety::Safe(_) | Safety::Default => block.extern_span,
                };
                lint(cx, span, "`extern` block");
            }
            _ => {}
        }
    }

    fn check_attribute(&mut self, cx: &EarlyContext<'_>, attr: &Attribute) {
        if let AttrKind::Normal(normal) = &attr.kind
            && let Safety::Unsafe(keyword) = normal.item.unsafety
        {
            lint(cx, keyword, "`#[unsafe(..)]` attribute");
        }
    }
}

/// Reports `span` unless it is code a macro or derive wrote. A derive's `unsafe impl`, such as
/// the `TrivialClone` impl that `#[derive(Clone, Copy)]` writes, has an `unsafe` with a dummy
/// span, which `from_expansion` doesn't catch.
fn lint(cx: &EarlyContext<'_>, span: Span, construct: &str) {
    if span.from_expansion() || span.is_dummy() || is_unsafe_crate(cx) {
        return;
    }
    span_lint_and_help(
        cx,
        UNSAFE_OUTSIDE_UNSAFE_CRATE,
        span,
        format!("{construct} in a crate not listed in `unsafe_crates`"),
        None,
        "move it into a small crate that wraps it in a safe API, and list that crate in \
         `unsafe_crates` under `[trellis_lints]` in `dylint.toml`",
    );
}

/// Whether `unsafe_crates` in `dylint.toml` lists the crate being compiled.
fn is_unsafe_crate(cx: &EarlyContext<'_>) -> bool {
    let session = cx.sess();
    crate::config::lists_current_crate(session, &crate::config::get(session).unsafe_crates)
}

#[test]
fn ui() {
    crate::ui_harness::ui_test(UNSAFE_OUTSIDE_UNSAFE_CRATE);
}

/// Runs the same fixture with `dylint.toml` listing its crate, from a copy without the expected
/// warnings, so the test passes only if nothing is reported. The fixture's `#[expect]` goes
/// unfulfilled there, as it should, so that warning is off.
#[test]
fn ui_listed_crate() {
    let fixture = crate::ui_harness::temp_dir().join("unsafe_outside_unsafe_crate_listed");
    let file = "unsafe_outside_unsafe_crate.rs";
    std::fs::create_dir_all(&fixture).expect("the temporary directory should be writable");
    std::fs::copy(
        std::path::Path::new("ui/unsafe_outside_unsafe_crate").join(file),
        fixture.join(file),
    )
    .expect("the fixture should be copyable");
    crate::ui_harness::ui_test_of(UNSAFE_OUTSIDE_UNSAFE_CRATE, &fixture)
        .rustc_flags(["-A", "unfulfilled_lint_expectations"])
        .dylint_toml(crate::ui_harness::dylint_toml_with(
            "unsafe_crates",
            Some(r#"["unsafe_outside_unsafe_crate"]"#),
        ))
        .run();
    std::fs::remove_dir_all(&fixture).expect("the fixture copy should be removable");
}
