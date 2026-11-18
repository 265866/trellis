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
    fn rust_files_are_named_rs_in_any_case() {
        for path in ["lib.rs", "src/LIB.RS", "a\\b.Rs"] {
            assert!(is_rust(path), "{path} is a Rust file");
        }
        for path in ["notes.txt", "rs", "lib.rs.md", "src/rs/"] {
            assert!(!is_rust(path), "{path} isn't a Rust file");
        }
    }
}
