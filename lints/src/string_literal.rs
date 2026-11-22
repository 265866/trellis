//! Reading the paths a macro parses from a string literal the caller wrote, such as serde's
//! `#[serde(default = "defaults::zero")]`. The macro gives each token it parses the string's span,
//! so the text at a path's span is the whole literal, escapes and all. The lints read the
//! string's value, not its text, and trust a path in it only when it is a plain one: identifiers
//! joined by `::` and nothing else, so no whitespace, comment, escape, raw identifier or turbofish
//! can make the text differ from the path the macro parses.
//!
//! The compiler interns every identifier in Unicode's composed form (NFC), so `Cafe` followed by a
//! combining accent names the type `Café`. Text compared with a name is normalized the same way
//! first, by [`normalized`], or a name written decomposed would never match the one it spells.

use std::borrow::Cow;

use rustc_lexer::{FrontmatterAllowed, LiteralKind, TokenKind};
use unicode_normalization::UnicodeNormalization;

/// A string literal in the source.
pub(crate) enum Literal {
    /// The literal's value, its escapes resolved.
    Value(String),
    /// A literal with an escape the lints can't read.
    Unreadable,
}

impl Literal {
    /// Whether a macro may have parsed the path whose segments are `names` from this string: the
    /// names are among its identifiers in order, which holds however a path is written, or are
    /// the string's identifiers followed by more, as the `flag::deserialize` serde makes of
    /// `#[serde(with = "flag")]`. A string with no identifier or integer, such as `"::"`, spells
    /// nothing, and thiserror's `"{0}"` spells only the field `0`. A string that can't be read may
    /// hold any path.
    pub(crate) fn may_spell(&self, names: &[&str]) -> bool {
        let Self::Value(value) = self else {
            return true;
        };
        let identifiers = identifiers(value);
        if identifiers.is_empty() {
            return false;
        }
        let mut remaining = identifiers.iter();
        names
            .iter()
            .all(|name| remaining.any(|identifier| identifier == name))
            || names.starts_with(&identifiers)
    }
}

/// The string literal `text`, the source at a span, if it is one.
pub(crate) fn literal(text: &str) -> Option<Literal> {
    let text = text.trim();
    let value = if let Some(raw) = text.strip_prefix('r') {
        let hashes = &raw[..raw.len() - raw.trim_start_matches('#').len()];
        raw[hashes.len()..]
            .strip_prefix('"')?
            .strip_suffix(hashes)
            .and_then(|body| body.strip_suffix('"'))
            .map(str::to_owned)
    } else {
        text.strip_prefix('"')?.strip_suffix('"').and_then(unescape)
    };
    Some(value.map_or(Literal::Unreadable, |value| {
        Literal::Value(normalized(&value).into_owned())
    }))
}

/// `text` in the composed form the compiler interns identifiers in.
pub(crate) fn normalized(text: &str) -> Cow<'_, str> {
    if unicode_normalization::is_nfc(text) {
        Cow::Borrowed(text)
    } else {
        Cow::Owned(text.nfc().collect())
    }
}

/// The identifiers of `value` read as Rust tokens, in order, raw ones without their `r#`, and its
/// unsuffixed integers, which name tuple fields.
fn identifiers(value: &str) -> Vec<&str> {
    let mut identifiers = Vec::new();
    let mut start = 0;
    for token in rustc_lexer::tokenize(value, FrontmatterAllowed::No) {
        let end = start + token.len as usize;
        match token.kind {
            TokenKind::Ident => identifiers.push(&value[start..end]),
            TokenKind::RawIdent => identifiers.push(&value[start + 2..end]),
            TokenKind::Literal {
                kind: LiteralKind::Int { .. },
                suffix_start,
            } if suffix_start == token.len => identifiers.push(&value[start..end]),
            _ => {}
        }
        start = end;
    }
    identifiers
}

