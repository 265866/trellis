//! Whether a change to a Rust source file only adds, removes or moves lint expectations.

use std::ops::Range;

use proc_macro2::{Delimiter, Group, Spacing, TokenStream, TokenTree};

/// Whether `after` differs from `before` only in lint expectation attributes and whitespace, with
/// at least one expectation added, removed, changed or moved. A lint expectation is an outer
/// `#[expect(<lint>, ..., reason = "...")]`, the same `expect` inside
/// `#[cfg_attr(dylint_lib = "trellis_lints", ...)]`, or an `expect` of `dead_code` alone inside
/// `#[cfg_attr(not(test), ...)]`, on one line or several. A missing side, a
/// side that doesn't lex, and any other change, comments included, make it false.
pub(crate) fn only_expectations_changed(before: Option<&str>, after: Option<&str>) -> bool {
    let (Some(before), Some(after)) = (before.and_then(Stripped::of), after.and_then(Stripped::of))
    else {
        return false;
    };
    before.tokens == after.tokens
        && before.residue == after.residue
        && before.expectations != after.expectations
}

/// One token of a Rust source, for comparing two versions of it.
#[derive(Debug, PartialEq, Eq)]
enum Token {
    /// The start of a group with this delimiter.
    Open(Delimiter),
    /// The end of a group.
    Close,
    /// An identifier or keyword.
    Ident(String),
    /// A punctuation character.
    Punct {
        /// The character.
        character: char,
        /// Whether the next punctuation character joins it, as in `::`.
        joint: bool,
    },
    /// A literal, as written.
    Literal(String),
}

/// A lint expectation taken out of a Rust source.
#[derive(Debug, PartialEq, Eq)]
struct Expectation {
    /// The index in the remaining tokens where it was, so moving it is a change.
    position: usize,
    /// The attribute's contents.
    contents: String,
}

/// A Rust source without its lint expectations.
#[derive(Debug, Default)]
struct Stripped {
    /// The tokens outside the expectations.
    tokens: Vec<Token>,
    /// The expectations, in source order.
    expectations: Vec<Expectation>,
    /// The source text outside the expectations without whitespace, which still holds the
    /// comments that lexing drops.
    residue: String,
}

impl Stripped {
    /// The source `text` without its lint expectations, if it lexes.
    fn of(text: &str) -> Option<Self> {
        use std::str::FromStr as _;

        let stream = TokenStream::from_str(text).ok()?;
        let mut stripped = Self::default();
        let mut ranges = Vec::new();
        stripped.strip(stream, Context::Code, &mut ranges);
        let mut kept = String::with_capacity(text.len());
        let mut position = 0;
        for range in ranges {
            kept.push_str(text.get(position..range.start)?);
            position = range.end;
        }
        kept.push_str(text.get(position..)?);
        stripped.residue = kept
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect();
        Some(stripped)
    }

    /// Adds the tokens of `stream` outside its lint expectations to `self`, and the byte range of
    /// each expectation to `ranges`, in source order. Only a `stream` in `Context::Code` has
    /// expectations; the tokens of a macro's arguments or an attribute's contents are kept whole.
    fn strip(&mut self, stream: TokenStream, context: Context, ranges: &mut Vec<Range<usize>>) {
        let trees: Vec<TokenTree> = stream.into_iter().collect();
        let mut index = 0;
        while let Some(tree) = trees.get(index) {
            if context == Context::Code
                && let Some(TokenTree::Group(attribute)) = trees.get(index + 1)
                && is_expectation(tree, attribute)
            {
                ranges.push(tree.span().byte_range().start..attribute.span().byte_range().end);
                self.expectations.push(Expectation {
                    position: self.tokens.len(),
                    contents: attribute.stream().to_string(),
                });
                index += 2;
                continue;
            }
            match tree {
                TokenTree::Group(group) => {
                    let inner = if context == Context::Code && is_code_group(&trees[..index]) {
                        Context::Code
                    } else {
                        Context::Opaque
                    };
                    self.tokens.push(Token::Open(group.delimiter()));
                    self.strip(group.stream(), inner, ranges);
                    self.tokens.push(Token::Close);
                }
                TokenTree::Ident(ident) => self.tokens.push(Token::Ident(ident.to_string())),
                TokenTree::Punct(punct) => self.tokens.push(Token::Punct {
                    character: punct.as_char(),
                    joint: punct.spacing() == Spacing::Joint,
                }),
                TokenTree::Literal(literal) => {
                    self.tokens.push(Token::Literal(literal.to_string()));
                }
            }
            index += 1;
        }
    }
}

