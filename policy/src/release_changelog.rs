//! The changelog of the main package takes in the commits of every other workspace crate.

use std::fs;
use std::io::ErrorKind;

use toml::{Table, Value};

use crate::policy_error::PolicyError;
use crate::toml_file;
use crate::violation::Violation;
use crate::workspace::{Workspace, WorkspaceMember};

/// The release-plz configuration, relative to the root.
const RELEASE_PLZ: &str = "release-plz.toml";

/// Checks that each `[[package]]` entry of the root `release-plz.toml` with
/// `changelog_update = true`, the main package that keeps the project's one `CHANGELOG.md`, lists
/// every other workspace member in `changelog_include`, so every crate's commits reach the release
/// notes. The list is a rule, so it can name crates that aren't members yet: a new crate is added
/// to it in the rule change that allows the crate, before the code lands.
///
/// Without a `release-plz.toml` there is nothing to check. With one, an entry with
/// `changelog_update = true` must exist, since without it no release has notes.
///
/// # Errors
///
/// Fails if `release-plz.toml` exists but can't be read or parsed.
pub fn check(workspace: &Workspace) -> Result<Vec<Violation>, PolicyError> {
    let path = workspace.root().join(RELEASE_PLZ);
    match fs::metadata(&path) {
        Ok(_) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => return Err(PolicyError::Read { path, source }),
    }
    let config = toml_file::read(&path)?;
    let members: Vec<&str> = workspace
        .members()
        .iter()
        .map(WorkspaceMember::name)
        .collect();
    Ok(violations(&config, &members))
}

/// The violations of the rule by the parsed `release-plz.toml` `config`, in a workspace with the
/// members `members`.
fn violations(config: &Table, members: &[&str]) -> Vec<Violation> {
    let Some(packages) = config.get("package") else {
        return vec![no_main_entry()];
    };
    let Some(packages) = packages.as_array() else {
        return vec![malformed(
            "`package` isn't an array of `[[package]]` tables",
        )];
    };
    let mut violations = Vec::new();
    let mut has_main = false;
    for package in packages {
        let Some(package) = package.as_table() else {
            violations.push(malformed("a `package` entry isn't a `[[package]]` table"));
            continue;
        };
        if package.get("changelog_update").and_then(Value::as_bool) != Some(true) {
            continue;
        }
        has_main = true;
        let Some(main) = package.get("name").and_then(Value::as_str) else {
            violations.push(malformed(
                "the `[[package]]` entry with `changelog_update = true` has no string `name`",
            ));
            continue;
        };
        let empty = Value::Array(Vec::new());
        let value = package.get("changelog_include").unwrap_or(&empty);
        let Some(included) = crate_names(value) else {
            violations.push(malformed(&format!(
                "`changelog_include` of the `{main}` entry is {}, not an array of crate names",
                toml_file::render(value)
            )));
            continue;
        };
        for member in members {
            if *member == main || included.contains(member) {
                continue;
            }
            violations.push(Violation::new(
                RELEASE_PLZ.to_owned(),
                format!(
                    "`{member}` is a workspace member that `changelog_include` of the `{main}` \
                     entry in `{RELEASE_PLZ}` doesn't list, so its commits don't reach \
                     `CHANGELOG.md`. Add `{member}` to `changelog_include` in `{RELEASE_PLZ}` in \
                     the rule change that allows the crate."
                ),
            ));
        }
    }
    if !has_main {
        violations.push(no_main_entry());
    }
    violations
}

/// The violation for a `release-plz.toml` with no `[[package]]` entry that keeps `CHANGELOG.md`.
fn no_main_entry() -> Violation {
    Violation::new(
        RELEASE_PLZ.to_owned(),
        format!(
            "`{RELEASE_PLZ}` has no `[[package]]` entry with `changelog_update = true`, so no \
             release gets notes. Give the main package that entry, with `changelog_include` \
             listing every other crate."
        ),
    )
}

/// The strings in `value`, if it is an array of strings.
fn crate_names(value: &Value) -> Option<Vec<&str>> {
    value.as_array()?.iter().map(Value::as_str).collect()
}

/// A violation for a `release-plz.toml` whose shape the rule can't read, which `problem` says.
fn malformed(problem: &str) -> Violation {
    Violation::new(
        RELEASE_PLZ.to_owned(),
        format!("{problem}, so the check can't see which crates the changelog takes in. Fix it."),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const MEMBERS: [&str; 3] = ["demo", "demo_core", "demo_cli"];

    fn check_text(text: &str) -> Vec<String> {
        let config = toml_file::parse(text).expect("the fixture should parse");
        violations(&config, &MEMBERS)
            .into_iter()
            .map(|violation| violation.message().to_owned())
            .collect()
    }

    #[test]
    fn every_member_listed_passes() {
        let messages = check_text(
            "[workspace]\nchangelog_update = false\n\n[[package]]\nname = \"demo\"\n\
             changelog_update = true\nchangelog_include = [\"demo_core\", \"demo_cli\"]\n",
        );
        assert!(
            messages.is_empty(),
            "the main package needn't list itself: {messages:?}"
        );
    }

    #[test]
    fn a_missing_member_fails() {
        let messages = check_text(
            "[[package]]\nname = \"demo\"\nchangelog_update = true\n\
             changelog_include = [\"demo_core\"]\n",
        );
        assert_eq!(
            messages.len(),
            1,
            "only `demo_cli` is missing: {messages:?}"
        );
        assert!(
            messages[0].starts_with("`demo_cli` is a workspace member")
                && messages[0]
                    .contains("Add `demo_cli` to `changelog_include` in `release-plz.toml`"),
            "the message should name the crate and the fix: {messages:?}"
        );
    }

    #[test]
    fn names_that_arent_members_yet_are_allowed() {
        let messages = check_text(
            "[[package]]\nname = \"demo\"\nchangelog_update = true\n\
             changelog_include = [\"demo_core\", \"demo_cli\", \"demo_server\"]\n",
        );
        assert!(
            messages.is_empty(),
            "a crate the rule change allows before its code lands passes: {messages:?}"
        );
    }

    #[test]
    fn without_an_include_list_every_other_member_is_missing() {
        let messages = check_text("[[package]]\nname = \"demo\"\nchangelog_update = true\n");
        assert_eq!(messages.len(), 2, "both other members: {messages:?}");
    }

    #[test]
    fn a_config_without_a_main_entry_fails() {
        for text in [
            "[workspace]\ngit_only = true\n",
            "[workspace]\nchangelog_update = true\n\n[[package]]\nname = \"demo\"\n\
             changelog_path = \"./CHANGELOG.md\"\n",
            "[[package]]\nname = \"demo\"\nchangelog_update = false\n",
        ] {
            let messages = check_text(text);
            assert_eq!(
                messages.len(),
                1,
                "one violation for {text:?}: {messages:?}"
            );
            assert!(
                messages[0].contains("no `[[package]]` entry with `changelog_update = true`"),
                "{text:?} has no main entry: {messages:?}"
            );
        }
    }

    #[test]
    fn a_malformed_include_list_fails() {
        let messages = check_text(
            "[[package]]\nname = \"demo\"\nchangelog_update = true\nchangelog_include = \"demo_core\"\n",
        );
        assert_eq!(messages.len(), 1, "one malformed entry: {messages:?}");
        assert!(
            messages[0].contains("not an array of crate names"),
            "the message should say what is wrong: {messages:?}"
        );
    }
}
