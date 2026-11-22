//! A member's `Cargo.toml` names the workspace crates it depends on without a version, which
//! release-plz would rewrite in it.

use std::path::{Component, Path, PathBuf};

use toml::{Table, Value};

use crate::toml_file;
use crate::violation::Violation;
use crate::workspace::{Workspace, WorkspaceMember};

/// Checks that no member's `Cargo.toml` gives a path dependency on another member a `version` in
/// `[dependencies]` or `[dev-dependencies]`, a `[target.<platform>]` one or a renamed dependency
/// included.
///
/// release-plz rewrites that `version` whenever it bumps the dependency's version, whether or
/// not either crate is published, so the release pull request changes that part of the member's
/// `Cargo.toml`, which is code, beside the version in the root `Cargo.toml`, which is a rule, and
/// fails "Rule changes stand alone". It bumps nothing at a project's first release, so without
/// this check the project finds out at its second. `cargo add --path` writes the key for a
/// normal dependency.
///
/// Cargo reads the `version` of a path dependency only to publish the crate that has it, and
/// leaves a dev-dependency without one out of the published package, so the fix is to remove it,
/// unless both crates are published and it is a normal dependency. Then crates.io needs the
/// `version`, and it moves to `[workspace.dependencies]` of the root `Cargo.toml` beside the
/// `path`, where release-plz's rewrite changes no rule and no code. A `[build-dependencies]`
/// entry is a rule, as the root's `version` is, so release-plz's rewrite of one mixes nothing,
/// and the check leaves it alone.
#[must_use]
pub fn check(workspace: &Workspace) -> Vec<Violation> {
    let metadata = workspace.metadata();
    let publishes = |member: &WorkspaceMember| {
        metadata[member.id()]
            .publish
            .as_ref()
            .is_none_or(|registries| !registries.is_empty())
    };
    let members: Vec<(PathBuf, &WorkspaceMember)> = workspace
        .members()
        .iter()
        .map(|member| (normalize(member.dir()), member))
        .collect();
    let mut violations = Vec::new();
    for member in workspace.members() {
        let file = workspace.relative(member.manifest_path());
        for dependency in versioned_path_dependencies(member.manifest()) {
            let target = normalize(&member.dir().join(&dependency.path));
            let Some((_, target)) = members.iter().find(|(dir, _)| *dir == target) else {
                continue;
            };
            let unpublished = if !publishes(member) {
                Some(member.name())
            } else if !publishes(target) {
                Some(target.name())
            } else {
                None
            };
            violations.push(Violation::new(
                file.clone(),
                dependency.problem(target.name(), unpublished),
            ));
        }
    }
    violations
}

/// The kind of dependency a table holds, of the two kinds the check reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// `[dependencies]`, which a published package keeps.
    Normal,
    /// `[dev-dependencies]`, which a published package keeps only with a `version`.
    Dev,
}

/// A dependency that a member's `Cargo.toml` gives both a `path` and a `version`.
#[derive(Debug, PartialEq)]
struct VersionedPathDependency {
    /// The table it is in, written as TOML, such as `[target."cfg(unix)".dev-dependencies]`.
    table: String,
    /// The kind of dependency that table holds.
    kind: Kind,
    /// Its key in that table, which `package` may rename.
    key: String,
    /// Its `path`, relative to the member's directory.
    path: String,
    /// Its entry without the `version`, written as TOML on one line.
    without_version: String,
}