/// Where a token stream sits, which decides whether a `#[...]` in it is an attribute.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Context {
    /// Rust code, where a `#[...]` is an outer attribute on the item, field, variant, statement,
    /// expression or parameter after it.
    Code,
    /// A macro's arguments or body, or an attribute's contents, whose tokens mean whatever the
    /// macro or attribute makes of them.
    Opaque,
}

/// Whether a group in code after the tokens `before` holds code too, rather than a macro's
/// arguments (`name!(...)`, `path::name![...]`, `name! {...}`), a macro definition's rules or body
/// (`macro_rules! name {...}`, `macro name(...) {...}`), or an attribute's contents (`#[...]`,
/// `#![...]`). A keyword before `!`, as in `if !(...)`, also reads as a macro, which only keeps
/// more tokens whole.
fn is_code_group(before: &[TokenTree]) -> bool {
    let is_punct = |tree: &TokenTree, character| matches!(tree, TokenTree::Punct(punct) if punct.as_char() == character);
    let is_word =
        |tree: &TokenTree, word: &str| matches!(tree, TokenTree::Ident(ident) if ident == word);
    match before {
        [.., last] if is_punct(last, '#') => false,
        [.., pound, bang] if is_punct(pound, '#') && is_punct(bang, '!') => false,
        [.., TokenTree::Ident(_), bang] if is_punct(bang, '!') => false,
        [.., rules, bang, TokenTree::Ident(_)]
            if is_word(rules, "macro_rules") && is_punct(bang, '!') =>
        {
            false
        }
        [.., keyword, TokenTree::Ident(_)] if is_word(keyword, "macro") => false,
        [.., keyword, TokenTree::Ident(_), TokenTree::Group(_)] if is_word(keyword, "macro") => {
            false
        }
        _ => true,
    }
}

/// Whether `pound` followed by `attribute` is an outer lint expectation: `#[expect(...)]`,
/// `#[cfg_attr(dylint_lib = "trellis_lints", expect(...))]`, or
/// `#[cfg_attr(not(test), expect(dead_code, ...))]`, whose `expect` names one or more lints and
/// then a reason.
fn is_expectation(pound: &TokenTree, attribute: &Group) -> bool {
    let is_pound = matches!(pound, TokenTree::Punct(punct) if punct.as_char() == '#');
    if !is_pound || attribute.delimiter() != Delimiter::Bracket {
        return false;
    }
    let contents: Vec<TokenTree> = attribute.stream().into_iter().collect();
    match contents.as_slice() {
        [TokenTree::Ident(name), TokenTree::Group(arguments)] if name == "expect" => {
            expected_lints(arguments).is_some()
        }
        [TokenTree::Ident(name), TokenTree::Group(cfg_attr)]
            if name == "cfg_attr" && cfg_attr.delimiter() == Delimiter::Parenthesis =>
        {
            let inner: Vec<TokenTree> = cfg_attr.stream().into_iter().collect();
            let inner = match inner.as_slice() {
                [rest @ .., TokenTree::Punct(comma)] if comma.as_char() == ',' => rest,
                all => all,
            };
            match inner {
                [
                    TokenTree::Ident(key),
                    TokenTree::Punct(equals),
                    TokenTree::Literal(value),
                    TokenTree::Punct(comma),
                    TokenTree::Ident(expect),
                    TokenTree::Group(arguments),
                ] => {
                    key == "dylint_lib"
                        && equals.as_char() == '='
                        && value.to_string() == "\"trellis_lints\""
                        && comma.as_char() == ','
                        && expect == "expect"
                        && expected_lints(arguments).is_some()
                }
                [
                    TokenTree::Ident(not),
                    TokenTree::Group(predicate),
                    TokenTree::Punct(comma),
                    TokenTree::Ident(expect),
                    TokenTree::Group(arguments),
                ] => {
                    not == "not"
                        && predicate.delimiter() == Delimiter::Parenthesis
                        && predicate.stream().to_string() == "test"
                        && comma.as_char() == ','
                        && expect == "expect"
                        && expected_lints(arguments)
                            .is_some_and(|lints| lints.iter().all(|lint| lint == "dead_code"))
                }
                _ => false,
            }
        }
        _ => false,
    }
}