/// The paths in `value`, read as a type, a bound or a path, each as its segments, if every path
/// in it is plain and nothing else in it is code a lint could read: only names joined by `::`,
/// lifetimes, `_`, integers, and the punctuation of types and bounds, such as `<`, `,`, `:`, `+`,
/// `&`, `[`, `;` and `->`. A qualified path such as `<T as Trait>::Assoc` is its type's paths,
/// then the trait's segments followed by the associated item's, `Trait::Assoc`, as the compiler
/// holds it. A comment, a raw identifier, whitespace inside a path, a leading `::`, a turbofish,
/// a block, a macro call, a string or a qualified self with no trait makes it `None`.
pub(crate) fn paths(value: &str) -> Option<Vec<Vec<&str>>> {
    let mut paths: Vec<Vec<&str>> = Vec::new();
    let mut path_ended_by_last_token: Option<usize> = None;
    let mut path_continued_by_colons: Option<usize> = None;
    let mut unclosed_angles: Vec<Angle> = Vec::new();
    let mut start = 0;
    let mut tokens = rustc_lexer::tokenize(value, FrontmatterAllowed::No).peekable();
    while let Some(token) = tokens.next() {
        let text = &value[start..start + token.len as usize];
        start += token.len as usize;
        let next_kind = tokens.peek().map(|next| next.kind);
        match token.kind {
            TokenKind::Ident if text == "_" && path_continued_by_colons.is_none() => {
                path_ended_by_last_token = None;
            }
            TokenKind::Ident
                if text == "as"
                    && path_continued_by_colons.is_none()
                    && unclosed_angles.last() == Some(&Angle::StartsPath) =>
            {
                unclosed_angles.pop();
                unclosed_angles.push(Angle::QualifiedSelf(None));
                path_ended_by_last_token = None;
            }
            TokenKind::Ident if is_identifier(text) => {
                let index = if let Some(index) = path_continued_by_colons.take() {
                    paths.get_mut(index)?.push(text);
                    index
                } else {
                    paths.push(vec![text]);
                    let index = paths.len() - 1;
                    if let Some(angle @ Angle::QualifiedSelf(None)) = unclosed_angles.last_mut() {
                        *angle = Angle::QualifiedSelf(Some(index));
                    }
                    index
                };
                path_ended_by_last_token = Some(index);
                continue;
            }
            TokenKind::Colon if next_kind == Some(TokenKind::Colon) => {
                tokens.next();
                start += 1;
                if path_continued_by_colons.is_some() {
                    return None;
                }
                path_continued_by_colons = Some(path_ended_by_last_token.take()?);
                continue;
            }
            TokenKind::Minus if next_kind == Some(TokenKind::Gt) => {
                tokens.next();
                start += 1;
                path_ended_by_last_token = None;
            }
            TokenKind::Lt => {
                unclosed_angles.push(if path_ended_by_last_token.is_none() {
                    Angle::StartsPath
                } else {
                    Angle::Arguments
                });
                path_ended_by_last_token = None;
            }
            TokenKind::Gt => {
                let angle = unclosed_angles.pop()?;
                path_ended_by_last_token = match angle {
                    Angle::QualifiedSelf(Some(trait_path)) => Some(trait_path),
                    Angle::QualifiedSelf(None) => return None,
                    Angle::StartsPath if next_kind == Some(TokenKind::Colon) => return None,
                    Angle::StartsPath | Angle::Arguments => None,
                };
            }
            TokenKind::Literal {
                kind: LiteralKind::Int { .. },
                suffix_start,
            } if suffix_start == token.len => path_ended_by_last_token = None,
            TokenKind::Lifetime {
                starts_with_number: false,
            }
            | TokenKind::Whitespace
            | TokenKind::Colon
            | TokenKind::Comma
            | TokenKind::Plus
            | TokenKind::OpenParen
            | TokenKind::CloseParen
            | TokenKind::OpenBracket
            | TokenKind::CloseBracket
            | TokenKind::And
            | TokenKind::Semi
            | TokenKind::Eq
            | TokenKind::Question
            | TokenKind::Star => path_ended_by_last_token = None,
            _ => return None,
        }
        if path_continued_by_colons.is_some() {
            return None;
        }
    }
    path_continued_by_colons.is_none().then_some(paths)
}

/// A `<` that [`paths`] has read and not yet closed.
#[derive(PartialEq)]
enum Angle {
    /// One that opens the generic arguments of a path, as in `Vec<u8>`.
    Arguments,
    /// One that starts a path, as the `<` of `<T as Trait>::Assoc` does, before any `as`.
    StartsPath,
    /// One that an `as` made a qualified self, with the trait's path once there is one.
    QualifiedSelf(Option<usize>),
}

/// Whether `text`, an identifier token, is a name a path may hold: not `_`. Any Unicode
/// identifier the lexer reads is one, as `Café` is.
fn is_identifier(text: &str) -> bool {
    text != "_" && rustc_lexer::is_ident(text)
}

