//! The files in the repository that the checks read.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::git;
use crate::policy_error::PolicyError;

/// The files in the repository at `root`, relative to it, with `/` separators and sorted.
///
/// In a git checkout, these are the files git tracks and the untracked files `.gitignore` doesn't
/// exclude, as `git ls-files --cached --others --exclude-standard` lists them, so a file is found
/// whatever its directory is called. Another repository inside this one, such as a git worktree,
/// is left out, and so is a tracked file deleted from the working tree.
///
/// Outside a git checkout, such as in a source archive, every file is listed except those under
/// Cargo's build output, `target/` at the root.
///
/// # Errors
///
/// Fails if git fails in a checkout, or if a directory can't be read outside one.
pub(crate) fn list(root: &Path) -> Result<Vec<String>, PolicyError> {
    let git_entry = root.join(".git");
    let is_checkout = fs::exists(&git_entry).map_err(|source| PolicyError::Read {
        path: git_entry,
        source,
    })?;
    if is_checkout {
        git_files(root)
    } else {
        walk(root)
    }
}

/// The files that git tracks or would add in the checkout at `root`.
fn git_files(root: &Path) -> Result<Vec<String>, PolicyError> {
    let output = git::run(
        root,
        &[
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ],
    )?;
    let mut files = BTreeSet::new();
    for path in git::paths(&output) {
        if path.ends_with('/') {
            continue;
        }
        let full = root.join(path);
        let exists = fs::symlink_metadata(&full).is_ok();
        if exists {
            files.insert(path.to_owned());
        }
    }
    Ok(files.into_iter().collect())
}

/// Every file under `root`, outside a git checkout. Symbolic links are listed, not followed.
fn walk(root: &Path) -> Result<Vec<String>, PolicyError> {
    let mut files = Vec::new();
    let mut directories: Vec<PathBuf> = vec![root.to_owned()];
    while let Some(directory) = directories.pop() {
        let read_error = |source| PolicyError::Read {
            path: directory.clone(),
            source,
        };
        for entry in fs::read_dir(&directory).map_err(read_error)? {
            let path = entry.map_err(read_error)?.path();
            let Some(relative) = relative(root, &path) else {
                continue;
            };
            let metadata = fs::symlink_metadata(&path).map_err(|source| PolicyError::Read {
                path: path.clone(),
                source,
            })?;
            if !metadata.is_dir() {
                files.push(relative);
            } else if !is_build_output(&relative) {
                directories.push(path);
            }
        }
    }
    files.sort();
    Ok(files)
}

/// `path` relative to `root`, with `/` separators, if it is under `root` and its name is UTF-8.
fn relative(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    let mut parts = Vec::new();
    for component in relative.components() {
        parts.push(component.as_os_str().to_str()?);
    }
    Some(parts.join("/"))
}

/// Whether the directory `relative` is Cargo's build output for the workspace.
fn is_build_output(relative: &str) -> bool {
    relative == "target"
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use tempfile::TempDir;

    use super::*;

    fn write(root: &Path, path: &str) {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "").unwrap();
    }

    fn files_in(paths: &[&str], checkout_gitignore: Option<&str>) -> Vec<String> {
        let directory = TempDir::new().unwrap();
        for path in paths {
            write(directory.path(), path);
        }
        if let Some(gitignore) = checkout_gitignore {
            fs::write(directory.path().join(".gitignore"), gitignore).unwrap();
            let status = Command::new("git")
                .arg("-C")
                .arg(directory.path())
                .args(["init", "--quiet"])
                .status()
                .unwrap();
            assert!(status.success(), "git init should succeed");
        }
        list(directory.path()).unwrap()
    }

    #[test]
    fn a_checkout_lists_untracked_files_whatever_their_directory_is_called() {
        let files = files_in(
            &[
                "crates/target_rules/CACHEDIR.TAG",
                "crates/target_rules/rustfmt.toml",
                "target/CACHEDIR.TAG",
                "target/debug/clippy.toml",
            ],
            Some("/target\n"),
        );
        assert_eq!(
            files,
            [
                ".gitignore",
                "crates/target_rules/CACHEDIR.TAG",
                "crates/target_rules/rustfmt.toml",
            ],
            "only `.gitignore` decides what is skipped, not a directory's name or CACHEDIR.TAG"
        );
    }

    #[test]
    fn outside_a_checkout_only_the_build_output_is_skipped() {
        let files = files_in(
            &[
                "crates/target_rules/rustfmt.toml",
                "crates/app/target/clippy.toml",
                "tools/target/debug/clippy.toml",
                "targets/clippy.toml",
                "target/debug/clippy.toml",
            ],
            None,
        );
        assert_eq!(
            files,
            [
                "crates/app/target/clippy.toml",
                "crates/target_rules/rustfmt.toml",
                "targets/clippy.toml",
                "tools/target/debug/clippy.toml",
            ],
            "only `target/` at the root is skipped"
        );
    }
}