/// The lints that `arguments` names, each as written without whitespace, if it is the
/// parenthesized argument list of an `expect`: one or more lint paths, then `reason = "..."`,
/// with an optional trailing comma.
fn expected_lints(arguments: &Group) -> Option<Vec<String>> {
    if arguments.delimiter() != Delimiter::Parenthesis {
        return None;
    }
    let trees: Vec<TokenTree> = arguments.stream().into_iter().collect();
    let mut parts: Vec<&[TokenTree]> = trees
        .split(|tree| matches!(tree, TokenTree::Punct(punct) if punct.as_char() == ','))
        .collect();
    if parts.last().is_some_and(|last| last.is_empty()) {
        parts.pop();
    }
    let (reason, lints) = parts.split_last()?;
    let is_reason = matches!(
        reason,
        [TokenTree::Ident(key), TokenTree::Punct(equals), TokenTree::Literal(text)]
            if key == "reason" && equals.as_char() == '=' && is_string(&text.to_string())
    );
    (is_reason && !lints.is_empty() && lints.iter().all(|lint| is_path(lint))).then(|| {
        lints
            .iter()
            .map(|lint| lint.iter().map(ToString::to_string).collect())
            .collect()
    })
}

/// Whether `literal`, as written, is a string literal.
fn is_string(literal: &str) -> bool {
    literal.starts_with('"') || literal.starts_with("r\"") || literal.starts_with("r#")
}

