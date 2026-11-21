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
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    /// Asks the file system the tests run on which names it reads as an ASCII name, and checks
    /// that [`eq`] calls each pair the same file. It writes a file for every one-letter and
    /// two-letter ASCII name, and `ffi` and `ffl`, each holding its own name. Then it reads a
    /// file named by each non-ASCII character alone: a read that succeeds returns the ASCII name
    /// the file system chose. It reads `x`, the character, `y` the same way, for a character the
    /// file system leaves out of a comparison, and a few names with dots and spaces at their ends.
    #[test]
    fn every_name_the_file_system_reads_as_an_ascii_name_matches_it() {
        let directory = TempDir::new().expect("the temporary directory should be created");
        let letters: Vec<char> = ('a'..='z').chain('0'..='9').collect();
        let mut names: Vec<String> = letters.iter().map(char::to_string).collect();
        for first in 'a'..='z' {
            for second in 'a'..='z' {
                names.push(format!("{first}{second}"));
            }
        }
        names.extend(["ffi".to_owned(), "ffl".to_owned()]);
        for name in &names {
            fs::write(directory.path().join(name), name).expect("the file should be written");
        }
        let mut candidates: Vec<String> = Vec::new();
        for character in '\u{80}'..='\u{1ffff}' {
            candidates.push(character.to_string());
            candidates.push(format!("x{character}y"));
        }
        for edge in ["xy.", "xy ", "xy..", "xy. ", " xy", "xy\t"] {
            candidates.push(edge.to_owned());
        }
        let mut missed = Vec::new();
        for candidate in candidates {
            if let Ok(found) = fs::read_to_string(directory.path().join(&candidate))
                && !eq(&candidate, &found)
            {
                missed.push(format!("{} reads `{found}`", candidate.escape_unicode()));
            }
        }
        assert!(
            missed.is_empty(),
            "this file system reads these names as an ASCII name that `eq` tells apart: {missed:#?}"
        );
    }

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
