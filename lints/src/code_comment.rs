use std::ops::Range;

use clippy_utils::diagnostics::span_lint_and_help;
use rustc_ast::Crate;
use rustc_lexer::TokenKind;
use rustc_lint::{EarlyContext, EarlyLintPass};
use rustc_lint_defs::{declare_lint, declare_lint_pass};
use rustc_span::Span;

use crate::local_source::{LocalSource, RawToken, SourceText};

declare_lint! {
    /// Checks for `//` and `/* */` comments in Rust code, other than `// SAFETY:` comments on
    /// unsafe code and doc comments.
    ///
    /// trellis policy, stricter than the Google Rust Style Guide: code comments drift from the
    /// code, and agents reach for them to justify a workaround instead of fixing it. Say why in a
    /// place that is checked or reviewed as part of the code: a doc comment on the item, a test
    /// name, an assertion message, or the `reason` of an `#[expect(...)]`.
    ///
    /// `// SAFETY:` comments stay, because `clippy::undocumented_unsafe_blocks` requires them. A
    /// `SAFETY:` comment may run on over the `//` lines right below it, and counts only where
    /// unsafe code follows it: on its own line, or on the first line of code after it, past any
    /// attributes and doc comments. Anywhere else it is flagged like any other comment.
    ///
    /// It reads the crate root and the file of every `mod foo;` declared in them, including
    /// modules that a `cfg` leaves out of the build, since their declarations are read from the
    /// source text. Files that `include_str!` pulls in aren't Rust and aren't read. `include!`
    /// would bring in Rust that isn't a module, so `disabled_code` rejects it.
    ///
    /// This lint can't be silenced: `suppression_policy` rejects an `expect` that names it, and
    /// it reads whole files rather than items, so an attribute on an item couldn't reach it
    /// anyway. Rewrite or delete the comment.
    ///
    /// ```rust,ignore
    /// // Retry once: the mirror drops the first request after a deploy.
    /// let response = send(request).or_else(|_| send(request))?;
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// let response = send_with_one_retry(request)?;
    ///
    /// /// Sends `request`, and once more if it fails: the mirror drops the first request after a
    /// /// deploy.
    /// fn send_with_one_retry(request: &Request) -> Result<Response, Error> { ... }
    /// ```
    pub CODE_COMMENT,
    Warn,
    "a `//` or `/* */` comment other than `// SAFETY:` or a doc comment"
}

declare_lint_pass!(CodeComment => [CODE_COMMENT]);

impl EarlyLintPass for CodeComment {
    fn check_crate(&mut self, cx: &EarlyContext<'_>, krate: &Crate) {
        for file in &LocalSource::get(cx, krate).files {
            lint_file(cx, file);
        }
    }
}

fn lint_file(cx: &EarlyContext<'_>, file: &SourceText) {
    let source = file.source.as_str();
    let tokens = &file.tokens;
    let file_span = |range: Range<usize>| file.span(range);
    let mut index = 0;
    while index < tokens.len() {
        let token = &tokens[index];
        if !is_code_comment(token.kind) {
            index += 1;
            continue;
        }
        if !is_safety_comment(&source[token.range.clone()]) {
            lint(cx, file_span(token.range.clone()), false);
            index += 1;
            continue;
        }
        let last = safety_comment_end(source, tokens, index);
        if !covers_unsafe_code(source, tokens, index, last) {
            lint(
                cx,
                file_span(token.range.start..tokens[last].range.end),
                true,
            );
        }
        index = last + 1;
    }
}

fn lint(cx: &EarlyContext<'_>, span: Span, misplaced_safety: bool) {
    let (message, help) = if misplaced_safety {
        (
            "`SAFETY:` comment with no unsafe code after it",
            "put it right above, or at the end of, the line with the `unsafe` code it explains, \
             or delete it",
        )
    } else {
        (
            "code comment",
            "move the explanation into a doc comment, a name, an assertion message or an \
             `#[expect(..., reason = \"...\")]`, or delete it",
        )
    };
    span_lint_and_help(cx, CODE_COMMENT, span, message, None, help);
}

