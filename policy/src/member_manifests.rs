//! Each member's `Cargo.toml` holds only settings that keep the crate built, tested and checked
//! the way the rest of the workspace is.

use toml::{Table, Value};

use crate::violation::Violation;
use crate::workspace::Workspace;
use crate::{source_path, toml_file, upstream};

/// The top-level keys a member manifest may have. `[lints]` has its own check, and
/// `[package] build` is left to the build script check.
const TOP_LEVEL_KEYS: [&str; 12] = [
    "package",
    "lib",
    "bin",
    "test",
    "bench",
    "example",
    "dependencies",
    "dev-dependencies",
    "build-dependencies",
    "target",
    "features",
    "lints",
];

/// The keys a member's `[package]` table may have.
const PACKAGE_KEYS: [&str; 16] = [
    "name",
    "version",
    "description",
    "edition",
    "rust-version",
    "repository",
    "publish",
    "authors",
    "license",
    "readme",
    "homepage",
    "documentation",
    "keywords",
    "categories",
    "default-run",
    "build",
];

/// The `[package]` keys a member must inherit from `[workspace.package]`, each with why it must.
const INHERITED_PACKAGE_KEYS: [(&str, &str); 3] = [
    (
        "edition",
        "the edition decides the language and its lints, so every crate is built and linted with \
         the workspace's",
    ),
    (
        "rust-version",
        "clippy turns off the lints for APIs newer than `rust-version`, so every crate is linted \
         against the workspace's",
    ),
    (
        "publish",
        "a crate is published only when the workspace says so",
    ),
];

/// The keys a target table (`[lib]`, `[[bin]]`, `[[test]]`, `[[bench]]` or `[[example]]`) may
/// have. Everything else, such as `test = false`, `harness = false` or `required-features`, can
/// stop a target's tests from running or hide it from the checks.
const TARGET_KEYS: [&str; 2] = ["name", "path"];

/// The tables a `[target.<platform>]` table may have.
const PLATFORM_KEYS: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];

/// Checks that each member's `Cargo.toml` holds only the keys the workspace allows, so that no
/// setting turns off a crate's tests or doctests (`test = false`, `doctest = false`,
/// `harness = false`, `autotests = false`), builds code the checks don't see
/// (`required-features`, `proc-macro`), or reads sources from outside the crate's directory. The
/// crate's edition, `rust-version` and `publish` come from `[workspace.package]`, and every
/// target's `path`
/// stays inside the crate's directory.
#[must_use]
pub fn check(workspace: &Workspace) -> Vec<Violation> {
    let mut violations = Vec::new();
    for member in workspace.members() {
        let file = workspace.relative(member.manifest_path());
        for problem in problems(member.manifest()) {
            violations.push(Violation::new(file.clone(), problem));
        }
    }
    violations
}

/// What is wrong with the member manifest `manifest`.
fn problems(manifest: &Table) -> Vec<String> {
    let mut problems = Vec::new();
    for (key, value) in manifest {
        match key.as_str() {
            "package" => package_problems(value, &mut problems),
            "lib" => target_problems("[lib]", value, &mut problems),
            "bin" | "test" | "bench" | "example" => {
                let name = format!("[[{key}]]");
                for target in value.as_array().map_or(&[][..], Vec::as_slice) {
                    target_problems(&name, target, &mut problems);
                }
            }
            "target" => {
                let platforms = value.as_table().into_iter().flatten();
                for (platform, settings) in platforms {
                    let name = format!("[target.{}]", toml_file::render_key(platform));
                    unknown_keys(&name, settings, &PLATFORM_KEYS, &mut problems);
                }
            }
            _ if TOP_LEVEL_KEYS.contains(&key.as_str()) => {}
            _ => problems.push(unknown(&format!("`{key}`"))),
        }
    }
    problems
}

/// Adds what is wrong with the `[package]` table `package` to `problems`.
fn package_problems(package: &Value, problems: &mut Vec<String>) {
    unknown_keys("[package]", package, &PACKAGE_KEYS, problems);
    for (key, why) in INHERITED_PACKAGE_KEYS {
        let value = package.get(key);
        let inherits = value.and_then(Value::as_table).is_some_and(|table| {
            table.len() == 1 && table.get("workspace") == Some(&Value::Boolean(true))
        });
        if !inherits {
            let found = value.map_or_else(|| "missing".to_owned(), toml_file::render);
            problems.push(format!(
                "`[package] {key}` is {found}, but it must be `{key}.workspace = true`, because \
                 {why}. To change it, change `[workspace.package]` in the root Cargo.toml, in a \
                 pull request of its own."
            ));
        }
    }
}

/// Adds what is wrong with the target table `target`, written `name`, to `problems`.
fn target_problems(name: &str, target: &Value, problems: &mut Vec<String>) {
    unknown_keys(name, target, &TARGET_KEYS, problems);
    if let Some(path) = target.get("path").and_then(Value::as_str)
        && source_path::escapes(0, path)
    {
        problems.push(format!(
            "`{name} path = {path:?}` reads the target from outside the crate's directory. A \
             crate's sources live under its own directory, where a change to them counts as a \
             code change; move the file there."
        ));
    }
}