impl VersionedPathDependency {
    /// What is wrong with the dependency, which reaches the member `target`. `unpublished` is the
    /// crate, the member that has the dependency or `target`, that isn't published, if one isn't.
    fn problem(&self, target: &str, unpublished: Option<&str>) -> String {
        let Self {
            table,
            kind,
            key,
            without_version,
            ..
        } = self;
        let fix = match unpublished {
            Some(crate_name) => format!(
                "`{crate_name}` isn't published, so cargo never needs the `version`: remove it and \
                 keep the `path`, as `{key} = {without_version}`"
            ),
            None if *kind == Kind::Dev => format!(
                "Cargo leaves a dev-dependency without a `version` out of the published package, so \
                 it needs none: remove it and keep the `path`, as `{key} = {without_version}`"
            ),
            None => format!(
                "Both crates are published, so crates.io needs the `version`: move the entry, \
                 `path` and `version` both, to `[workspace.dependencies]` of the root Cargo.toml, \
                 where release-plz's rewrite changes neither rules nor code, and write \
                 `{key}.workspace = true` here"
            ),
        };
        format!(
            "`{table} {key}` gives the workspace crate `{target}` a `version` beside its `path`. \
             release-plz rewrites that `version` whenever it bumps `{target}`, so the release \
             pull request would change this Cargo.toml, which is code, beside the version in the \
             root Cargo.toml, which is a rule, and fail \"Rule changes stand alone\". {fix}, in a \
             code pull request. `cargo add --path` writes the `version` of a normal dependency, \
             so fix it after that too."
        )
    }
}

/// The dependencies that the member manifest `manifest` gives both a `path` and a `version`, in
/// `[dependencies]`, `[dev-dependencies]` and each `[target.<platform>]` table's.
fn versioned_path_dependencies(manifest: &Table) -> Vec<VersionedPathDependency> {
    let mut tables: Vec<(String, Kind, &Value)> = Vec::new();
    let names = [
        ("dependencies", Kind::Normal),
        ("dev-dependencies", Kind::Dev),
    ];
    for (name, kind) in names {
        if let Some(table) = manifest.get(name) {
            tables.push((format!("[{name}]"), kind, table));
        }
    }
    let platforms = manifest.get("target").and_then(Value::as_table);
    for (platform, settings) in platforms.into_iter().flatten() {
        for (name, kind) in names {
            if let Some(table) = settings.get(name) {
                let platform = toml_file::render_key(platform);
                tables.push((format!("[target.{platform}.{name}]"), kind, table));
            }
        }
    }
    let mut dependencies = Vec::new();
    for (table, kind, entries) in tables {
        for (key, entry) in entries.as_table().into_iter().flatten() {
            let Some(entry) = entry.as_table() else {
                continue;
            };
            let Some(path) = entry.get("path").and_then(Value::as_str) else {
                continue;
            };
            if !entry.contains_key("version") {
                continue;
            }
            let mut without_version = entry.clone();
            without_version.remove("version");
            dependencies.push(VersionedPathDependency {
                table: table.clone(),
                kind,
                key: key.clone(),
                path: path.to_owned(),
                without_version: toml_file::render(&Value::Table(without_version)),
            });
        }
    }
    dependencies
}