/// Whether `trees` is a path such as `dead_code` or `clippy::too_many_lines`.
fn is_path(trees: &[TokenTree]) -> bool {
    let Some((TokenTree::Ident(_), rest)) = trees.split_first() else {
        return false;
    };
    rest.chunks(3).all(|segment| {
        matches!(
            segment,
            [TokenTree::Punct(first), TokenTree::Punct(second), TokenTree::Ident(_)]
                if first.as_char() == ':' && first.spacing() == Spacing::Joint
                    && second.as_char() == ':'
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A source file with one function.
    const SOURCE: &str = "//! The app.\n\npub fn run(a: u8) -> u8 {\n    a + 1\n}\n";

    /// `SOURCE` with `attribute` on its function.
    fn with(attribute: &str) -> String {
        SOURCE.replace("pub fn run", &format!("{attribute}\npub fn run"))
    }

    #[test]
    fn adding_removing_or_moving_an_expectation_is_one() {
        let expect = "#[expect(clippy::cast_possible_truncation, reason = \"the input fits\")]";
        let multiline = "#[expect(\n    clippy::too_many_lines,\n    dead_code,\n    \
                         reason = \"the parser is one table of cases\",\n)]";
        let cfg_attr = "#[cfg_attr(\n    dylint_lib = \"trellis_lints\",\n    \
                        expect(shared_ownership, reason = \"the cache is shared by threads\")\n)]";
        for (before, after, why) in [
            (SOURCE.to_owned(), with(expect), "adding an expect"),
            (with(expect), SOURCE.to_owned(), "removing an expect"),
            (SOURCE.to_owned(), with(multiline), "a multi-line expect"),
            (SOURCE.to_owned(), with(cfg_attr), "the cfg_attr form"),
            (
                SOURCE.to_owned(),
                with(
                    "#[cfg_attr(not(test), expect(dead_code, reason = \"the next change calls it\"))]",
                ),
                "the cfg_attr form for code only tests use",
            ),
            (
                with(expect),
                with(&expect.replace("fits", "always fits")),
                "a changed reason",
            ),
            (
                with(expect),
                SOURCE.replace("    a + 1", &format!("    {expect}\n    a + 1")),
                "a moved expect",
            ),
            (
                SOURCE.to_owned(),
                with(expect).replace("    a + 1", "        a   +   1"),
                "an expect beside a whitespace change",
            ),
        ] {
            assert!(
                only_expectations_changed(Some(&before), Some(&after)),
                "{why} changes only expectations"
            );
        }
    }

    #[test]
    fn any_other_change_is_not() {
        let expect = "#[expect(dead_code, reason = \"kept for the next release\")]";
        for (before, after, why) in [
            (
                SOURCE.to_owned(),
                with(expect).replace("a + 1", "a + 2"),
                "an expect beside a code change",
            ),
            (
                SOURCE.to_owned(),
                with(expect).replace("//! The app.", "//! The app, changed."),
                "an expect beside a doc change",
            ),
            (
                SOURCE.to_owned(),
                with(expect).replace("a + 1", "a + 1 // SAFETY: fine"),
                "an expect beside a comment",
            ),
            (SOURCE.to_owned(), with("#[allow(dead_code)]"), "an allow"),
            (
                SOURCE.to_owned(),
                with("#[expect(dead_code)]"),
                "an expect without a reason",
            ),
            (
                SOURCE.to_owned(),
                with("#[cfg_attr(test, expect(dead_code, reason = \"used only in builds\"))]"),
                "a cfg_attr on another condition",
            ),
            (
                SOURCE.to_owned(),
                with(
                    "#[cfg_attr(not(test), expect(unused, reason = \"the next change calls it\"))]",
                ),
                "a cfg_attr on not(test) of a lint other than dead_code",
            ),
            (
                SOURCE.to_owned(),
                with(
                    "#[cfg_attr(not(test), expect(dead_code, unused, reason = \"the next change calls it\"))]",
                ),
                "a cfg_attr on not(test) of dead_code beside another lint",
            ),
            (
                SOURCE.to_owned(),
                with(
                    "#[cfg_attr(not(all(test)), expect(dead_code, reason = \"the next change calls it\"))]",
                ),
                "a cfg_attr on a predicate other than exactly not(test)",
            ),
            (
                SOURCE.to_owned(),
                SOURCE.replace(
                    "//! The app.",
                    "#![expect(dead_code, reason = \"kept for the next release\")]",
                ),
                "an inner expect",
            ),
            (
                SOURCE.to_owned(),
                SOURCE.replace("    a + 1", "        a + 1"),
                "whitespace alone",
            ),
        ] {
            assert!(
                !only_expectations_changed(Some(&before), Some(&after)),
                "{why} changes more than expectations"
            );
        }
        assert!(
            !only_expectations_changed(None, Some(&with(expect))),
            "a new file changes more"
        );
        assert!(
            !only_expectations_changed(Some(SOURCE), Some(&format!("{} \"", with(expect)))),
            "a file that doesn't lex changes more"
        );
    }

    /// A source file with expectation sites in several positions, and macros whose arguments hold
    /// what reads as expectations. Each `/*name*/` marks where a test puts an attribute.
    const POSITIONS: &str = r#"
/*item*/ pub struct Point {
    /*field*/ pub x: u8,
}

pub enum Shape {
    /*variant*/ Dot,
}

pub fn run(/*parameter*/ a: u8) -> u8 {
    /*statement*/ let b = a + 1;
    let c = (/*expression*/ || b)();
    if !(c > 2) { c } else { b }
}

pub const NAME: &str = stringify!(#[expect(dead_code, reason = "old")] fn f() {});

pub fn list() -> Vec<u8> {
    vec![/*vec*/ 1]
}

macro_rules! make {
    () => {
        /*rules*/ pub fn made() {}
    };
}

make! { /*braces*/ fn braced() {} }
"#;

    /// `POSITIONS` with `attribute` at `/*site*/`.
    fn at(site: &str, attribute: &str) -> String {
        POSITIONS.replace(&format!("/*{site}*/"), &format!("/*{site}*/ {attribute}"))
    }

    #[test]
    fn expectations_count_only_in_attribute_position() {
        let expect = "#[expect(dead_code, reason = \"kept for the next release\")]";
        let multiline = "#[expect(\n    clippy::too_many_lines,\n    \
                         reason = \"the parser is one table of cases\",\n)]";
        let cfg_attr = "#[cfg_attr(\n    dylint_lib = \"trellis_lints\",\n    \
                        expect(shared_ownership, reason = \"the cache is shared by threads\")\n)]";
        for site in [
            "item",
            "field",
            "variant",
            "parameter",
            "statement",
            "expression",
        ] {
            for attribute in [expect, multiline, cfg_attr] {
                assert!(
                    only_expectations_changed(Some(POSITIONS), Some(&at(site, attribute))),
                    "an expectation on the {site} changes only expectations: {attribute}"
                );
            }
        }
        for (after, why) in [
            (
                POSITIONS.replace("reason = \"old\"", "reason = \"new\""),
                "a changed literal inside stringify!",
            ),
            (
                POSITIONS.replace("#[expect(dead_code, reason = \"old\")] ", ""),
                "a removed expect inside stringify!",
            ),
            (at("vec", expect), "an expect inside vec![...]"),
            (
                at("braces", expect),
                "an expect inside a macro call with braces",
            ),
            (
                at("rules", expect),
                "an expect inside a macro_rules! definition",
            ),
            (
                at("rules", cfg_attr),
                "a cfg_attr expect inside a macro_rules! definition",
            ),
        ] {
            assert!(
                !only_expectations_changed(Some(POSITIONS), Some(&after)),
                "{why} changes code"
            );
        }
    }
}
