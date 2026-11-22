//! The changelog of the main package takes in the commits of every other workspace crate, and
//! release-plz writes no changelog that is code.

use std::fs;
use std::io::ErrorKind;

use toml::{Table, Value};

use crate::policy_error::PolicyError;
use crate::violation::Violation;
use crate::workspace::{Workspace, WorkspaceMember};
use crate::{changed_files, toml_file};

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
/// It also checks that release-plz writes no member's changelog to a file that
/// `trellis-policy rule-changes` counts as code, such as `crates/<crate>/CHANGELOG.md`. release-plz
/// updates a package's changelog unless `changelog_update` is `false` in its `[[package]]` entry
/// or, without one there, in `[workspace]`, and writes it to `changelog_path`, from the entry or
/// `[workspace]`, or else beside the package's `Cargo.toml`. Such a file would change in the
/// release pull request beside the version in the root `Cargo.toml`, a rule, and fail "Rule
/// changes stand alone".
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
    let mut violations = violations(&config, &members);
    let directories: Vec<CrateDirectory<'_>> = workspace
        .members()
        .iter()
        .map(|member| CrateDirectory {
            name: member.name(),
            directory: workspace.relative(member.dir()),
        })
        .collect();
    violations.extend(changelogs_in_code(&config, &directories));
    Ok(violations)
}

/// A workspace member and where it is.
#[derive(Debug)]
struct CrateDirectory<'workspace> {
    /// The package name.
    name: &'workspace str,
    /// Its directory, relative to the workspace root with `/` separators.
    directory: String,
}

