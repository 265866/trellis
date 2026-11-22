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
/// release-plz rewrites that `version` when it bumps the dependency past what the requirement
/// allows, as it does `"0.1.0"` at any bump, whether or not either crate is published, so the
/// release pull request changes that part of the member's
/// `Cargo.toml`, which is code, beside the version in the root `Cargo.toml`, which is a rule, and
/// fails "Rule changes stand alone". It bumps nothing at a project's first release, so without
/// this check the project finds out at its second. `cargo add --path` writes the key for a
/// normal dependency.
///
/// Cargo reads the `version` of a path dependency only to publish the crate that has it, and
/// leaves a dev-dependency without one out of the published package, so the fix is to remove it,
/// unless both crates are published and it is a normal dependency. Then crates.io needs the
/// `version`, and it moves to `[workspace.dependencies]` of the root `Cargo.toml` beside the
/// `path`, where release-plz's rewrite changes no rule and no code, while `optional`, `features`
/// and `default-features` stay in the member beside `workspace = true`. A `[build-dependencies]`
/// entry is a rule, as the root's `version` is, so release-plz's rewrite of one mixes nothing,
/// and the check leaves it alone.
#[must_use]
pub fn check(workspace: &Workspace) -> Vec<Violation> {
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
            let unpublished = if !workspace.publishes(member) {
                Some(member.name())
            } else if !workspace.publishes(target) {
                Some(target.name())
            } else {
                None
            };
            violations.push(Violation::new(
                file.clone(),
                dependency.problem(
                    target.name(),
                    &workspace.relative(target.dir()),
                    unpublished,
                ),
            ));
        }
    }
    violations
}

/// The keys of a dependency that stay in the member when the rest of it moves to
/// `[workspace.dependencies]`: the ones cargo reads beside `workspace = true`. Cargo rejects a
/// workspace dependency that is `optional`, and ignores any other key beside `workspace = true`
/// with an "unused manifest key" warning.
const MEMBER_KEYS: [&str; 3] = ["default-features", "features", "optional"];

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
    /// Its entry, every key of it.
    entry: Table,
}

impl VersionedPathDependency {
    /// What is wrong with the dependency, which reaches the member `target` in the directory
    /// `target_dir`, relative to the workspace root. `unpublished` is the crate, the member that
    /// has the dependency or `target`, that isn't published, if one isn't.
    fn problem(&self, target: &str, target_dir: &str, unpublished: Option<&str>) -> String {
        let Self {
            table, kind, key, ..
        } = self;
        let key = toml_file::render_key(key);
        let fix = match unpublished {
            Some(crate_name) => format!(
                "`{crate_name}` isn't published, so cargo never needs the `version`: remove it and \
                 keep the `path`, as `{}`",
                self.without_version()
            ),
            None if *kind == Kind::Dev => format!(
                "Cargo leaves a dev-dependency without a `version` out of the published package, so \
                 it needs none: remove it and keep the `path`, as `{}`",
                self.without_version()
            ),
            None => {
                let WorkspaceDependency { root, member } = self.workspace_dependency(target_dir);
                format!(
                    "Both crates are published, so crates.io needs the `version`: give it in \
                     `[workspace.dependencies]` of the root Cargo.toml, where release-plz's \
                     rewrite changes neither rules nor code, as `{root}`, with the `path` from \
                     the root, and write `{member}` here, where `optional`, `features` and \
                     `default-features` stay"
                )
            }
        };
        format!(
            "`{table} {key}` gives the workspace crate `{target}` a `version` beside its `path`. \
             release-plz rewrites that `version` when it bumps `{target}` past what the \
             requirement allows, so the release pull request would change this Cargo.toml, which \
             is code, beside the version in the \
             root Cargo.toml, which is a rule, and fail \"Rule changes stand alone\". {fix}, in a \
             code pull request. `cargo add --path` writes the `version` of a normal dependency, \
             so fix it after that too."
        )
    }

    /// The dependency's line without its `version`, such as `core = { path = "../core" }`.
    fn without_version(&self) -> String {
        let mut entry = self.entry.clone();
        entry.remove("version");
        format!(
            "{} = {}",
            toml_file::render_key(&self.key),
            toml_file::render(&Value::Table(entry))
        )
    }

    /// The dependency split into its line in `[workspace.dependencies]` of the root `Cargo.toml`,
    /// with its `path` from the root, `target_dir`, and every key but the [`MEMBER_KEYS`], and its
    /// line in the member, `workspace = true` with those keys.
    fn workspace_dependency(&self, target_dir: &str) -> WorkspaceDependency {
        let key = toml_file::render_key(&self.key);
        let mut root = self.entry.clone();
        root.insert("path".to_owned(), Value::String(target_dir.to_owned()));
        let mut member = Vec::new();
        for name in MEMBER_KEYS {
            if let Some(value) = root.remove(name) {
                member.push(format!("{name} = {}", toml_file::render(&value)));
            }
        }
        let root = format!("{key} = {}", toml_file::render(&Value::Table(root)));
        let member = if member.is_empty() {
            format!("{key}.workspace = true")
        } else {
            format!("{key} = {{ workspace = true, {} }}", member.join(", "))
        };
        WorkspaceDependency { root, member }
    }
}

