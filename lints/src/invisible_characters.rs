use clippy_utils::diagnostics::span_lint_and_help;
use rustc_ast::token::LitKind;
use rustc_ast::{Crate, Expr, ExprKind};
use rustc_lexer::{LiteralKind as LexLiteralKind, TokenKind};
use rustc_lint::{EarlyContext, EarlyLintPass};
use rustc_lint_defs::{declare_lint, impl_lint_pass};
use rustc_span::Span;

use crate::local_source::LocalSource;

declare_lint! {
    /// Checks for string, character and C-string literals that contain an invisible,
    /// line-separating or bidirectional-control character written as itself rather than as a
    /// `\u{..}` escape. It checks every such literal in the source files: in expressions and
    /// patterns, format strings, attributes such as `reason = "..."`, the arguments of macros
    /// such as `concat!`, and macro definitions.
    ///
    /// The characters include zero-width spaces and joiners, bidirectional controls, the line
    /// and paragraph separators U+2028 and U+2029, variation selectors (U+FE00 to U+FE0F, U+180B
    /// to U+180F and U+E0100 to U+E01EF), and the tag characters U+E0000 to U+E007F.
    ///
    /// The Google Rust Style Guide, "Use the appropriate formatting tool" in "Code formatting and
    /// aesthetics", asks for "usage of escapes for invisible characters in literals". A reader
    /// can't see these characters, and a bidirectional control can make the code read differently
    /// from how it compiles.
    ///
    /// Emoji are left alone. A variation selector, U+FE0E or U+FE0F, right after an emoji or in a
    /// keycap (`7`, U+FE0F, U+20E3), and a zero-width joiner, U+200D, between two emoji (the heart
    /// on fire is U+2764, U+FE0F, U+200D, U+1F525), are part of how the emoji is written, as are
    /// the tag characters after a black flag, U+1F3F4, that spell a subdivision flag. "Emoji"
    /// here means the blocks emoji come from, such as U+1F000 to U+1FAFF and U+2600 to U+27BF,
    /// rather than the exact Unicode emoji list. Those characters are still flagged anywhere else.
    ///
    /// ```rust,ignore
    /// // Between "check" and "out" is a zero-width joiner, U+200D, written as itself.
    /// let label = "checkout";
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// let label = "check\u{200D}out";
    /// ```
    pub INVISIBLE_CHARACTERS,
    Warn,
    "an invisible character written literally in a string or char literal"
}

/// Lints literals, and collects the spans of those the AST shows, so that
/// [`EarlyLintPass::check_crate_post`] reads the rest from the source text.
#[derive(Default)]
pub struct InvisibleCharacters {
    /// The literals linted through the AST.
    seen: Vec<Span>,
}

impl_lint_pass!(InvisibleCharacters => [INVISIBLE_CHARACTERS]);

impl EarlyLintPass for InvisibleCharacters {
    /// Lints the string, C string or character literal `expr` is, or the string of a format macro,
    /// which parses its string itself, so the string isn't an `ExprKind::Lit`. Byte literals hold
    /// only ASCII, so they are left alone.
    fn check_expr(&mut self, cx: &EarlyContext<'_>, expr: &Expr) {
        let (kind, text, span) = match &expr.kind {
            ExprKind::Lit(lit) => (lit.kind, lit.symbol, expr.span),
            ExprKind::FormatArgs(args) if args.is_source_literal => {
                (args.uncooked_fmt_str.0, args.uncooked_fmt_str.1, args.span)
            }
            _ => return,
        };
        if span.from_expansion() {
            return;
        }
        let raw = match kind {
            LitKind::Str | LitKind::Char | LitKind::CStr => false,
            LitKind::StrRaw(_) | LitKind::CStrRaw(_) => true,
            _ => return,
        };
        self.seen.push(span);
        lint_invisible(cx, text.as_str(), span, raw);
    }

    /// Lints the literals no expression holds, such as those in attributes, like
    /// `reason = "..."`, in the arguments of macros like `concat!`, and in macro definitions.
    fn check_crate_post(&mut self, cx: &EarlyContext<'_>, krate: &Crate) {
        for file in &LocalSource::get(cx, krate).files {
            for token in &file.tokens {
                let TokenKind::Literal { kind, .. } = token.kind else {
                    continue;
                };
                let raw = match kind {
                    LexLiteralKind::Str { .. }
                    | LexLiteralKind::Char { .. }
                    | LexLiteralKind::CStr { .. } => false,
                    LexLiteralKind::RawStr { .. } | LexLiteralKind::RawCStr { .. } => true,
                    _ => continue,
                };
                let span = file.span(token.range.clone());
                if self.seen.contains(&span) {
                    continue;
                }
                lint_invisible(cx, &file.source[token.range.clone()], span, raw);
            }
        }
    }
}

