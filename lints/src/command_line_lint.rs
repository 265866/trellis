//! Reporting for the lints that guard the lint levels themselves, which source code must not be
//! able to turn off.

use clippy_utils::diagnostics::span_lint_and_then;
use rustc_lint::{EarlyContext, Level, LintContext};
use rustc_lint_defs::Lint;
use rustc_middle::lint::LintLevelSource;
use rustc_span::Span;

/// Reports `lint` at `span` with a help line for each of `helps`, but only the command line may
/// turn it off, and only by naming it.
///
/// These lints report from `check_crate`, where the crate's inner attributes set the level, so a
/// `#![allow(lint)]`, `#![expect(lint)]` or `#![allow(warnings)]` would otherwise silence the very
/// lint that flags it, and `-A warnings` or `--cap-lints` in `RUSTFLAGS` would too. In those cases
/// this reports a hard error instead. `-A lint` on the command line still turns the lint off,
/// which is how the UI tests of the other lints run without it.
pub(crate) fn span_lint_from_command_line(
    cx: &EarlyContext<'_>,
    lint: &'static Lint,
    span: Span,
    message: &str,
    helps: &[&str],
) {
    let level = cx.get_lint_level_spec(lint);
    let name = lint.name_lower();
    let error = || {
        let mut error = cx.sess().dcx().struct_span_err(span, message.to_owned());
        for &help in helps {
            error.help(help.to_owned());
        }
        error
    };
    let warn = || {
        span_lint_and_then(cx, lint, span, message.to_owned(), |diag| {
            for &help in helps {
                diag.help(help.to_owned());
            }
        });
    };
    match level.src {
        LintLevelSource::Node {
            span: attribute, ..
        } => {
            error()
                .with_span_note(
                    attribute,
                    format!(
                        "this attribute sets the level of `{name}`, which only the command line \
                         may set, so the lint reports an error"
                    ),
                )
                .emit();
        }
        LintLevelSource::CommandLine(flag, requested)
            if flag.to_string() == name && requested == level.level() =>
        {
            warn();
        }
        LintLevelSource::Default | LintLevelSource::CommandLine(..)
            if matches!(level.level(), Level::Allow | Level::Expect) =>
        {
            error()
                .with_note(format!(
                    "`-A warnings`, `--cap-lints` or a lint group on the command line turned off \
                     `{name}`, which only `-A {name}` may turn off, so the lint reports an error"
                ))
                .emit();
        }
        LintLevelSource::Default | LintLevelSource::CommandLine(..) => {
            warn();
        }
    }
}