fn is_code_comment(kind: TokenKind) -> bool {
    match kind {
        TokenKind::LineComment { doc_style } | TokenKind::BlockComment { doc_style, .. } => {
            doc_style.is_none()
        }
        _ => false,
    }
}

/// Whether `comment`, including its `//` or `/*`, is a `SAFETY:` comment.
fn is_safety_comment(comment: &str) -> bool {
    comment
        .trim_start_matches('/')
        .trim_start_matches('*')
        .trim_start()
        .starts_with("SAFETY:")
}

/// The index of the last token of the `SAFETY:` comment at `first`: a line comment on a line of
/// its own runs on over the `//` comments on the lines right below it.
fn safety_comment_end(source: &str, tokens: &[RawToken], first: usize) -> usize {
    let mut last = first;
    let start = tokens[first].range.start;
    let line_start = source[..start].rfind('\n').map_or(0, |newline| newline + 1);
    if !matches!(tokens[first].kind, TokenKind::LineComment { .. })
        || !source[line_start..start].trim().is_empty()
    {
        return last;
    }
    while let [gap, next, ..] = &tokens[last + 1..]
        && gap.kind == TokenKind::Whitespace
        && source[gap.range.clone()].matches('\n').count() == 1
        && next.kind == (TokenKind::LineComment { doc_style: None })
    {
        last += 2;
    }
    last
}

/// Whether the comment from token `first` to token `last` explains unsafe code: the line it
/// starts on has an `unsafe`, or the first line of code after it, past attributes and doc
/// comments, does.
fn covers_unsafe_code(source: &str, tokens: &[RawToken], first: usize, last: usize) -> bool {
    if line_has_unsafe(source, tokens, tokens[first].range.start) {
        return true;
    }
    let mut index = last + 1;
    while let Some(token) = tokens.get(index) {
        match token.kind {
            TokenKind::Whitespace
            | TokenKind::LineComment { doc_style: Some(_) }
            | TokenKind::BlockComment {
                doc_style: Some(_), ..
            } => index += 1,
            TokenKind::Pound => index = attribute_end(tokens, index),
            _ => return line_has_unsafe(source, tokens, token.range.start),
        }
    }
    false
}

/// The index just past the attribute whose `#` is token `pound`, or past the `#` alone when no
/// `[` follows it.
fn attribute_end(tokens: &[RawToken], pound: usize) -> usize {
    let mut index = pound + 1;
    while tokens
        .get(index)
        .is_some_and(|token| matches!(token.kind, TokenKind::Bang | TokenKind::Whitespace))
    {
        index += 1;
    }
    if tokens
        .get(index)
        .is_none_or(|token| token.kind != TokenKind::OpenBracket)
    {
        return pound + 1;
    }
    let mut depth = 0_usize;
    while let Some(token) = tokens.get(index) {
        index += 1;
        match token.kind {
            TokenKind::OpenBracket => depth += 1,
            TokenKind::CloseBracket => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            _ => {}
        }
    }
    index
}

/// Whether the line holding byte `at` of `source` has an `unsafe` keyword.
fn line_has_unsafe(source: &str, tokens: &[RawToken], at: usize) -> bool {
    let line_start = source[..at].rfind('\n').map_or(0, |newline| newline + 1);
    let line_end = source[at..]
        .find('\n')
        .map_or(source.len(), |newline| at + newline);
    let first = tokens.partition_point(|token| token.range.start < line_start);
    tokens[first..]
        .iter()
        .take_while(|token| token.range.start < line_end)
        .any(|token| token.kind == TokenKind::Ident && &source[token.range.clone()] == "unsafe")
}

#[test]
fn ui() {
    crate::ui_harness::ui_test(CODE_COMMENT);
}