/// A dependency on a published workspace crate, written as `[workspace.dependencies]` takes it.
#[derive(Debug, PartialEq)]
struct WorkspaceDependency {
    /// Its line in `[workspace.dependencies]` of the root `Cargo.toml`.
    root: String,
    /// Its line in the member's `Cargo.toml`.
    member: String,
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
            dependencies.push(VersionedPathDependency {
                table: table.clone(),
                kind,
                key: key.clone(),
                path: path.to_owned(),
                entry: entry.clone(),
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

    fn dependency(table: &str, kind: Kind, version: &str) -> VersionedPathDependency {
        let entry = format!("path = \"../core\"\nversion = \"{version}\"\n");
        VersionedPathDependency {
            table: table.to_owned(),
            kind,
            key: "core".to_owned(),
            path: "../core".to_owned(),
            entry: toml_file::parse(&entry).unwrap(),
        }
    }

    fn only(text: &str) -> VersionedPathDependency {
        let mut found = dependencies_in(text);
        assert_eq!(
            found.len(),
            1,
            "the manifest has one such dependency: {found:?}"
        );
        found.remove(0)
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
                dependency("[dependencies]", Kind::Normal, "0.1.0"),
                dependency("[dev-dependencies]", Kind::Dev, "0.1"),
                dependency("[target.\"cfg(unix)\".dependencies]", Kind::Normal, "1"),
                dependency(
                    "[target.x86_64-pc-windows-msvc.dev-dependencies]",
                    Kind::Dev,
                    "1"
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
        let found = only(
            "[dependencies]\nserver = { package = \"food_server\", version = \"0.1.0\", \
             path = \"../food_server\", features = [\"x\"] }\n",
        );
        assert_eq!(
            (found.key.as_str(), found.path.as_str()),
            ("server", "../food_server"),
            "the dependency keeps its key, and its path names the crate"
        );
        assert_eq!(
            found.without_version(),
            "server = { features = [\"x\"], package = \"food_server\", path = \"../food_server\" }",
            "the fix keeps every other key of the entry"
        );
    }

    #[test]
    fn a_plain_dependency_moves_whole_to_the_workspace_with_its_path_from_the_root() {
        let found = only("[dependencies]\ncore = { version = \"0.3.1\", path = \"../core\" }\n");
        assert_eq!(
            found.workspace_dependency("crates/core"),
            WorkspaceDependency {
                root: "core = { path = \"crates/core\", version = \"0.3.1\" }".to_owned(),
                member: "core.workspace = true".to_owned(),
            },
            "the root reads the path from its own directory, and the member keeps nothing else"
        );
    }

    #[test]
    fn a_renamed_dependency_keeps_its_package_in_the_workspace_entry() {
        let found = only(
            "[dependencies]\nserver = { package = \"food_server\", version = \"0.1.0\", \
             path = \"../food_server\" }\n",
        );
        assert_eq!(
            found.workspace_dependency("crates/food_server"),
            WorkspaceDependency {
                root: "server = { package = \"food_server\", path = \"crates/food_server\", \
                       version = \"0.1.0\" }"
                    .to_owned(),
                member: "server.workspace = true".to_owned(),
            },
            "cargo ignores `package` beside `workspace = true`, so the rename moves to the root \
             under the member's key"
        );
    }

    #[test]
    fn optional_features_and_default_features_stay_in_the_member() {
        let found = only(
            "[dependencies]\ncore = { version = \"0.1.0\", path = \"../core\", optional = true, \
             features = [\"extra\"], default-features = false }\n",
        );
        assert_eq!(
            found.workspace_dependency("crates/core"),
            WorkspaceDependency {
                root: "core = { path = \"crates/core\", version = \"0.1.0\" }".to_owned(),
                member: "core = { workspace = true, default-features = false, \
                         features = [\"extra\"], optional = true }"
                    .to_owned(),
            },
            "cargo rejects an `optional` workspace dependency, and reads these keys beside \
             `workspace = true`"
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
        let normal = dependency("[dependencies]", Kind::Normal, "0.1.0");
        let dev = dependency("[dev-dependencies]", Kind::Dev, "0.1.0");
        let remove = "remove it and keep the `path`, as `core = { path = \"../core\" }`, in a code \
                      pull request";
        for (problem, texts) in [
            (
                normal.problem("core", "crates/core", Some("app")),
                vec![
                    "`[dependencies] core` gives the workspace crate `core` a `version`",
                    "when it bumps `core` past what the requirement allows",
                    "\"Rule changes stand alone\"",
                    "`app` isn't published",
                    remove,
                ],
            ),
            (
                dev.problem("core", "crates/core", None),
                vec!["leaves a dev-dependency without a `version` out", remove],
            ),
            (
                normal.problem("core", "crates/core", None),
                vec![
                    "give it in `[workspace.dependencies]` of the root Cargo.toml",
                    "as `core = { path = \"crates/core\", version = \"0.1.0\" }`, with the `path` \
                     from the root",
                    "write `core.workspace = true` here",
                    "in a code pull request",
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