/// `path` with each `.` dropped and each `..` taking off the part before it, as Cargo reads a
/// dependency's `path`, without reading the file system.
fn normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other),
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dependencies_in(text: &str) -> Vec<VersionedPathDependency> {
        versioned_path_dependencies(&toml_file::parse(text).unwrap())
    }

    fn dependency(table: &str, kind: Kind) -> VersionedPathDependency {
        VersionedPathDependency {
            table: table.to_owned(),
            kind,
            key: "core".to_owned(),
            path: "../core".to_owned(),
            without_version: "{ path = \"../core\" }".to_owned(),
        }
    }

    #[test]
    fn a_path_dependency_with_a_version_is_found_in_normal_and_dev_tables() {
        let text = "[dependencies]\ncore = { version = \"0.1.0\", path = \"../core\" }\n\
                    [dev-dependencies]\ncore = { path = \"../core\", version = \"0.1\" }\n\
                    [target.'cfg(unix)'.dependencies]\ncore = { path = \"../core\", version = \"1\" }\n\
                    [target.x86_64-pc-windows-msvc.dev-dependencies]\n\
                    core = { path = \"../core\", version = \"1\" }\n";
        assert_eq!(
            dependencies_in(text),
            [
                dependency("[dependencies]", Kind::Normal),
                dependency("[dev-dependencies]", Kind::Dev),
                dependency("[target.\"cfg(unix)\".dependencies]", Kind::Normal),
                dependency(
                    "[target.x86_64-pc-windows-msvc.dev-dependencies]",
                    Kind::Dev
                ),
            ],
            "release-plz rewrites the `version` in each of these tables, which are code"
        );
    }

    #[test]
    fn a_build_dependency_is_left_alone() {
        let text = "[build-dependencies]\ncore = { version = \"0.1.0\", path = \"../core\" }\n\
                    [target.'cfg(unix)'.build-dependencies]\n\
                    core = { path = \"../core\", version = \"1\" }\n";
        assert_eq!(
            dependencies_in(text),
            [],
            "`[build-dependencies]` is a rule, so its rewrite beside the root's version mixes \
             nothing"
        );
    }

    #[test]
    fn a_renamed_dependency_keeps_its_key_and_drops_only_the_version() {
        let found = dependencies_in(
            "[dependencies]\nserver = { package = \"food_server\", version = \"0.1.0\", \
             path = \"../food_server\", features = [\"x\"] }\n",
        );
        assert_eq!(
            found,
            [VersionedPathDependency {
                table: "[dependencies]".to_owned(),
                kind: Kind::Normal,
                key: "server".to_owned(),
                path: "../food_server".to_owned(),
                without_version: "{ features = [\"x\"], package = \"food_server\", \
                                  path = \"../food_server\" }"
                    .to_owned(),
            }],
            "the fix keeps every other key of the entry"
        );
    }

    #[test]
    fn dependencies_without_both_a_path_and_a_version_are_left_alone() {
        let found = dependencies_in(
            "[dependencies]\ncore = { path = \"../core\" }\nserde = \"1\"\n\
             tokio = { version = \"1\", features = [\"rt\"] }\nshared.workspace = true\n\
             remote = { git = \"https://example.com/remote\", version = \"1\" }\n",
        );
        assert_eq!(
            found,
            [],
            "only an entry with both keys gets rewritten in the member's Cargo.toml"
        );
    }

    #[test]
    fn a_dotted_entry_counts_like_an_inline_one() {
        let found = dependencies_in(
            "[dependencies.core]\npath = \"../core\"\nversion = \"0.1.0\"\n\
             [dependencies]\nother.path = \"../other\"\nother.version = \"0.1.0\"\n",
        );
        let keys: Vec<&str> = found
            .iter()
            .map(|dependency| dependency.key.as_str())
            .collect();
        assert_eq!(
            keys,
            ["core", "other"],
            "TOML reads a table header or dotted keys as the same entry"
        );
    }

    #[test]
    fn normalize_resolves_dots_without_the_file_system() {
        assert_eq!(
            normalize(Path::new("/work/crates/app/../core/./")),
            PathBuf::from("/work/crates/core"),
            "`..` takes off the part before it and `.` is dropped"
        );
        assert_eq!(
            normalize(Path::new("/work/crates/app")),
            PathBuf::from("/work/crates/app"),
            "a plain path stays as it is"
        );
    }

    #[test]
    fn the_message_names_the_fix_for_each_case() {
        let normal = dependency("[dependencies]", Kind::Normal);
        let dev = dependency("[dev-dependencies]", Kind::Dev);
        let remove = "remove it and keep the `path`, as `core = { path = \"../core\" }`, in a code \
                      pull request";
        for (problem, texts) in [
            (
                normal.problem("core", Some("app")),
                vec![
                    "`[dependencies] core` gives the workspace crate `core` a `version`",
                    "\"Rule changes stand alone\"",
                    "`app` isn't published",
                    remove,
                ],
            ),
            (
                dev.problem("core", None),
                vec!["leaves a dev-dependency without a `version` out", remove],
            ),
            (
                normal.problem("core", None),
                vec![
                    "move the entry, `path` and `version` both, to `[workspace.dependencies]`",
                    "write `core.workspace = true` here, in a code pull request",
                ],
            ),
        ] {
            for text in texts {
                assert!(
                    problem.contains(text),
                    "the message should contain {text:?}: {problem}"
                );
            }
        }
    }
}
