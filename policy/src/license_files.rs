//! Each published member carries the license files its license names, since a crate's package
//! holds only the files under its directory.

use std::ffi::OsStr;
use std::fs;
use std::path::Path;

use spdx::{Expression, LicenseItem, ParseMode};

use crate::case_insensitive;
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

/// The licenses whose files the check knows: the two that trellis's `license` answer offers,
/// alone or together.
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

/// The files a crate under a license expression carries: each entry is a requirement, met by any
/// one of its files.
type Requirements = Vec<Vec<&'static str>>;

/// Checks that every published member's directory holds the license files its `license`
/// expression names, in any case: under one license, such as `MIT`, `LICENSE` or
/// `LICENSE-<license>`, here `LICENSE-MIT`; under several, such as `MIT OR Apache-2.0`, a
/// `LICENSE-<license>` for each, here `LICENSE-MIT` and `LICENSE-APACHE`.
///
/// Cargo packages only the files under a crate's directory, so a crate whose license files sit
/// only at the repository root reaches crates.io without the text its license asks every copy to
/// carry, and nothing in `cargo package` or `cargo publish` says so. The root keeps the files for
/// the repository, and each published crate keeps its own copy.
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
        let Some(requirements) = requirements(expression) else {
            continue;
        };
        let present = file_names(member.dir())?;
        let missing: Requirements = requirements
            .into_iter()
            .filter(|files| {
                !files
                    .iter()
                    .any(|file| present.iter().any(|name| case_insensitive::eq(name, file)))
            })
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
/// expression doesn't parse or names a license the check doesn't know. It parses as crates.io
/// takes it, so `MIT/Apache-2.0` reads as `MIT OR Apache-2.0`, and a `WITH` exception asks for no
/// file beyond its license's.
fn requirements(expression: &str) -> Option<Requirements> {
    let parsed = Expression::parse_mode(expression, ParseMode::LAX).ok()?;
    let mut licenses: Vec<&KnownLicense> = Vec::new();
    for requirement in parsed.requirements() {
        let LicenseItem::Spdx { id, .. } = requirement.req.license else {
            return None;
        };
        let license = KNOWN_LICENSES
            .iter()
            .find(|license| license.id == id.name)?;
        if !licenses.iter().any(|known| known.id == license.id) {
            licenses.push(license);
        }
    }
    match licenses.as_slice() {
        [] => None,
        [license] => Some(vec![vec![SOLE_LICENSE_FILE, license.file]]),
        _ => Some(licenses.iter().map(|license| vec![license.file]).collect()),
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

/// What is wrong with `member`, published under `expression`, whose directory lacks the files of
/// `missing`, and where to copy each from: the root's file for the requirement, or its first file
/// when the root has none.
fn problem(
    workspace: &Workspace,
    member: &WorkspaceMember,
    expression: &str,
    missing: &Requirements,
) -> String {
    let directory = format!("{}/", workspace.relative(member.dir()));
    let quoted = |files: &[&str], separator: &str| {
        files
            .iter()
            .map(|file| format!("`{file}`"))
            .collect::<Vec<_>>()
            .join(separator)
    };
    let lacks = match missing.as_slice() {
        [files] if files.len() > 1 => format!("has neither {}", quoted(files, " nor ")),
        _ => format!("lacks {}", quoted(&missing.concat(), " and ")),
    };
    let copies: Vec<&str> = missing
        .iter()
        .filter_map(|files| {
            files
                .iter()
                .copied()
                .find(|file| workspace.root().join(file).is_file())
                .or_else(|| files.first().copied())
        })
        .collect();
    format!(
        "`{name}` is published under `{expression}`, but `{directory}` {lacks}. Cargo packages \
         only the files under a crate's directory, so the crate would reach crates.io without its \
         license text, which the license asks every copy to carry. Copy {copies} from the \
         repository root into `{directory}`.",
        name = member.name(),
        copies = quoted(&copies, " and "),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_license_takes_its_sole_file_or_its_named_one() {
        assert_eq!(
            requirements("MIT"),
            Some(vec![vec!["LICENSE", "LICENSE-MIT"]]),
            "MIT alone is met by LICENSE or LICENSE-MIT"
        );
        assert_eq!(
            requirements("Apache-2.0"),
            Some(vec![vec!["LICENSE", "LICENSE-APACHE"]]),
            "Apache-2.0 alone is met by LICENSE or LICENSE-APACHE"
        );
    }

    #[test]
    fn several_licenses_take_a_named_file_each() {
        let both = Some(vec![vec!["LICENSE-MIT"], vec!["LICENSE-APACHE"]]);
        assert_eq!(
            requirements("MIT OR Apache-2.0"),
            both,
            "the template's dual license needs both files"
        );
        assert_eq!(
            requirements("(MIT AND Apache-2.0)"),
            both,
            "operators and parentheses separate the licenses"
        );
        assert_eq!(
            requirements("Apache-2.0 OR MIT"),
            Some(vec![vec!["LICENSE-APACHE"], vec!["LICENSE-MIT"]]),
            "the order of the expression doesn't matter"
        );
    }

    #[test]
    fn an_expression_reads_as_crates_io_reads_it() {
        assert_eq!(
            requirements("MIT/Apache-2.0"),
            Some(vec![vec!["LICENSE-MIT"], vec!["LICENSE-APACHE"]]),
            "crates.io reads a slash as OR"
        );
        assert_eq!(
            requirements("Apache-2.0 WITH LLVM-exception"),
            Some(vec![vec!["LICENSE", "LICENSE-APACHE"]]),
            "an exception asks for no file of its own"
        );
    }

    #[test]
    fn a_license_named_twice_counts_once() {
        assert_eq!(
            requirements("MIT OR MIT"),
            Some(vec![vec!["LICENSE", "LICENSE-MIT"]]),
            "MIT twice is still one license"
        );
    }

    #[test]
    fn an_unknown_license_leaves_the_crate_alone() {
        for expression in [
            "BSD-3-Clause",
            "MIT OR BSD-3-Clause",
            "",
            "OR",
            "LicenseRef-Own",
        ] {
            assert_eq!(
                requirements(expression),
                None,
                "the check doesn't know the files of {expression:?}"
            );
        }
    }
}
