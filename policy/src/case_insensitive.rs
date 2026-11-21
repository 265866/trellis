//! File names compared the way a case-insensitive file system compares them.
//!
//! Windows and macOS find `Clippy.toml` when a tool asks for `clippy.toml`, so every rule about a
//! file's name matches it in any case. Besides ASCII letters in either case, macOS's APFS reads a
//! few other characters as ASCII letters: the long s (U+017F) as `s`, the Kelvin sign (U+212A) as
//! `k`, the sharp s in either case (U+00DF, U+1E9E) as `ss`, and the ligatures U+FB00 to U+FB06 as
//! the letters they join, so a name with the `st` ligature in `rustfmt.toml` is that file there.
//! Those match too. A test here asks the file system it runs on which names it reads as an ASCII
//! name, on every system CI has.

/// Whether `left` and `right` name the same file on a case-insensitive file system.
pub(crate) fn eq(left: &str, right: &str) -> bool {
    left.chars().flat_map(fold).eq(right.chars().flat_map(fold))
}

/// `name` as a case-insensitive file system compares it, so two names that it reads as one fold
/// to the same text. A `/` stays a `/`.
pub(crate) fn folded(name: &str) -> String {
    name.chars().flat_map(fold).collect()
}

/// `path` without `prefix`, if `path` starts with it in any case.
pub(crate) fn strip_prefix<'a>(path: &'a str, prefix: &str) -> Option<&'a str> {
    let mut expected = prefix.chars().flat_map(fold).peekable();
    let mut rest = path.chars();
    while expected.peek().is_some() {
        for found in fold(rest.next()?) {
            if expected.next() != Some(found) {
                return None;
            }
        }
    }
    Some(rest.as_str())
}

/// `path` without `suffix`, if `path` ends with it in any case.
pub(crate) fn strip_suffix<'a>(path: &'a str, suffix: &str) -> Option<&'a str> {
    let folded: Vec<char> = suffix.chars().flat_map(fold).collect();
    let mut expected = folded.into_iter().rev().peekable();
    let mut rest = path.chars();
    while expected.peek().is_some() {
        let found: Vec<char> = fold(rest.next_back()?).collect();
        for found in found.into_iter().rev() {
            if expected.next() != Some(found) {
                return None;
            }
        }
    }
    Some(rest.as_str())
}

/// The characters a case-insensitive file system compares in place of `character`.
fn fold(character: char) -> impl Iterator<Item = char> {
    let letters = match character {
        '\u{17f}' => "s",
        '\u{212a}' => "k",
        '\u{df}' | '\u{1e9e}' => "ss",
        '\u{fb00}' => "ff",
        '\u{fb01}' => "fi",
        '\u{fb02}' => "fl",
        '\u{fb03}' => "ffi",
        '\u{fb04}' => "ffl",
        '\u{fb05}' | '\u{fb06}' => "st",
        _ => "",
    };
    let itself = letters.is_empty().then(|| character.to_ascii_lowercase());
    letters.chars().chain(itself)
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;
    use std::path::Path;
    use std::process::{Command, Stdio};
    use std::{env, fs};

    use tempfile::TempDir;

    use super::*;

    /// Runs git in `repository` with `arguments` and `input` on its standard input, and returns
    /// what it printed if it succeeded.
    fn git(repository: &Path, arguments: &[&str], input: &str) -> Option<String> {
        let mut child = Command::new("git")
            .arg("-C")
            .arg(repository)
            .args(arguments)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("git should start");
        child
            .stdin
            .take()
            .expect("git has a standard input")
            .write_all(input.as_bytes())
            .expect("git should read its input");
        let output = child.wait_with_output().expect("git should finish");
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }

    /// Whether git on this system refuses to write a file named `name` out of a commit. Git for
    /// Windows refuses a name that Windows would store under another one, so no checkout there
    /// holds such a file.
    fn git_refuses(name: &str) -> bool {
        let repository = TempDir::new().expect("the temporary directory should be created");
        let at = repository.path();
        git(at, &["init", "--quiet"], "").expect("git should make a repository");
        let blob = git(at, &["hash-object", "-w", "--stdin"], "contents\n")
            .expect("git should store the file");
        let tree = git(at, &["mktree"], &format!("100644 blob {blob}\t{name}\n"))
            .expect("git should store the tree");
        git(at, &["read-tree", &tree], "")
            .and_then(|_| git(at, &["checkout-index", "--all"], ""))
            .is_none()
    }

    /// Asks the file system the tests run on which names it reads as an ASCII name, and checks
    /// that [`eq`] calls each pair the same file. It writes a file for every one-letter and
    /// two-letter ASCII name, and `ffi` and `ffl`, each holding its own name. Then it reads a
    /// file named by each non-ASCII character alone: a read that succeeds returns the ASCII name
    /// the file system chose. It reads `x`, the character, `y` the same way, for a character the
    /// file system leaves out of a comparison, and a few names with dots and spaces at their ends
    /// or a stream after a `:`. Windows reads those as the name without them. Git refuses to
    /// check such a name out there, so it reaches no checkout, and the test accepts it then.
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
        for edge in ["xy.", "xy ", "xy..", "xy. ", " xy", "xy\t", "xy::$DATA"] {
            candidates.push(edge.to_owned());
        }
        assert!(
            !git_refuses("xy"),
            "git writes an ordinary name, so a refusal below is about the name"
        );
        let ignores_case = fs::read_to_string(directory.path().join("A")).is_ok();
        assert!(
            ignores_case || !matches!(env::consts::OS, "macos" | "windows"),
            "the temporary directory keeps case apart, so this test would ask nothing here; run \
             it on a volume that ignores case, as macOS and Windows do by default"
        );
        let mut missed = Vec::new();
        for candidate in candidates {
            if let Ok(found) = fs::read_to_string(directory.path().join(&candidate))
                && !eq(&candidate, &found)
                && !git_refuses(&candidate)
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
            ("rustfmt.toml", "ru\u{fb06}fmt.toml"),
            ("rust-toolchain.toml", "RU\u{fb05}-toolchain.toml"),
            ("config.toml", "con\u{fb01}g.toml"),
            ("tsconfig.json", "TSCON\u{fb01}G.JSON"),
            ("justfile", "ju\u{fb06}file"),
            ("class.rs", "cla\u{df}.rs"),
            ("CLASS.rs", "cla\u{1e9e}.rs"),
            ("office", "o\u{fb03}ce"),
            ("shuffle", "shu\u{fb04}e"),
            ("staff", "sta\u{fb00}"),
            ("flag", "\u{fb02}ag"),
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
        assert_eq!(
            strip_suffix("crates/app/ru\u{fb06}fmt.toml", "rustfmt.toml"),
            Some("crates/app/"),
            "a ligature in the path matches the letters it joins"
        );
        assert_eq!(
            strip_prefix("CRATE\u{17f}/app/lib.rs", "crates/"),
            Some("app/lib.rs"),
            "and so does one in a prefix"
        );
        assert_eq!(
            strip_prefix("lint\u{df}/lib.rs", "lints"),
            None,
            "a prefix that ends inside a character's letters isn't one"
        );
        assert_eq!(
            strip_suffix("a\u{df}.rs", "s.rs"),
            None,
            "nor is a suffix that starts inside them"
        );
    }
}