/// Adds a problem to `problems` for each key of the table `table`, written `name`, that `allowed`
/// doesn't list.
fn unknown_keys(name: &str, table: &Value, allowed: &[&str], problems: &mut Vec<String>) {
    let keys = table.as_table().into_iter().flat_map(Table::keys);
    for key in keys {
        if !allowed.contains(&key.as_str()) {
            problems.push(unknown(&format!("`{name} {key}`")));
        }
    }
}

/// The problem for the manifest key `key`, which isn't allowed.
fn unknown(key: &str) -> String {
    format!(
        "{key} isn't a setting a crate's Cargo.toml may have. Settings such as `test = false`, \
         `doctest = false`, `harness = false`, `autotests = false`, `required-features` or \
         `[package.metadata]` can stop a crate's tests from running or change what the checks \
         see, so a member's Cargo.toml holds only the keys trellis-policy allows. Remove it. {}",
        upstream::change_the_rule("policy/src/member_manifests.rs")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const PACKAGE: &str = "[package]\nname = \"app\"\nedition.workspace = true\n\
                           rust-version.workspace = true\npublish.workspace = true\n";

    fn problems_in(text: &str) -> Vec<String> {
        problems(&toml_file::parse(text).unwrap())
    }

    #[test]
    fn the_usual_manifest_passes() {
        let text = format!(
            "{PACKAGE}version.workspace = true\ndescription = \"An app\"\n\
             repository.workspace = true\n\
             [features]\nx = []\n[dependencies]\na.workspace = true\n\
             [target.'cfg(unix)'.dependencies]\nb = \"1\"\n[dev-dependencies]\nc = \"1\"\n\
             [lib]\npath = \"src/lib.rs\"\n[[bin]]\nname = \"app\"\npath = \"src/main.rs\"\n\
             [[test]]\nname = \"t\"\npath = \"tests/t.rs\"\n[lints]\nworkspace = true\n"
        );
        assert_eq!(
            problems_in(&text),
            Vec::<String>::new(),
            "ordinary settings should pass"
        );
    }

    #[test]
    fn settings_that_turn_off_tests_fail() {
        for (setting, key) in [
            ("[lib]\ntest = false\n", "`[lib] test`"),
            ("[lib]\ndoctest = false\n", "`[lib] doctest`"),
            ("[lib]\nproc-macro = true\n", "`[lib] proc-macro`"),
            (
                "[[test]]\nname = \"t\"\nharness = false\n",
                "`[[test]] harness`",
            ),
            ("[[test]]\nname = \"t\"\ntest = false\n", "`[[test]] test`"),
            (
                "[[bin]]\nname = \"b\"\nrequired-features = [\"x\"]\n",
                "`[[bin]] required-features`",
            ),
            (
                "[[bench]]\nname = \"b\"\nbench = false\n",
                "`[[bench]] bench`",
            ),
            ("autotests = false\n", "`[package] autotests`"),
            ("autolib = false\n", "`[package] autolib`"),
            ("autobins = false\n", "`[package] autobins`"),
            ("autoexamples = false\n", "`[package] autoexamples`"),
            ("autobenches = false\n", "`[package] autobenches`"),
            (
                "metadata.cargo-shear.ignored = [\"a\"]\n",
                "`[package] metadata`",
            ),
            ("workspace = \"../other\"\n", "`[package] workspace`"),
            ("[patch.crates-io]\na = { path = \"a\" }\n", "`patch`"),
            (
                "[target.'cfg(unix)'.lints]\nworkspace = true\n",
                "`[target.\"cfg(unix)\"] lints`",
            ),
        ] {
            let text = format!("{PACKAGE}{setting}");
            let problems = problems_in(&text);
            assert!(
                problems.iter().any(|problem| problem.starts_with(key)),
                "{setting:?} should be rejected as {key}, found {problems:?}"
            );
        }
    }

    #[test]
    fn target_paths_outside_the_crate_fail() {
        for setting in [
            "[lib]\npath = \"../other/lib.rs\"\n",
            "[[test]]\nname = \"t\"\npath = \"../../docs/t.rs\"\n",
            "[[bin]]\nname = \"b\"\npath = \"/tmp/main.rs\"\n",
        ] {
            let text = format!("{PACKAGE}{setting}");
            let problems = problems_in(&text);
            assert!(
                problems
                    .iter()
                    .any(|problem| problem.contains("outside the crate's directory")),
                "{setting:?} should be rejected, found {problems:?}"
            );
        }
    }

    #[test]
    fn a_package_key_of_its_own_instead_of_the_workspace_one_fails() {
        let edition = "edition.workspace = true\n";
        let rust_version = "rust-version.workspace = true\n";
        let publish = "publish.workspace = true\n";
        for (keys, key) in [
            (
                format!("edition = \"2015\"\n{rust_version}{publish}"),
                "`[package] edition`",
            ),
            (
                format!("{edition}rust-version = \"1.0\"\n{publish}"),
                "`[package] rust-version`",
            ),
            (
                format!("{edition}{rust_version}publish = true\n"),
                "`[package] publish`",
            ),
            (
                format!("{rust_version}{publish}"),
                "`[package] edition` is missing",
            ),
            (
                format!("{edition}{rust_version}"),
                "`[package] publish` is missing",
            ),
        ] {
            let text = format!("[package]\nname = \"app\"\n{keys}");
            let problems = problems_in(&text);
            assert!(
                problems.iter().any(|problem| problem.starts_with(key)),
                "{text:?} should be rejected as {key}, found {problems:?}"
            );
        }
    }
}
