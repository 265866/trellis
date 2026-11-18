//! File names compared the way a case-insensitive file system compares them.
//!
//! Windows and macOS find `Clippy.toml` when a tool asks for `clippy.toml`, so every rule about a
//! file's name matches it in any case. Besides ASCII letters in either case, Unicode case folding,
//! which macOS's APFS uses, maps the long s (U+017F) to `s` and the Kelvin sign (U+212A) to `k`,
//! so those match too.

/// Whether `left` and `right` name the same file on a case-insensitive file system.
pub(crate) fn eq(left: &str, right: &str) -> bool {
    left.chars().map(fold).eq(right.chars().map(fold))
}

/// `path` without `prefix`, if `path` starts with it in any case.
pub(crate) fn strip_prefix<'a>(path: &'a str, prefix: &str) -> Option<&'a str> {
    let mut rest = path.char_indices();
    for expected in prefix.chars() {
        let (_, found) = rest.next()?;
        if fold(found) != fold(expected) {
            return None;
        }
    }
    Some(rest.as_str())
}

/// `path` without `suffix`, if `path` ends with it in any case.
pub(crate) fn strip_suffix<'a>(path: &'a str, suffix: &str) -> Option<&'a str> {
    let mut rest = path.chars();
    for expected in suffix.chars().rev() {
        let found = rest.next_back()?;
        if fold(found) != fold(expected) {
            return None;
        }
    }
    Some(rest.as_str())
}

/// `character` as a case-insensitive file system compares it.
fn fold(character: char) -> char {
    match character {
        '\u{17f}' => 's',
        '\u{212a}' => 'k',
        _ => character.to_ascii_lowercase(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_match_in_any_case() {
        for (left, right) in [
            ("clippy.toml", "Clippy.toml"),
            ("build.rs", "BUILD.RS"),
            ("rustfmt.toml", "ru\u{17f}tfmt.toml"),
            ("Makefile", "ma\u{212a}efile"),
        ] {
            assert!(eq(left, right), "{left} and {right} name the same file");
        }
        for (left, right) in [
            ("clippy.toml", "clippy.tom"),
            ("build.rs", "build.rs "),
            ("deny.toml", "d\u{e9}ny.toml"),
        ] {
            assert!(!eq(left, right), "{left} and {right} name different files");
        }
    }

    #[test]
    fn prefixes_and_suffixes_match_in_any_case() {
        assert_eq!(
            strip_prefix("Lints/src/lib.rs", "lints/"),
            Some("src/lib.rs"),
            "the prefix matches in any case"
        );
        assert_eq!(
            strip_prefix("lint", "lints/"),
            None,
            "a shorter path has no prefix"
        );
        assert_eq!(
            strip_suffix("crates/app/CLIPPY.toml", "clippy.toml"),
            Some("crates/app/"),
            "the suffix matches in any case"
        );
        assert_eq!(
            strip_suffix("crates/\u{e9}/myclippy.toml", "clippy.toml"),
            Some("crates/\u{e9}/my"),
            "a non-ASCII path keeps its characters"
        );
        assert_eq!(
            strip_suffix("toml", "clippy.toml"),
            None,
            "a shorter path has no suffix"
        );
    }
}