/// The names the `{..}` placeholders of the format string `value` read: the argument each opens
/// with, such as `name` for `{name:?}` and `0` for `{0}`, and the width or precision its format
/// spec takes from an argument, such as `width` for `{:>width$}`. Leading zeros aren't part of a
/// name: neither the `0` flag right before a width, as in `{:0width$}`, nor those of an index, as
/// in `{01}` and `{:001$}`, which both read `1`. The `{{` that writes a brace opens none.
pub(crate) fn placeholders(value: &str) -> Vec<&str> {
    let mut names = Vec::new();
    let mut rest = value;
    while let Some(open) = rest.find('{') {
        rest = &rest[open + 1..];
        if let Some(after) = rest.strip_prefix('{') {
            rest = after;
            continue;
        }
        let end = rest.find('}').unwrap_or(rest.len());
        let (argument, spec) = rest[..end].split_once(':').unwrap_or((&rest[..end], ""));
        let argument_end = argument
            .find(|character: char| !rustc_lexer::is_id_continue(character))
            .unwrap_or(argument.len());
        let argument = &argument[..argument_end];
        names.push(if argument.bytes().all(|byte| byte.is_ascii_digit()) {
            without_leading_zeros(argument)
        } else {
            argument
        });
        for (dollar, _) in spec.match_indices('$') {
            let before = &spec[..dollar];
            let start = before
                .char_indices()
                .rev()
                .take_while(|&(_, character)| rustc_lexer::is_id_continue(character))
                .last()
                .map_or(dollar, |(index, _)| index);
            let name = without_leading_zeros(&before[start..]);
            if !name.is_empty() {
                names.push(name);
            }
        }
        rest = &rest[end..];
    }
    names
}

/// `run` without its leading zeros, or `0` if it is only zeros.
fn without_leading_zeros(run: &str) -> &str {
    match run.trim_start_matches('0') {
        "" if !run.is_empty() => "0",
        after => after,
    }
}

/// Whether `name`, a binding a macro reads at a format string whose value is `value`, is one of
/// the names its `{..}` placeholders read, as `placeholders` gives them. `format!` reads `name`
/// for `{name}` and `width` for `{:width$}`. thiserror reads the field `name` of `{name}` as
/// `name`, and binds it to a variable such as `__field_name`; it reads the field `0` of `{0}` as
/// `_0`, bound to `__field0`.
pub(crate) fn reads_placeholder(value: &str, name: &str) -> bool {
    let member = ["__field", "__display", "__pointer"]
        .iter()
        .find_map(|prefix| name.strip_prefix(prefix))
        .unwrap_or(name)
        .trim_start_matches('_');
    !member.is_empty()
        && placeholders(value)
            .iter()
            .any(|placeholder| placeholder.trim_start_matches('_') == member)
}

/// The value of `body`, the text between the quotes of a string literal, with its escapes
/// resolved, or `None` if one can't be read.
fn unescape(body: &str) -> Option<String> {
    let mut value = String::with_capacity(body.len());
    let mut characters = body.chars().peekable();
    while let Some(character) = characters.next() {
        if character != '\\' {
            value.push(character);
            continue;
        }
        match characters.next()? {
            'n' => value.push('\n'),
            'r' => value.push('\r'),
            't' => value.push('\t'),
            '0' => value.push('\0'),
            escaped @ ('\\' | '\'' | '"') => value.push(escaped),
            'x' => {
                let digits: String = characters.by_ref().take(2).collect();
                let code = u8::from_str_radix(&digits, 16).ok().filter(u8::is_ascii)?;
                value.push(char::from(code));
            }
            'u' => {
                if characters.next()? != '{' {
                    return None;
                }
                let mut digits = String::new();
                loop {
                    match characters.next()? {
                        '}' => break,
                        '_' => {}
                        digit => digits.push(digit),
                    }
                }
                value.push(char::from_u32(u32::from_str_radix(&digits, 16).ok()?)?);
            }
            '\n' => while characters.next_if(|next| next.is_whitespace()).is_some() {},
            _ => return None,
        }
    }
    Some(value)
}