/// The violations by the parsed `release-plz.toml` `config` of a changelog that release-plz would
/// write to a file that is code, in a workspace whose members are `members`.
fn changelogs_in_code(config: &Table, members: &[CrateDirectory<'_>]) -> Vec<Violation> {
    let workspace = config.get("workspace").and_then(Value::as_table);
    let packages = config.get("package").and_then(Value::as_array);
    let mut violations = Vec::new();
    for CrateDirectory { name, directory } in members {
        let entry = packages
            .into_iter()
            .flatten()
            .filter_map(Value::as_table)
            .find(|package| package.get("name").and_then(Value::as_str) == Some(name));
        let setting = |key: &str| {
            let from_entry = entry.and_then(|entry| entry.get(key));
            from_entry
                .map(|value| (value, "its `[[package]]` entry"))
                .or_else(|| {
                    let value = workspace.and_then(|workspace| workspace.get(key))?;
                    Some((value, "`[workspace]`"))
                })
        };
        let updated = setting("changelog_update");
        if updated.is_some_and(|(value, _)| value.as_bool() == Some(false)) {
            continue;
        }
        let file = match setting("changelog_path") {
            Some((value, _)) => match value.as_str().and_then(repository_path) {
                Some(file) => file,
                None => continue,
            },
            None if directory.is_empty() => "CHANGELOG.md".to_owned(),
            None => format!("{directory}/CHANGELOG.md"),
        };
        if !changed_files::is_code(&file) {
            continue;
        }
        let why = match updated {
            Some((_, place)) => format!("`changelog_update = true` in {place}"),
            None => "no `changelog_update = false` in its `[[package]]` entry or in \
                     `[workspace]`, and release-plz updates a changelog unless it is `false`"
                .to_owned(),
        };
        violations.push(Violation::new(
            RELEASE_PLZ.to_owned(),
            format!(
                "release-plz would write the changelog of `{name}` to `{file}`, which is code, \
                 since `{RELEASE_PLZ}` has {why}. That file would change in the release pull \
                 request beside the version in the root Cargo.toml, which is a rule, and fail \
                 \"Rule changes stand alone\". The project keeps one `CHANGELOG.md`, which the \
                 main package's entry writes: set `changelog_update = false` for `{name}`, in \
                 `[workspace]` or its `[[package]]` entry, in a rules pull request."
            ),
        ));
    }
    violations
}

/// `path`, a `changelog_path` that release-plz reads from the workspace root, relative to the
/// root with `/` separators and its `.` and `..` resolved, or `None` if it leaves the repository.
fn repository_path(path: &str) -> Option<String> {
    if path.starts_with(['/', '\\']) || path.contains(':') {
        return None;
    }
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            _ => parts.push(part),
        }
    }
    Some(parts.join("/"))
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

    fn changelog_messages(text: &str) -> Vec<String> {
        let config = toml_file::parse(text).expect("the fixture should parse");
        let members = ["demo", "demo_core"].map(|name| CrateDirectory {
            name,
            directory: format!("crates/{name}"),
        });
        changelogs_in_code(&config, &members)
            .into_iter()
            .map(|violation| violation.message().to_owned())
            .collect()
    }

    /// The main package's entry, as trellis writes it, after the `[workspace]` table.
    const MAIN: &str = "\n[[package]]\nname = \"demo\"\nchangelog_path = \"./CHANGELOG.md\"\n\
                        changelog_update = true\nchangelog_include = [\"demo_core\"]\n";

    #[test]
    fn one_changelog_at_the_root_passes() {
        for workspace in [
            "[workspace]\nchangelog_update = false\n",
            "[workspace]\nchangelog_update = true\nchangelog_path = \"CHANGELOG.md\"\n",
            "[workspace]\nchangelog_path = \"docs/changes.md\"\n",
        ] {
            let text = format!("{workspace}{MAIN}");
            let messages = changelog_messages(&text);
            assert!(
                messages.is_empty(),
                "a changelog at the root or in docs/ isn't code: {text:?} {messages:?}"
            );
        }
    }

    #[test]
    fn a_changelog_release_plz_would_write_into_a_crate_fails() {
        for (text, why) in [
            (
                format!("[workspace]\ngit_only = true\n{MAIN}"),
                "since `release-plz.toml` has no `changelog_update = false` in its `[[package]]` \
                 entry or in `[workspace]`",
            ),
            (
                format!(
                    "[workspace]\nchangelog_update = false\n{MAIN}\n[[package]]\n\
                     name = \"demo_core\"\nchangelog_update = true\n"
                ),
                "since `release-plz.toml` has `changelog_update = true` in its `[[package]]` entry",
            ),
            (
                format!("[workspace]\nchangelog_update = true\n{MAIN}"),
                "since `release-plz.toml` has `changelog_update = true` in `[workspace]`",
            ),
        ] {
            let messages = changelog_messages(&text);
            assert_eq!(
                messages.len(),
                1,
                "only `demo_core` writes into its crate: {text:?} {messages:?}"
            );
            for part in [
                "release-plz would write the changelog of `demo_core` to \
                 `crates/demo_core/CHANGELOG.md`, which is code",
                why,
                "set `changelog_update = false` for `demo_core`, in `[workspace]` or its \
                 `[[package]]` entry, in a rules pull request",
            ] {
                assert!(
                    messages[0].contains(part),
                    "the message should contain {part:?}: {messages:?}"
                );
            }
        }
    }

    #[test]
    fn a_changelog_path_into_a_crate_fails_wherever_it_is_set() {
        let messages = changelog_messages(&format!(
            "[workspace]\nchangelog_update = false\n{MAIN}\n[[package]]\nname = \"demo_core\"\n\
             changelog_update = true\nchangelog_path = \"./crates/demo/../demo/NOTES.md\"\n"
        ));
        assert_eq!(messages.len(), 1, "one changelog in a crate: {messages:?}");
        assert!(
            messages[0].contains("to `crates/demo/NOTES.md`, which is code"),
            "the path is read from the root with its dots resolved: {messages:?}"
        );
    }

    #[test]
    fn repository_path_resolves_dots_and_refuses_paths_outside() {
        for (path, expected) in [
            ("./CHANGELOG.md", Some("CHANGELOG.md")),
            ("crates/a/../b/CHANGELOG.md", Some("crates/b/CHANGELOG.md")),
            ("crates\\a\\CHANGELOG.md", Some("crates/a/CHANGELOG.md")),
            ("../CHANGELOG.md", None),
            ("/tmp/CHANGELOG.md", None),
            ("C:/CHANGELOG.md", None),
        ] {
            assert_eq!(
                repository_path(path).as_deref(),
                expected,
                "{path:?} from the root"
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
