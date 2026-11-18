use rustc_ast::Crate;
use rustc_lint::{EarlyContext, EarlyLintPass, LintContext};
use rustc_lint_defs::{declare_lint, declare_lint_pass};
use rustc_span::edition::ALL_EDITIONS;
use rustc_span::{FileName, Span, Symbol};

use crate::command_line_lint::span_lint_from_command_line;

declare_lint! {
    /// Checks for raw identifiers and raw lifetimes, `r#name` and `'r#name`, whose name isn't a
    /// keyword in the crate's edition or a later one.
    ///
    /// A raw identifier exists to name something after a keyword, such as a `r#type` field. On
    /// any other name it changes nothing for the compiler, but it does for a lint that compares
    /// a node with the source text there: clippy's `is_from_proc_macro`, which some lints use to
    /// skip the code a proc macro writes, reads `r#Arc` as text that isn't `Arc`, and skips it.
    /// Keywords of later editions pass, so a crate can rename an identifier before it moves to
    /// the edition that reserves it, as rustc's `keyword_idents` lints ask.
    ///
    /// The lint reads every raw identifier the compiler lexed in the crate's files, including
    /// ones in macro calls and macro bodies. It reports from the crate root, so it has no
    /// `expect` exception: the fix, deleting the `r#`, is always possible.
    ///
    /// ```rust,ignore
    /// pub settings: r#Arc<Settings>,
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// pub settings: Arc<Settings>,
    /// ```
    pub NEEDLESS_RAW_IDENTIFIER,
    Warn,
    "a raw identifier whose name isn't a keyword"
}

declare_lint_pass!(NeedlessRawIdentifier => [NEEDLESS_RAW_IDENTIFIER]);

impl EarlyLintPass for NeedlessRawIdentifier {
    fn check_crate(&mut self, cx: &EarlyContext<'_>, _: &Crate) {
        let edition = cx.sess().edition();
        let source_map = cx.sess().source_map();
        let mut needless: Vec<(Span, String)> = cx
            .sess()
            .psess
            .raw_identifier_spans
            .iter()
            .filter(|span| {
                !span.from_expansion()
                    && matches!(
                        source_map.lookup_source_file(span.lo()).name,
                        FileName::Real(_)
                    )
            })
            .filter_map(|span| {
                let text = source_map.span_to_snippet(span).ok()?;
                let name = text
                    .strip_prefix('\'')
                    .unwrap_or(&text)
                    .strip_prefix("r#")?;
                let symbol = Symbol::intern(name);
                let keyword = ALL_EDITIONS
                    .iter()
                    .filter(|later| **later >= edition)
                    .any(|later| symbol.is_reserved(|| *later));
                (!keyword).then_some((span, text))
            })
            .collect();
        needless.sort_by_key(|(span, _)| (span.lo(), span.hi()));
        needless.dedup_by_key(|(span, _)| *span);
        for (span, text) in needless {
            let plain = text.replacen("r#", "", 1);
            span_lint_from_command_line(
                cx,
                NEEDLESS_RAW_IDENTIFIER,
                span,
                &format!("raw identifier `{text}`, whose name isn't a keyword"),
                &[&format!(
                    "write `{plain}`: `r#` is only for names the edition reserves, and lints that \
                     compare the code with its source text skip a needless one"
                )],
            );
        }
    }
}

#[test]
fn ui() {
    crate::ui_harness::ui_test(NEEDLESS_RAW_IDENTIFIER);
}