/// Lints `literal`, whose source text is `text`, if it holds an invisible character.
fn lint_invisible(cx: &EarlyContext<'_>, text: &str, literal: Span, raw: bool) {
    let mut found: Vec<char> = Vec::new();
    let characters: Vec<char> = text.chars().collect();
    for (index, &character) in characters.iter().enumerate() {
        if !is_invisible(character) || is_part_of_emoji(&characters, index) {
            continue;
        }
        if !found.contains(&character) {
            found.push(character);
        }
    }
    if found.is_empty() {
        return;
    }
    let escapes: Vec<String> = found
        .iter()
        .map(|&character| format!("`\\u{{{:04X}}}`", u32::from(character)))
        .collect();
    let (pronoun, escapes) = match escapes.as_slice() {
        [escape] => ("it", escape.clone()),
        _ => ("them", escapes.join(", ")),
    };
    let unraw = if raw {
        "use a literal that isn't raw, and "
    } else {
        ""
    };
    span_lint_and_help(
        cx,
        INVISIBLE_CHARACTERS,
        literal,
        "invisible character written literally",
        None,
        format!("{unraw}write {pronoun} as {escapes}"),
    );
}

/// Whether `character` renders as nothing or reorders the text around it.
fn is_invisible(character: char) -> bool {
    matches!(
        character,
        '\u{00AD}'
            | '\u{034F}'
            | '\u{061C}'
            | '\u{115F}'
            | '\u{1160}'
            | '\u{17B4}'
            | '\u{17B5}'
            | '\u{180B}'..='\u{180F}'
            | '\u{200B}'..='\u{200F}'
            | '\u{2028}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{206F}'
            | '\u{3164}'
            | '\u{FE00}'..='\u{FE0F}'
            | '\u{FEFF}'
            | '\u{FFA0}'
            | '\u{E0000}'..='\u{E007F}'
            | '\u{E0100}'..='\u{E01EF}'
    )
}

/// Whether the invisible character at `index` of `characters` is part of an emoji: a variation
/// selector right after an emoji or in a keycap sequence, a zero-width joiner between two emoji,
/// or a tag character in a subdivision flag, which follows a black flag.
fn is_part_of_emoji(characters: &[char], index: usize) -> bool {
    let Some(before) = index.checked_sub(1).map(|before| characters[before]) else {
        return false;
    };
    let after = characters.get(index + 1).copied();
    match characters[index] {
        '\u{FE0E}' | '\u{FE0F}' => {
            is_emoji(before) || matches!(before, '0'..='9' | '#' | '*') && after == Some('\u{20E3}')
        }
        '\u{200D}' => {
            let joined = characters[..index]
                .iter()
                .rev()
                .find(|&&character| !matches!(character, '\u{FE0E}' | '\u{FE0F}'));
            joined.is_some_and(|&joined| is_emoji(joined)) && after.is_some_and(is_emoji)
        }
        '\u{E0020}'..='\u{E007F}' => {
            let flag = characters[..index]
                .iter()
                .rev()
                .find(|&&character| !matches!(character, '\u{E0020}'..='\u{E007F}'));
            flag == Some(&'\u{1F3F4}')
        }
        _ => false,
    }
}

/// Whether `character` is in a block that emoji come from. Broader than Unicode's emoji list,
/// which would need a table: the lint only uses it to let joiners and selectors through.
fn is_emoji(character: char) -> bool {
    matches!(
        character,
        '\u{00A9}'
            | '\u{00AE}'
            | '\u{203C}'
            | '\u{2049}'
            | '\u{2122}'
            | '\u{2139}'
            | '\u{2194}'..='\u{21AA}'
            | '\u{231A}'..='\u{23FF}'
            | '\u{24C2}'
            | '\u{25AA}'..='\u{25FE}'
            | '\u{2600}'..='\u{27BF}'
            | '\u{2934}'
            | '\u{2935}'
            | '\u{2B05}'..='\u{2B55}'
            | '\u{3030}'
            | '\u{303D}'
            | '\u{3297}'
            | '\u{3299}'
            | '\u{1F000}'..='\u{1FAFF}'
    )
}

#[test]
fn ui() {
    crate::ui_harness::ui_test(INVISIBLE_CHARACTERS);
}
