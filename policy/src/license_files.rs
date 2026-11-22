//! Each published member carries the license files its license names, since a crate's package
//! holds only the files under its directory.

use std::ffi::OsStr;
use std::fs;
use std::path::Path;

use spdx::{Expression, LicenseItem, ParseMode};

use crate::policy_error::PolicyError;
use crate::violation::Violation;
use crate::workspace::{Workspace, WorkspaceMember};

/// A license the check knows the file of, by its SPDX identifier.
#[derive(Clone, Copy, Debug)]
struct KnownLicense {
    /// The SPDX identifier, as an expression names it.
    id: &'static str,
    /// The file that holds its text beside another license's.
    file: &'static str,
}

/// The licenses whose files the check knows, in the order it names them: the two that trellis's
/// `license` answer offers, alone or together.
const KNOWN_LICENSES: [KnownLicense; 2] = [
    KnownLicense {
        id: "MIT",
        file: "LICENSE-MIT",
    },
    KnownLicense {
        id: "Apache-2.0",
        file: "LICENSE-APACHE",
    },
];

/// The file that holds a crate's only license, whichever it is.
const SOLE_LICENSE_FILE: &str = "LICENSE";

/// How crates.io parses a crate's `license`: the `spdx` crate's lax mode, which reads `/` as `OR`,
/// but with the license names exact, so `Apache 2.0` or `mit` doesn't parse.
const CRATES_IO: ParseMode = ParseMode {
    allow_imprecise_license_names: false,
    ..ParseMode::LAX
};

/// Checks that every published member's directory holds the license files its `license`
/// expression names, by the names trellis gives them and `just package` checks, in that case:
/// under one license, such as `MIT`, `LICENSE`; under several, such as `MIT OR Apache-2.0`, a
/// `LICENSE-<license>` for each, here `LICENSE-MIT` and `LICENSE-APACHE`.
///
/// Cargo packages only the files under a crate's directory, so a crate whose license files sit
/// only at the repository root reaches crates.io without the text its license asks every copy to
/// carry, and nothing in `cargo package` or `cargo publish` says so. The root keeps the files for
/// the repository, and each published crate keeps its own copy. Another name, such as `license`,
/// `LICENSE.md` or, under MIT alone, `LICENSE-MIT`, would carry the text too, but trellis's
/// license files have one name each, so a crate's match the generated crate's and the root's.
///
/// It leaves alone a crate that isn't published, since no package of it is made; a crate with no
/// `license`, which `cargo publish` refuses on its own; a crate with a `license-file`, whose file
/// cargo packages from wherever it is; and an expression with a license other than MIT and
/// Apache-2.0, whose file names the check doesn't know.
///
/// # Errors
///
/// Fails if a published member's directory can't be read.
pub fn check(workspace: &Workspace) -> Result<Vec<Violation>, PolicyError> {
    let metadata = workspace.metadata();
    let mut violations = Vec::new();
    for member in workspace.members() {
        let package = &metadata[member.id()];
        if !workspace.publishes(member) || package.license_file.is_some() {
            continue;
        }
        let Some(expression) = package.license.as_deref() else {
            continue;
        };
        let Some(files) = license_files(expression) else {
            continue;
        };
        let present = file_names(member.dir())?;
        let missing: Vec<&str> = files
            .into_iter()
            .filter(|file| !present.iter().any(|name| name == file))
            .collect();
        if !missing.is_empty() {
            violations.push(Violation::new(
                workspace.relative(member.manifest_path()),
                problem(workspace, member, expression, &missing),
            ));
        }
    }
    Ok(violations)
}

/// The files a crate under the SPDX expression `expression` carries, or nothing when the
/// expression doesn't parse as crates.io parses it or names a license the check doesn't know. So
/// `MIT/Apache-2.0` reads as `MIT OR Apache-2.0`, and a `WITH` exception asks for no file beyond
/// its license's.
fn license_files(expression: &str) -> Option<Vec<&'static str>> {
    let parsed = Expression::parse_mode(expression, CRATES_IO).ok()?;
    let mut ids = Vec::new();
    for requirement in parsed.requirements() {
        let LicenseItem::Spdx { id, .. } = requirement.req.license else {
            return None;
        };
        if !KNOWN_LICENSES.iter().any(|license| license.id == id.name) {
            return None;
        }
        ids.push(id.name);
    }
    let licenses: Vec<&KnownLicense> = KNOWN_LICENSES
        .iter()
        .filter(|license| ids.contains(&license.id))
        .collect();
    match licenses.as_slice() {
        [] => None,
        [_] => Some(vec![SOLE_LICENSE_FILE]),
        _ => Some(licenses.iter().map(|license| license.file).collect()),
    }
}

/// The names of the entries of the directory `directory` that are files.
fn file_names(directory: &Path) -> Result<Vec<String>, PolicyError> {
    let read_error = |source| PolicyError::Read {
        path: directory.to_owned(),
        source,
    };
    let mut names = Vec::new();
    for entry in fs::read_dir(directory).map_err(read_error)? {
        let path = entry.map_err(read_error)?.path();
        if path.is_file()
            && let Some(name) = path.file_name().and_then(OsStr::to_str)
        {
            names.push(name.to_owned());
        }
    }
    Ok(names)
}

/// What is wrong with `member`, published under `expression`, whose directory lacks `missing`.
fn problem(
    workspace: &Workspace,
    member: &WorkspaceMember,
    expression: &str,
    missing: &[&str],
) -> String {
    let directory = format!("{}/", workspace.relative(member.dir()));
    let files = missing
        .iter()
        .map(|file| format!("`{file}`"))
        .collect::<Vec<_>>()
        .join(" and ");
    format!(
        "`{name}` is published under `{expression}`, but `{directory}` lacks {files}. Cargo \
         packages only the files under a crate's directory, so the crate would reach crates.io \
         without its license text, which the license asks every copy to carry. Copy {files} from \
         the repository root into `{directory}`, under the same {names}, which `just package` \
         checks too.",
        name = member.name(),
        names = if missing.len() == 1 { "name" } else { "names" },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_license_takes_the_sole_license_file() {
        for expression in [
            "MIT",
            "Apache-2.0",
            "MIT OR MIT",
            "Apache-2.0 WITH LLVM-exception",
        ] {
            assert_eq!(
                license_files(expression),
                Some(vec!["LICENSE"]),
                "{expression:?} names one license, whose text goes in LICENSE"
            );
        }
    }

    #[test]
    fn several_licenses_take_a_named_file_each_in_one_order() {
        for expression in [
            "MIT OR Apache-2.0",
            "Apache-2.0 OR MIT",
            "(MIT AND Apache-2.0)",
            "MIT/Apache-2.0",
        ] {
            assert_eq!(
                license_files(expression),
                Some(vec!["LICENSE-MIT", "LICENSE-APACHE"]),
                "{expression:?} names MIT and Apache-2.0, whose texts go in LICENSE-MIT and \
                 LICENSE-APACHE, in the template's order"
            );
        }
    }

    #[test]
    fn an_unknown_or_imprecise_license_leaves_the_crate_alone() {
        for expression in [
            "BSD-3-Clause",
            "MIT OR BSD-3-Clause",
            "",
            "OR",
            "LicenseRef-Own",
            "Apache 2.0",
            "mit",
        ] {
            assert_eq!(
                license_files(expression),
                None,
                "crates.io doesn't take {expression:?}, or the check doesn't know its files"
            );
        }
    }
}
