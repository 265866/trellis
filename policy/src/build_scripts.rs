//! No member has a build script unless the policy settings allow it.

use std::ffi::OsStr;
use std::fs;
use std::path::PathBuf;

use toml::Value;

use crate::policy_error::PolicyError;
use crate::violation::Violation;
use crate::workspace::{Workspace, WorkspaceMember};
use crate::{case_insensitive, source_path, toml_file};

/// Checks that no member has a `build.rs`, in any case, or a `build` key in its `[package]` table,
/// unless `build_script_crates` in `[workspace.metadata.policy]` of the root `Cargo.toml` lists
/// it. A build script can emit `cargo::rustc-cfg` and so hide code from clippy and the custom
/// lints. A case-insensitive file system finds `Build.rs` when Cargo looks for `build.rs`.
///
/// It also checks that each crate the setting lists is a member with a build script, so the list
/// doesn't keep allowing one after the crate stops needing it. The list entry, the `build` key,
/// the `[build-dependencies]` and the script's own file, the crate's `build.rs` or the file the
/// key names, are all rules, so adding or removing a build script is one rule change that holds
/// all of them, and no order of landing needs a stale entry. The modules the script reads and the
/// crates it calls are code, like the crate's other sources, so once a crate is listed, a code
/// change can still change what its build script emits; the list entry is what review weighs.
///
/// A listed crate's `build` key names a file in the crate's directory. Outside it, the path could
/// pass through a symbolic link that leads back to one of the crate's modules, and a change to
/// that module would then change the build script as code. The key holds no `\`, which is a
/// separator on Windows and part of a file name elsewhere, and only ASCII characters, since a
/// file system that ignores case or accents reads two other spellings as one file. So every
/// system and the rule-change check agree on which file the key names.
///
/// `build = false`, which turns off Cargo's automatic `build.rs`, is allowed.
///
/// # Errors
///
/// Fails if a member's directory can't be read.
pub fn check(workspace: &Workspace) -> Result<Vec<Violation>, PolicyError> {
    let allowed = workspace.config().build_script_crates();
    let mut violations = Vec::new();
    for member in workspace.members() {
        let build_key = build_key(member);
        let build_file = build_file(member)?;
        if allowed.iter().any(|name| name == member.name()) {
            if build_key.is_none() && build_file.is_none() {
                violations.push(Violation::new(
                    "Cargo.toml".to_owned(),
                    format!(
                        "`build_script_crates` in `[workspace.metadata.policy]` lists `{}`, which \
                         has no build script any more. Remove it from the list.",
                        member.name()
                    ),
                ));
            }
            if let Some(path) = build_key.and_then(Value::as_str) {
                if source_path::escapes(0, path) {
                    violations.push(Violation::new(
                        workspace.relative(member.manifest_path()),
                        format!(
                            "`build = {path:?}` in `[package]` names a build script outside the \
                             crate's directory, where no check looks for a symbolic link on the \
                             way to it. Move the file into the crate's directory."
                        ),
                    ));
                }
                if !path.is_ascii() {
                    violations.push(Violation::new(
                        workspace.relative(member.manifest_path()),
                        format!(
                            "`build = {path:?}` in `[package]` holds a character outside ASCII. \
                             File systems disagree on which such names are one file, so no check \
                             can tell which file the key names everywhere. Give the file an ASCII \
                             name."
                        ),
                    ));
                }
                if path.contains('\\') {
                    violations.push(Violation::new(
                        workspace.relative(member.manifest_path()),
                        format!(
                            "`build = {path:?}` in `[package]` holds a `\\`, which separates \
                             directories on Windows and is part of a name elsewhere, so the key \
                             names different files on different systems. Write the path with `/`."
                        ),
                    ));
                }
            }
            continue;
        }
        if let Some(value) = build_key {
            violations.push(Violation::new(
                workspace.relative(member.manifest_path()),
                format!(
                    "`build = {}` in `[package]` gives the crate a build script. {}",
                    toml_file::render(value),
                    remedy(member.name())
                ),
            ));
        }
        if let Some(build_file) = build_file {
            violations.push(Violation::new(
                workspace.relative(&build_file),
                format!(
                    "Cargo runs this file as the crate's build script. {}",
                    remedy(member.name())
                ),
            ));
        }
    }
    for name in allowed {
        if !workspace
            .members()
            .iter()
            .any(|member| member.name() == name)
        {
            violations.push(Violation::new(
                "Cargo.toml".to_owned(),
                format!(
                    "`build_script_crates` in `[workspace.metadata.policy]` lists `{name}`, which \
                     isn't a workspace member. Remove it from the list."
                ),
            ));
        }
    }
    Ok(violations)
}

/// The value of `member`'s `build` key, unless it is missing or `false`.
fn build_key(member: &WorkspaceMember) -> Option<&Value> {
    toml_file::lookup(member.manifest(), &["package", "build"])
        .filter(|value| **value != Value::Boolean(false))
}

/// The path of `member`'s `build.rs`, in any case, if it has one.
fn build_file(member: &WorkspaceMember) -> Result<Option<PathBuf>, PolicyError> {
    let read_error = |source| PolicyError::Read {
        path: member.dir().to_owned(),
        source,
    };
    for entry in fs::read_dir(member.dir()).map_err(read_error)? {
        let path = entry.map_err(read_error)?.path();
        let is_build_file = path
            .file_name()
            .and_then(OsStr::to_str)
            .is_some_and(|name| case_insensitive::eq(name, "build.rs"));
        if is_build_file {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

/// How to fix a build script in the crate `name`.
fn remedy(name: &str) -> String {
    format!(
        "A build script can emit `cargo::rustc-cfg` and hide code from clippy and the custom lints, \
         so crates have none. Remove it, or, if the crate can't do without it, add `{name}` to \
         `build_script_crates` in `[workspace.metadata.policy]` of the root Cargo.toml in a pull \
         request of its own that says why."
    )
}
