//! Paths that name a crate's source files.

use std::path::Path;

/// Whether `path`, written relative to a directory `depth` levels below a crate's directory,
/// reaches outside the crate's directory. An absolute path, a path with a drive such as `C:`, and
/// a `..` that climbs above the crate's directory do. Both `/` and `\` count as separators,
/// because Windows reads either.
pub(crate) fn escapes(depth: usize, path: &str) -> bool {
    if path.starts_with(['/', '\\']) || path.contains(':') {
        return true;
    }
    let mut depth = depth;
    for part in path.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => match depth.checked_sub(1) {
                Some(parent) => depth = parent,
                None => return true,
            },
            _ => depth += 1,
        }
    }
    false
}

/// Whether Windows reads `path`, written with `/` separators, as another path: one it rewrites,
/// as [`windows_rewrites`] says, or one with a part of the short name form, which it reads as the
/// file that has that short name, as [`is_short_name`] says.
pub(crate) fn windows_reads_as_another(path: &str) -> bool {
    windows_rewrites(path) || path.split('/').any(is_short_name)
}

/// Whether Windows rewrites `path`, written with `/` separators. It drops dots and spaces from the
/// end of each part, reads a `:` as the start of a stream of the file before it, and reads a `\`
/// as a separator, so `build.rs.` and `build.rs::$DATA` are `build.rs` there. Git for Windows
/// refuses to check such a path out, but only while its `core.protectNTFS` setting is on.
pub(crate) fn windows_rewrites(path: &str) -> bool {
    path.split('/')
        .any(|part| part.contains([':', '\\']) || part.ends_with(['.', ' ']))
}

/// The most characters before the dot of a short name, and after it.
const SHORT_NAME_LENGTHS: (usize, usize) = (8, 3);

/// Whether `part`, one name of a path, has the form of the short name Windows gives a file whose
/// own name doesn't fit eight characters, a dot and three more: at most eight characters that end
/// in `~` and digits, then at most one dot with at most three characters after it, as `CARGO~1.TOM`
/// is for `Cargo.toml` and `GITIGN~1` for `.gitignore`. A volume that keeps short names opens the
/// longer-named file under that name, in any letter case. Git writes both paths there whatever
/// its settings, in byte order, so when the longer name sorts first, as `Cargo.toml` does before
/// `cargo~1.tom`, the checkout holds one file with the contents of the other.
pub(crate) fn is_short_name(part: &str) -> bool {
    let (base, extension) = part.split_once('.').unwrap_or((part, ""));
    let numbered = base
        .trim_end_matches(|character: char| character.is_ascii_digit())
        .ends_with('~');
    numbered
        && !base.ends_with('~')
        && base.chars().count() <= SHORT_NAME_LENGTHS.0
        && !extension.contains('.')
        && extension.chars().count() <= SHORT_NAME_LENGTHS.1
}

/// Whether `path` names a Rust source file: one whose extension is `rs` in any case, since a
/// case-insensitive file system finds `lib.RS` for `lib.rs`.
pub(crate) fn is_rust(path: &str) -> bool {
    Path::new(path)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("rs"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_inside_the_crate_stay() {
        for (depth, path) in [
            (0, "src/lib.rs"),
            (0, "./tests/../tests/app.rs"),
            (1, "../build/x.rs"),
            (2, "../../x.rs"),
            (0, "src//a.rs"),
        ] {
            assert!(
                !escapes(depth, path),
                "{path} from depth {depth} stays inside the crate"
            );
        }
    }

    #[test]
    fn paths_outside_the_crate_escape() {
        for (depth, path) in [
            (0, "../other/src/lib.rs"),
            (1, "../../docs/notes.rs"),
            (2, "a/../../../../x.rs"),
            (2, "..\\..\\..\\x.rs"),
            (0, "/etc/x.rs"),
            (0, "\\x.rs"),
            (0, "C:/x.rs"),
            (0, "C:x.rs"),
        ] {
            assert!(
                escapes(depth, path),
                "{path} from depth {depth} leaves the crate"
            );
        }
    }

    #[test]
    fn names_of_the_short_form_are_short_names() {
        for part in [
            "CARGO~1.TOM",
            "cargo~1.tom",
            "GITIGN~1",
            "ABCDEF~1.RS",
            "AB1F2C~1",
            "ABCD~123",
            "A~1",
            "~1",
            "~1.rs",
            "abcdef~1.",
            "\u{e9}t\u{e9}~1.rs",
        ] {
            assert!(is_short_name(part), "{part} has the short name form");
        }
        for part in [
            "Cargo.toml",
            ".gitignore",
            "build.rs",
            "notes~",
            "notes~.md",
            "~",
            "a~b",
            "a~1b",
            "abcdefg~1",
            "abcdef~1.toml",
            "abc~1.d.e",
            "~1.tar.gz",
            ".abc~1",
            "backup.rs~1",
            "",
        ] {
            assert!(!is_short_name(part), "{part} hasn't the short name form");
        }
    }

    #[test]
    fn rust_files_are_named_rs_in_any_case() {
        for path in ["lib.rs", "src/LIB.RS", "a\\b.Rs"] {
            assert!(is_rust(path), "{path} is a Rust file");
        }
        for path in ["notes.txt", "rs", "lib.rs.md", "src/rs/"] {
            assert!(!is_rust(path), "{path} isn't a Rust file");
        }
    }
}
