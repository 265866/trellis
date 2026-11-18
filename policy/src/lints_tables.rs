//! Every member compiles with the workspace's lint levels and no others.

use toml::{Table, Value};

use crate::toml_file;
use crate::violation::Violation;
use crate::workspace::Workspace;

/// Checks that every member's `Cargo.toml` has a `[lints]` table that is exactly
/// `workspace = true`, so each crate compiles with the lint levels in `[workspace.lints]` of the
/// root `Cargo.toml` and no others.
#[must_use]
pub fn check(workspace: &Workspace) -> Vec<Violation> {
    let mut violations = Vec::new();
    for member in workspace.members() {
        if let Some(problem) = problem(member.manifest()) {
            let file = workspace.relative(member.manifest_path());
            violations.push(Violation::new(file, problem));
        }
    }
    violations
}

/// What is wrong with the `[lints]` table of the member manifest `manifest`, if anything.
fn problem(manifest: &Table) -> Option<String> {
    let Some(lints) = manifest.get("lints") else {
        return Some(
            "the crate has no `[lints]` table, so it compiles without the workspace's lint \
             levels. Add a `[lints]` table with exactly `workspace = true`."
                .to_owned(),
        );
    };
    let inherits_only = lints.as_table().is_some_and(|table| {
        table.len() == 1 && table.get("workspace") == Some(&Value::Boolean(true))
    });
    if inherits_only {
        return None;
    }
    Some(format!(
        "`[lints]` is `{}`, but it must be exactly `workspace = true`, so the crate compiles with \
         the lint levels in `[workspace.lints]` of the root Cargo.toml and no others. Remove the \
         other keys. To change a lint level, change it in the root Cargo.toml for every crate, in \
         a pull request of its own.",
        toml_file::render(lints)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn problem_in(text: &str) -> Option<String> {
        problem(&toml_file::parse(text).unwrap())
    }

    #[test]
    fn inheriting_the_workspace_lints_passes() {
        assert_eq!(
            problem_in("[lints]\nworkspace = true\n"),
            None,
            "`workspace = true` alone is the required table"
        );
        assert_eq!(
            problem_in("lints = { workspace = true }\n"),
            None,
            "an inline table is the same table"
        );
    }

    #[test]
    fn a_missing_table_fails() {
        let problem = problem_in("[package]\nname = \"a\"\n").unwrap();
        assert!(
            problem.contains("no `[lints]` table"),
            "a crate without `[lints]` should fail: {problem}"
        );
    }

    #[test]
    fn extra_keys_or_a_false_workspace_fail() {
        for text in [
            "[lints.clippy]\npedantic = \"allow\"\n",
            "[lints]\nworkspace = false\n",
            "[lints]\nworkspace = true\n[lints.rust]\nmissing_docs = \"allow\"\n",
            "lints = true\n",
        ] {
            let problem = problem_in(text);
            assert!(
                problem
                    .as_deref()
                    .is_some_and(|problem| problem.contains("exactly `workspace = true`")),
                "{text:?} should fail, found {problem:?}"
            );
        }
    }
}