#[test]
fn reads_values() {
    let value = |text| match literal(text) {
        Some(Literal::Value(value)) => Some(Some(value)),
        Some(Literal::Unreadable) => Some(None),
        None => None,
    };
    assert_eq!(value(r#""a::b""#), Some(Some("a::b".to_owned())));
    assert_eq!(value(r##"r#"a::b"#"##), Some(Some("a::b".to_owned())));
    assert_eq!(value(r#""a::\u{62}""#), Some(Some("a::b".to_owned())));
    assert_eq!(value(r#""a::\x62\n""#), Some(Some("a::b\n".to_owned())));
    assert_eq!(value(r#""\q""#), Some(None));
    assert_eq!(value("r#a"), None);
    assert_eq!(value("retries"), None);
    assert_eq!(value(r#""Cafe\u{301}""#), Some(Some("Café".to_owned())));
    assert_eq!(value("r\"Cafe\u{301}\""), Some(Some("Café".to_owned())));
}

#[test]
fn spells_paths() {
    let spells = |value: &str, names: &[&str]| Literal::Value(value.to_owned()).may_spell(names);
    assert!(spells(
        "std::string::/**/String::new",
        &["std", "string", "String", "new"]
    ));
    assert!(spells(
        "crate::defaults::r#zero::<>",
        &["crate", "defaults", "zero"]
    ));
    assert!(spells("A<B::<C>::d>", &["B", "d"]));
    assert!(spells("flag", &["flag", "deserialize"]));

    assert!(!spells("a message", &["hidden_fields", "helper"]));
    assert!(!spells("{0}", &["as_display"]));
    assert!(spells("0", &["0"]));
    assert!(!spells("{0:>5} {1}", &["as_display"]));
    assert!(!spells("", &["helper"]));
    assert!(Literal::Unreadable.may_spell(&["anything"]));
}

#[test]
fn reads_paths() {
    assert_eq!(paths("Vec<u8>"), Some(vec![vec!["Vec"], vec!["u8"]]));
    assert_eq!(
        paths("T: Deserialize<'de> + serde::Serialize"),
        Some(vec![
            vec!["T"],
            vec!["Deserialize"],
            vec!["serde", "Serialize"]
        ])
    );
    assert_eq!(
        paths("(u32, [u8; 4], &'static str, Box<dyn Fn(u8) -> u8>, Vec<_>)"),
        Some(vec![
            vec!["u32"],
            vec!["u8"],
            vec!["str"],
            vec!["Box"],
            vec!["dyn"],
            vec!["Fn"],
            vec!["u8"],
            vec!["u8"],
            vec!["Vec"],
        ])
    );
    assert_eq!(
        paths("defaults::zero"),
        Some(vec![vec!["defaults", "zero"]])
    );
    assert_eq!(
        paths("T: FromStr, <T as FromStr>::Err: Display"),
        Some(vec![
            vec!["T"],
            vec!["FromStr"],
            vec!["T"],
            vec!["FromStr", "Err"],
            vec!["Display"],
        ])
    );
    assert_eq!(
        paths("<Vec<T> as Into<a::B>>::Output::Item"),
        Some(vec![
            vec!["Vec"],
            vec!["T"],
            vec!["Into", "Output", "Item"],
            vec!["a", "B"],
        ])
    );
    assert_eq!(paths("Café"), Some(vec![vec!["Café"]]));
    for written in [
        "::zero",
        "a:: b",
        "a ::b",
        "a::/**/b",
        "a::r#b",
        "a::b::<>",
        "<T>::B",
        "<T as>::B",
        "<T as A::>::B",
        "a::",
        "[u8; { 4 }]",
        "a!()",
        "a.b",
        "[u8; 4_usize]",
        "#[a] u8",
        "\"a\"",
        "u8 // a",
        "a:::b",
    ] {
        assert_eq!(paths(written), None, "{written}");
    }
}

#[test]
fn reads_placeholders() {
    assert_eq!(placeholders("{{x}} {name:?} {0} {}"), vec!["name", "0", ""]);
    assert_eq!(
        placeholders("{name:>width$} {value:.precision$} {:1$} {:.*}"),
        vec!["name", "width", "value", "precision", "", "1", ""]
    );
    assert_eq!(placeholders("{x:>é$}"), vec!["x", "é"]);
    assert_eq!(placeholders("{café} {self.x}"), vec!["café", "self"]);
    assert_eq!(placeholders("{01} {00} {0}"), vec!["1", "0", "0"]);
    assert_eq!(
        placeholders("{n:0width$} {:0width$.prec$} {:01$} {:0$} {:001$}"),
        vec!["n", "width", "", "width", "prec", "", "1", "", "0", "", "1"]
    );
}
