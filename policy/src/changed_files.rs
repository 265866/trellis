//! Which files a branch changes are rules, and which are the code the rules check.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use toml::{Table, Value};

use crate::head::Head;
use crate::policy_error::PolicyError;
use crate::tool_config::ToolConfig;
use crate::{agent_files, case_insensitive, git, lint_expectations, source_path, toml_file};

/// Directories whose every file is a rule: the workflows.
const RULE_DIRECTORIES: [&str; 1] = [".github/"];

/// File names that are rules in any directory: the check recipes, and the settings files tools
/// read from the directory they check and its ancestors. An ignore file hides files from git,
/// `typos` and the checks that list the repository's files, and the TypeScript, Bun and npm
/// settings change what a web UI's check and install do.
const RULE_NAMES: [&str; 7] = [
    "justfile",
    ".justfile",
    ".gitignore",
    ".ignore",
    "tsconfig.json",
    "bunfig.toml",
    ".npmrc",
];

/// The extensions of the images that the pages under `docs/` show, which are neither rules nor
/// code, like the pages.
const DOCS_IMAGE_EXTENSIONS: [&str; 6] = ["png", "svg", "jpg", "jpeg", "gif", "webp"];

/// What the listing adds to an untracked file that is a rule only because no list knows it, and
/// that no diff lists, so ignoring it takes it out of the change.
const UNTRACKED_REMEDY: &str =
    "(untracked; commit it in a rules pull request, or ignore it in .git/info/exclude)";

/// Rule files that aren't tool configuration or agent instruction files: the documents that hold
/// the rules no check decides and the review checklist, and the template answers that
/// `copier update` regenerates the rules from.
const RULE_FILES: [&str; 3] = ["CONTRIBUTING.md", "docs/style.md", ".copier-answers.yml"];

/// The files, besides the licenses and the Markdown files under `docs/` other than
/// `docs/style.md`, that are neither rules nor code. The docs checks read the readme and the
/// pages, but nothing they say is a rule. `Cargo.lock` changes with every dependency, and
/// `cargo deny` checks what it pins. These names match exactly and win over the tool
/// configuration names, so the root `.editorconfig` is neither while a nested one is a rule.
const OTHER_FILES: [&str; 5] = [
    "README.md",
    "Cargo.lock",
    ".editorconfig",
    "CHANGELOG.md",
    "SECURITY.md",
];

/// The files a branch changes, split into rules and code. The rules are the tool configuration
/// files, such as the lint, format and dependency settings, the ignore files and a web UI's
/// TypeScript, Bun and npm settings, the build scripts, including the file a member's `build` key
/// names at either end, the check recipes, the workflows, `CONTRIBUTING.md`, the template answers,
/// every agent instruction file, the root `Cargo.toml` outside its dependency tables, the part of
/// a member `Cargo.toml` that holds the build script's settings and the license, and any file the
/// lists here don't know, which the listing marks when it is only untracked; the code is every
/// other change under `crates/` and every Rust file elsewhere, except a Rust file whose change
/// only adds, removes, moves or changes lint expectations, which is neither. A pull request
/// changes one or the other, so review sees every loosened rule apart from the code that might
/// have needed it.
#[derive(Debug, Default)]
pub struct ChangedFiles {
    /// The changed rule files.
    rules: Vec<String>,
    /// The changed code files.
    code: Vec<String>,
}

impl ChangedFiles {
    /// The files changed from the merge base of `base` and `head` to `head`, read from the git
    /// repository that holds `repository`. A rename counts as both of its paths.
    ///
    /// # Errors
    ///
    /// Fails if git fails, such as when `base` or `head` isn't a revision of the repository.
    pub fn between(repository: &Path, base: &str, head: &Head) -> Result<Self, PolicyError> {
        let top = git::run(repository, &["rev-parse", "--show-toplevel"])?;
        let top = PathBuf::from(top.trim_end_matches(['\r', '\n']));
        let head_revision = match head {
            Head::WorkingTree => "HEAD",
            Head::Commit(revision) => revision.as_str(),
        };
        let merge_base = git::run(&top, &["merge-base", base, head_revision])?;
        let merge_base = merge_base.trim();
        let ChangedPaths { paths, untracked } = ChangedPaths::between(&top, merge_base, head)?;
        let head_commit = match head {
            Head::WorkingTree => None,
            Head::Commit(revision) => Some(revision.as_str()),
        };
        let mut build_scripts = named_build_scripts(&top, Some(merge_base))?;
        build_scripts.extend(named_build_scripts(&top, head_commit)?);
        let mut changed = Self::default();
        let mut nesting = BTreeMap::new();
        let versions = |path: &str| {
            let before = show(&top, merge_base, path);
            let after = match head {
                Head::WorkingTree => fs::read_to_string(top.join(path)).ok(),
                Head::Commit(revision) => show(&top, revision, path),
            };
            (before, after)
        };
        for path in paths {
            let role = match role(&path) {
                Role::Code if is_named_build_script(&path) => {
                    let member = path.split('/').nth(1).unwrap_or_default();
                    let nests = *nesting
                        .entry(member.to_owned())
                        .or_insert_with(|| nests_a_package(&top, merge_base, head, member));
                    if nests { Role::Rule } else { Role::Code }
                }
                role => role,
            };
            let is_key_named_build_script = matches!(role, Role::Code | Role::Other)
                && build_scripts
                    .iter()
                    .any(|script| case_insensitive::eq(script, &path));
            if is_key_named_build_script {
                changed.rules.push(format!(
                    "{path} (the build script its crate's build key names)"
                ));
                continue;
            }
            match role {
                Role::Unlisted if untracked.contains(&path) => {
                    changed.rules.push(format!("{path} {UNTRACKED_REMEDY}"));
                }
                Role::Rule | Role::Unlisted => changed.rules.push(path),
                Role::Code if source_path::is_rust(&path) => {
                    let (before, after) = versions(&path);
                    if !lint_expectations::only_expectations_changed(
                        before.as_deref(),
                        after.as_deref(),
                    ) {
                        changed.code.push(path);
                    }
                }
                Role::Code => changed.code.push(path),
                Role::RootManifest => {
                    let (before, after) = versions(&path);
                    if root_manifest_changes_rules(before.as_deref(), after.as_deref()) {
                        changed.rules.push(format!(
                            "{path} (outside [workspace.dependencies] and [patch])"
                        ));
                    }
                }
                Role::MemberManifest => {
                    let (before, after) = versions(&path);
                    let change = member_manifest_change(before.as_deref(), after.as_deref());
                    if matches!(change, Some(ManifestChange::Rule | ManifestChange::Both)) {
                        changed.rules.push(format!(
                            "{path} ([build-dependencies], the build key and the license key)"
                        ));
                    }
                    if matches!(change, Some(ManifestChange::Code | ManifestChange::Both)) {
                        changed.code.push(path);
                    }
                }
                Role::Other => {}
            }
        }
        Ok(changed)
    }

    /// The changed rule files, relative to the repository root.
    #[must_use]
    pub fn rules(&self) -> &[String] {
        &self.rules
    }

    /// The changed code files, relative to the repository root.
    #[must_use]
    pub fn code(&self) -> &[String] {
        &self.code
    }

    /// Whether the changes touch both rules and code, which a pull request may not.
    #[must_use]
    pub fn mixes_rules_and_code(&self) -> bool {
        !self.rules.is_empty() && !self.code.is_empty()
    }
}

/// The paths a change touches.
struct ChangedPaths {
    /// Every changed path, relative to the repository root.
    paths: BTreeSet<String>,
    /// The changed paths that are untracked files, which only a working-tree head has.
    untracked: BTreeSet<String>,
}

impl ChangedPaths {
    /// The paths changed from `merge_base` to `head` in the repository at `top`.
    ///
    /// # Errors
    ///
    /// Fails if git fails.
    fn between(top: &Path, merge_base: &str, head: &Head) -> Result<Self, PolicyError> {
        let mut paths = BTreeSet::new();
        let mut untracked = BTreeSet::new();
        let diff = ["diff", "--name-only", "--no-renames", "-z"];
        let listings: Vec<Vec<&str>> = match head {
            Head::Commit(revision) => vec![[&diff[..], &[merge_base, revision.as_str()]].concat()],
            Head::WorkingTree => {
                let output = git::run(top, &["ls-files", "-z", "--others", "--exclude-standard"])?;
                untracked.extend(git::paths(&output).map(str::to_owned));
                vec![
                    [&diff[..], &["--cached", merge_base]].concat(),
                    [&diff[..], &[merge_base]].concat(),
                ]
            }
        };
        for arguments in listings {
            let output = git::run(top, &arguments)?;
            paths.extend(git::paths(&output).map(str::to_owned));
        }
        untracked.retain(|path| !paths.contains(path));
        paths.extend(untracked.iter().cloned());
        Ok(Self { paths, untracked })
    }
}

/// What a changed file is.
#[derive(Debug, PartialEq, Eq)]
enum Role {
    /// A rule file.
    Rule,
    /// A file none of the lists know, which is a rule, so a new tool's configuration file can't
    /// land beside code.
    Unlisted,
    /// Code that the rules check.
    Code,
    /// The root `Cargo.toml`, which holds rules and the workspace's dependency versions.
    RootManifest,
    /// A member's `Cargo.toml`, which is code apart from the build script's settings and the
    /// license.
    MemberManifest,
    /// Neither: the readme, the lockfile, the changelog, the licenses and the docs. A Rust file
    /// whose change only touches lint expectations is neither too, though its role is code: a
    /// code change can need an `expect` in another file, and a new lint can need one in code.
    Other,
}

/// What the file at `path`, relative to the repository root, is. Rule names match in any case,
/// since a case-insensitive file system finds `Clippy.toml` or `Build.rs` for the name a tool
/// looks for; the names that make a file code or neither match exactly.
fn role(path: &str) -> Role {
    if path == "Cargo.toml" {
        return Role::RootManifest;
    }
    if OTHER_FILES.contains(&path) {
        return Role::Other;
    }
    let name = path.rsplit('/').next().unwrap_or(path);
    if name == "Cargo.toml" && path.starts_with("crates/") {
        return Role::MemberManifest;
    }
    let is_rule = ToolConfig::of(path).is_some()
        || agent_files::is_agent_file(path)
        || is_build_script(path)
        || RULE_NAMES
            .iter()
            .any(|rule| case_insensitive::eq(name, rule))
        || RULE_FILES
            .iter()
            .any(|rule| case_insensitive::eq(path, rule))
        || RULE_DIRECTORIES
            .iter()
            .any(|directory| case_insensitive::strip_prefix(path, directory).is_some());
    let is_crate_license = !source_path::is_rust(path)
        && path
            .strip_prefix("crates/")
            .and_then(|rest| rest.split_once('/'))
            .is_some_and(|(_, file)| !file.contains('/') && file.starts_with("LICENSE"));
    let is_docs_file = path.starts_with("docs/")
        && Path::new(path).extension().is_some_and(|extension| {
            extension.eq_ignore_ascii_case("md")
                || DOCS_IMAGE_EXTENSIONS
                    .iter()
                    .any(|image| extension.eq_ignore_ascii_case(image))
        });
    let is_other = path.starts_with("LICENSE") || is_crate_license || is_docs_file;
    if is_rule {
        Role::Rule
    } else if is_crate_license {
        Role::Other
    } else if path.starts_with("crates/") || source_path::is_rust(path) {
        Role::Code
    } else if is_other {
        Role::Other
    } else {
        Role::Unlisted
    }
}

/// Whether `path`, by its shape alone, is a rule as a package's build script: `build.rs` in any
/// case and any directory, since Cargo runs the `build.rs` beside any package's `Cargo.toml`, and
/// a change can add a package anywhere before `trellis-policy check` sees it. The exception is a
/// file below a crate's `src/`, `tests/`, `benches/` or `examples/` directory,
/// `crates/<crate>/src/build.rs` for one, which is a module, a test, a benchmark or an example
/// unless a package is nested there. [`ChangedFiles::between`] still makes such a file a rule
/// when the crate's directory holds any package manifest besides its own, at either side of the
/// change, or when a side can't be read.
fn is_build_script(path: &str) -> bool {
    let directory = path.rsplit_once('/').map_or("", |(directory, _)| directory);
    let mut segments = directory.split('/');
    let in_target_directory = segments.next() == Some("crates")
        && segments.next().is_some_and(|member| !member.is_empty())
        && segments
            .next()
            .is_some_and(|top| ["src", "tests", "benches", "examples"].contains(&top));
    is_named_build_script(path) && !in_target_directory
}

/// Whether the file at `path` is named `build.rs`, in any case.
fn is_named_build_script(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    case_insensitive::eq(name, "build.rs")
}

/// Whether `name` is a package manifest's file name, in any case, since a case-insensitive file
/// system finds `cargo.toml` when Cargo looks for `Cargo.toml`.
fn is_manifest(name: &str) -> bool {
    case_insensitive::eq(name, "Cargo.toml")
}

/// The paths, relative to the repository root, of the build scripts that the `build` key of each
/// member manifest `crates/<crate>/Cargo.toml` names at `revision`, or in the working tree
/// without one. `build = true` names `build.rs`. A path that leaves the repository names no file
/// in it.
///
/// # Errors
///
/// Fails if git fails, or if the working tree's `crates/` exists but can't be read.
fn named_build_scripts(
    repository: &Path,
    revision: Option<&str>,
) -> Result<BTreeSet<String>, PolicyError> {
    let crates = match revision {
        Some(revision) => {
            let output = git::run(
                repository,
                &["ls-tree", "-z", "--name-only", revision, "crates/"],
            )?;
            git::paths(&output).map(str::to_owned).collect()
        }
        None => working_tree_crates(repository)?,
    };
    let mut scripts = BTreeSet::new();
    for directory in crates {
        let manifest = format!("{directory}/Cargo.toml");
        let text = match revision {
            Some(revision) => show(repository, revision, &manifest),
            None => fs::read_to_string(repository.join(&manifest)).ok(),
        };
        let Some(table) = text.and_then(|text| toml_file::parse(&text).ok()) else {
            continue;
        };
        let names = match toml_file::lookup(&table, &["package", "build"]) {
            Some(Value::Boolean(true)) => vec!["build.rs"],
            Some(Value::String(name)) => vec![name.as_str()],
            Some(Value::Array(names)) => names.iter().filter_map(Value::as_str).collect(),
            _ => Vec::new(),
        };
        scripts.extend(
            names
                .into_iter()
                .filter_map(|name| normalized(&format!("{directory}/{name}"))),
        );
    }
    Ok(scripts)
}

/// The entries of the working tree's `crates/` directory under `repository`, as `crates/<name>`.
///
/// # Errors
///
/// Fails if `crates/` exists but can't be read.
fn working_tree_crates(repository: &Path) -> Result<Vec<String>, PolicyError> {
    let directory = repository.join("crates");
    let read_error = |source| PolicyError::Read {
        path: directory.clone(),
        source,
    };
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(read_error(error)),
    };
    let mut crates = Vec::new();
    for entry in entries {
        let name = entry.map_err(read_error)?.file_name();
        crates.push(format!("crates/{}", name.to_string_lossy()));
    }
    Ok(crates)
}

/// `path`, relative to the repository root with `/` or `\` separators, with its `.` and `..`
/// parts resolved, or `None` if it is absolute or climbs above the root.
fn normalized(path: &str) -> Option<String> {
    if path.starts_with(['/', '\\']) || path.contains(':') {
        return None;
    }
    let mut parts = Vec::new();
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

/// Whether the root `Cargo.toml` changed anywhere but in `[workspace.dependencies]` and `[patch]`,
/// from `before` to `after`. A missing or unparsable side counts as a change. A dependency bump
/// can move both tables together with the code its new API needs; `deny.toml` limits where a
/// patch can come from, and the path-dependency check rejects a patch to a local crate that isn't
/// a member.
fn root_manifest_changes_rules(before: Option<&str>, after: Option<&str>) -> bool {
    let before = before.and_then(without_dependencies);
    let after = after.and_then(without_dependencies);
    before.is_none() || after.is_none() || before != after
}

/// The root manifest `text` without its `[workspace.dependencies]` and `[patch]` tables, if it
/// parses.
fn without_dependencies(text: &str) -> Option<Table> {
    let mut table = toml_file::parse(text).ok()?;
    if let Some(Value::Table(workspace)) = table.get_mut("workspace") {
        workspace.remove("dependencies");
    }
    table.remove("patch");
    Some(table)
}

/// Which part of a member's `Cargo.toml` a change touches.
#[derive(Debug, PartialEq, Eq)]
enum ManifestChange {
    /// Only the rule settings.
    Rule,
    /// Only the rest, which is code.
    Code,
    /// Both.
    Both,
}

/// What a change to a member's `Cargo.toml` from `before` to `after` touches, compared as parsed
/// TOML, or `None` if nothing parses differently. The rule settings are `[build-dependencies]` on
/// any platform and the `build` and `license` keys of `[package]`, so a build script and the
/// dependencies it needs land in one rule change, and so does a license with the answer that
/// sets it. A proc-macro dependency is code: it lands with its first use, since `cargo shear`
/// rejects a dependency nothing uses. A new, deleted or unparsable manifest is [`Code`].
///
/// [`Code`]: ManifestChange::Code
fn member_manifest_change(before: Option<&str>, after: Option<&str>) -> Option<ManifestChange> {
    let parse = |text: Option<&str>| text.and_then(|text| toml_file::parse(text).ok());
    let (Some(before), Some(after)) = (parse(before), parse(after)) else {
        return Some(ManifestChange::Code);
    };
    let before = ManifestParts::split(before);
    let after = ManifestParts::split(after);
    match (before.code == after.code, before.rules == after.rules) {
        (true, true) => None,
        (true, false) => Some(ManifestChange::Rule),
        (false, true) => Some(ManifestChange::Code),
        (false, false) => Some(ManifestChange::Both),
    }
}

/// A member manifest split into its rule settings and the rest.
#[derive(Debug)]
struct ManifestParts {
    /// The manifest without its rule settings.
    code: Table,
    /// The rule settings, each keyed by its path of keys.
    rules: BTreeMap<Vec<String>, Value>,
}

impl ManifestParts {
    /// The member manifest `table` split into its rule settings, which are `[build-dependencies]`
    /// on any platform and the `build` and `license` keys of `[package]`, and the rest. The
    /// license is a rule setting so that the pull request that changes the workspace's license
    /// can change each crate's line with it. A `[target.<platform>]` table or a `[target]` table
    /// left empty is dropped, so adding the first platform-specific build dependency changes
    /// nothing else.
    fn split(mut table: Table) -> Self {
        let mut rules = BTreeMap::new();
        if let Some(Value::Table(package)) = table.get_mut("package") {
            for key in ["build", "license"] {
                if let Some(value) = package.remove(key) {
                    rules.insert(vec!["package".to_owned(), key.to_owned()], value);
                }
            }
        }
        if let Some(build) = table.remove("build-dependencies") {
            rules.insert(vec!["build-dependencies".to_owned()], build);
        }
        if let Some(Value::Table(targets)) = table.get_mut("target") {
            for (platform, settings) in targets.iter_mut() {
                if let Value::Table(settings) = settings
                    && let Some(build) = settings.remove("build-dependencies")
                {
                    rules.insert(
                        vec![
                            "target".to_owned(),
                            platform.clone(),
                            "build-dependencies".to_owned(),
                        ],
                        build,
                    );
                }
            }
            targets.retain(|_, settings| settings.as_table().is_none_or(|table| !table.is_empty()));
            if targets.is_empty() {
                table.remove("target");
            }
        }
        Self { code: table, rules }
    }
}

/// Whether the crate directory `crates/<member>` may hold a package other than the crate itself at
/// `merge_base` or at `head`, in the repository at `top`: whether a package manifest other than
/// `crates/<member>/Cargo.toml` is anywhere below it, or a side can't be read. Every segment
/// matches in any case, as a case-insensitive file system merges the directories. A side without
/// the directory has no manifest there.
fn nests_a_package(top: &Path, merge_base: &str, head: &Head, member: &str) -> bool {
    let head = match head {
        Head::WorkingTree => Side::WorkingTree,
        Head::Commit(revision) => Side::Revision(revision),
    };
    [Side::Revision(merge_base), head]
        .iter()
        .any(|side| side.nests_a_package(top, member).unwrap_or(true))
}

/// One side of a change.
enum Side<'revision> {
    /// The tree of a revision.
    Revision(&'revision str),
    /// The working tree on disk.
    WorkingTree,
}

impl Side<'_> {
    /// Whether `crates/<member>` holds a package manifest other than its own, in any case, or
    /// `None` if this side can't be read.
    fn nests_a_package(&self, top: &Path, member: &str) -> Option<bool> {
        for crates in self.directories(top, "")? {
            if !case_insensitive::eq(&crates, "crates") {
                continue;
            }
            for directory in self.directories(top, &crates)? {
                if !case_insensitive::eq(&directory, member) {
                    continue;
                }
                let nested = self
                    .files(top, &format!("{crates}/{directory}"))?
                    .iter()
                    .filter_map(|file| file.rsplit_once('/'))
                    .any(|(_, name)| is_manifest(name));
                if nested {
                    return Some(true);
                }
            }
        }
        Some(false)
    }

    /// The names of the directories in `directory`, relative to `top`, or `None` if they can't
    /// be read or a name in it isn't UTF-8.
    fn directories(&self, top: &Path, directory: &str) -> Option<Vec<String>> {
        match self {
            Self::Revision(revision) => {
                let output =
                    git::run(top, &["ls-tree", "-z", &format!("{revision}:{directory}")]).ok()?;
                let directories = git::paths(&output).filter_map(|entry| {
                    let (metadata, name) = entry.split_once('\t')?;
                    let is_tree = metadata.split(' ').nth(1) == Some("tree");
                    is_tree.then(|| name.to_owned())
                });
                Some(directories.collect())
            }
            Self::WorkingTree => {
                let mut directories = Vec::new();
                for entry in fs::read_dir(top.join(directory)).ok()? {
                    let entry = entry.ok()?;
                    let name = entry.file_name().into_string().ok()?;
                    if entry.file_type().ok()?.is_dir() {
                        directories.push(name);
                    }
                }
                Some(directories)
            }
        }
    }

    /// The paths of the files below `directory`, relative to it, or `None` if they can't be
    /// read or a name below it isn't UTF-8.
    fn files(&self, top: &Path, directory: &str) -> Option<Vec<String>> {
        match self {
            Self::Revision(revision) => {
                let tree = format!("{revision}:{directory}");
                let output = git::run(top, &["ls-tree", "-r", "-z", "--name-only", &tree]).ok()?;
                Some(git::paths(&output).map(str::to_owned).collect())
            }
            Self::WorkingTree => {
                let mut files = Vec::new();
                let mut pending = vec![String::new()];
                while let Some(relative) = pending.pop() {
                    for entry in fs::read_dir(top.join(directory).join(&relative)).ok()? {
                        let entry = entry.ok()?;
                        let name = entry.file_name().into_string().ok()?;
                        let path = if relative.is_empty() {
                            name
                        } else {
                            format!("{relative}/{name}")
                        };
                        if entry.file_type().ok()?.is_dir() {
                            pending.push(path);
                        } else {
                            files.push(path);
                        }
                    }
                }
                Some(files)
            }
        }
    }
}

/// The text of `path` at `revision`, if it exists there.
fn show(repository: &Path, revision: &str, path: &str) -> Option<String> {
    git::run(repository, &["show", &format!("{revision}:{path}")]).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_files_are_rules_at_any_depth() {
        for path in [
            "build.rs",
            "clippy.toml",
            "crates/app/clippy.toml",
            "crates/app/src/.rustfmt.toml",
            "crates/app/.cargo/config",
            "crates/app/build.rs",
            "crates/a/b/build.rs",
            "crates/build.rs",
            "tools/x/build.rs",
            "lints/build.rs",
            "docs/build.rs",
            "crates/app/SRC/build.rs",
            ".github/workflows/ci.yml",
            ".github/dependabot.yml",
            "justfile",
            ".justfile",
            "crates/app/Justfile",
            ".config/nextest.toml",
            "deny.toml",
            "dylint.toml",
            "rust-toolchain.toml",
            "release-plz.toml",
            "renovate.json",
            "lychee.toml",
            ".gitattributes",
            ".cargo/config.toml",
            "crates/app/.editorconfig",
            "AGENTS.md",
            "CONTRIBUTING.md",
            ".copier-answers.yml",
            ".gitignore",
            "crates/app/.gitignore",
            "crates/app/src/.ignore",
            ".ignore",
            "crates/app/web/tsconfig.json",
            "crates/app/web/bunfig.toml",
            "crates/app/web/.npmrc",
            "crates/app/web/.GitIgnore",
            "crates/app/web/TSConfig.json",
        ] {
            assert_eq!(role(path), Role::Rule, "{path} should be a rule");
        }
    }

    #[test]
    fn rule_names_match_in_any_case() {
        for path in [
            "crates/app/Clippy.toml",
            "crates/app/.Cargo/Config.toml",
            "crates/app/Build.rs",
            "BUILD.RS",
            "crates/app/JUSTFILE",
            ".GitHub/workflows/ci.yml",
            ".Config/Nextest.toml",
            "contributing.md",
            ".Copier-Answers.yml",
            "Renovate.json",
        ] {
            assert_eq!(role(path), Role::Rule, "{path} should be a rule");
        }
    }

    #[test]
    fn agent_instruction_files_are_rules_even_in_crates() {
        for path in [
            "AGENTS.md",
            "crates/app/AGENTS.md",
            "crates/app/CLAUDE.md",
            "CLAUDE.local.md",
            "crates/app/src/GEMINI.md",
            "crates/app/.cursorrules",
            "crates/app/.cursor/rules/x.mdc",
            ".claude/settings.json",
            "crates/app/.claude/commands/fix.md",
            ".github/copilot-instructions.md",
            "docs/agents.md",
        ] {
            assert_eq!(role(path), Role::Rule, "{path} should be a rule");
        }
    }

    #[test]
    fn crate_sources_are_code() {
        for path in [
            "crates/app/src/lib.rs",
            "crates/app/tests/app.rs",
            "crates/app/cargo.toml",
            "crates/app_core/src/lib.rs",
            "crates/app/src/build.rs",
            "crates/app/src/BUILD.RS",
            "crates/app/src/image/build.rs",
            "crates/app/tests/build.rs",
            "crates/a/benches/build.rs",
            "crates/a/examples/build.rs",
            "crates/a/src/bin/build.rs",
            "crates/app/README.md",
            "crates/app/web/package.json",
            "crates/app/web/bun.lock",
            "crates/app/web/src/main.ts",
            "docs/notes.rs",
            "src/main.rs",
            "tools/x/y.rs",
        ] {
            assert_eq!(role(path), Role::Code, "{path} should be code");
        }
        for path in ["crates/app/Cargo.toml", "crates/new_crate/Cargo.toml"] {
            assert_eq!(
                role(path),
                Role::MemberManifest,
                "{path} is compared by content"
            );
        }
    }

    /// A member manifest with a normal dependency and the lints table.
    const MEMBER: &str = "[package]\nname = \"app\"\n\n[lints]\nworkspace = true\n\n\
                          [dependencies]\nserde = \"1\"\n";

    #[test]
    fn a_member_manifest_change_to_the_build_settings_alone_is_a_rule() {
        for (before, after, why) in [
            (
                MEMBER.to_owned(),
                MEMBER.replace("name = \"app\"\n", "name = \"app\"\nbuild = \"gen.rs\"\n"),
                "the build key",
            ),
            (
                MEMBER.to_owned(),
                format!("{MEMBER}\n[build-dependencies]\ncc = \"1\"\n"),
                "a build dependency",
            ),
            (
                MEMBER.to_owned(),
                format!("{MEMBER}\n[target.'cfg(windows)'.build-dependencies]\nwinres = \"0.1\"\n"),
                "a platform-specific build dependency",
            ),
            (
                MEMBER.to_owned(),
                MEMBER.replace(
                    "name = \"app\"\n",
                    "name = \"app\"\nlicense.workspace = true\n",
                ),
                "inheriting the license",
            ),
            (
                MEMBER.replace(
                    "name = \"app\"\n",
                    "name = \"app\"\nlicense.workspace = true\n",
                ),
                MEMBER.to_owned(),
                "dropping the license",
            ),
        ] {
            assert_eq!(
                member_manifest_change(Some(&before), Some(&after)),
                Some(ManifestChange::Rule),
                "{why} is a rule change"
            );
        }
    }

    #[test]
    fn other_member_manifest_changes_are_code() {
        for (before, after, why) in [
            (
                Some(MEMBER),
                Some(MEMBER.replace("serde = \"1\"", "serde = \"2\"")),
                "a normal dependency",
            ),
            (
                Some(MEMBER),
                Some(format!(
                    "{MEMBER}\n[target.'cfg(windows)'.dependencies]\nwinapi = \"0.3\"\n"
                )),
                "a platform-specific normal dependency",
            ),
            (
                Some(MEMBER),
                Some(format!("{MEMBER}serde_derive = \"1\"\n")),
                "a proc-macro dependency",
            ),
            (
                Some(MEMBER),
                Some(format!(
                    "{MEMBER}\n[dev-dependencies]\nerrors = {{ package = \"thiserror-impl\", version = \"2\" }}\n"
                )),
                "a renamed proc-macro dev-dependency",
            ),
            (None, Some(MEMBER.to_owned()), "a new crate"),
            (Some(MEMBER), None, "a deleted crate"),
            (
                Some(MEMBER),
                Some("not = [valid".to_owned()),
                "an unparsable manifest",
            ),
        ] {
            assert_eq!(
                member_manifest_change(before, after.as_deref()),
                Some(ManifestChange::Code),
                "{why} is code"
            );
        }
        assert_eq!(
            member_manifest_change(Some(MEMBER), Some(&format!("# The app.\n{MEMBER}"))),
            None,
            "a comment is neither"
        );
    }

    #[test]
    fn a_member_manifest_change_to_both_parts_is_both() {
        for (after, why) in [
            (
                format!("{MEMBER}\n[build-dependencies]\ncc = \"1\"\n")
                    .replace("serde = \"1\"", "serde = \"2\""),
                "a normal dependency beside a build dependency",
            ),
            (
                format!(
                    "{MEMBER}\n[target.'cfg(windows)'.dependencies]\nwinapi = \"0.3\"\n\n\
                     [target.'cfg(windows)'.build-dependencies]\nwinres = \"0.1\"\n"
                ),
                "a platform-specific normal and build dependency",
            ),
        ] {
            assert_eq!(
                member_manifest_change(Some(MEMBER), Some(&after)),
                Some(ManifestChange::Both),
                "{why} is both"
            );
        }
    }

    #[test]
    fn docs_and_the_lockfile_are_neither() {
        for path in [
            "Cargo.lock",
            "README.md",
            "CHANGELOG.md",
            "SECURITY.md",
            "LICENSE",
            "LICENSE-MIT",
            "crates/demo/LICENSE",
            "crates/demo/LICENSE-APACHE",
            ".editorconfig",
            "docs/architecture.md",
            "docs/guides/setup.md",
            "docs/agents-overview.md",
            "docs/claude-setup.md",
            "docs/img/arch.png",
            "docs/arch.SVG",
            "docs/img/photo.jpg",
            "docs/img/photo.jpeg",
            "docs/img/demo.gif",
            "docs/img/demo.webp",
        ] {
            assert_eq!(role(path), Role::Other, "{path} should be neither");
        }
        assert_eq!(
            role("Cargo.toml"),
            Role::RootManifest,
            "the root manifest is compared by content"
        );
    }

    #[test]
    fn only_a_crate_root_license_file_is_neither() {
        for path in [
            "crates/demo/src/LICENSE",
            "crates/demo/LICENSE.rs",
            "crates/demo/licenses/MIT",
        ] {
            assert_eq!(
                role(path),
                Role::Code,
                "{path} is code, not a crate's license file"
            );
        }
    }

    #[test]
    fn the_style_guide_is_a_rule() {
        for path in ["docs/style.md", "docs/Style.md", "CONTRIBUTING.md"] {
            assert_eq!(role(path), Role::Rule, "{path} holds rules review checks");
        }
    }

    #[test]
    fn files_no_list_knows_are_unlisted_rules() {
        for path in [
            ".pre-commit-config.yaml",
            "cliff.toml",
            "docs/notes.txt",
            "docs/img/arch.bmp",
            "arch.png",
            "img/arch.svg",
            "tools/check.sh",
            "lints/Cargo.toml",
            "readme.md",
            "notes.md",
        ] {
            assert_eq!(role(path), Role::Unlisted, "{path} should be a rule");
        }
    }

    #[test]
    fn a_dependency_change_in_the_root_manifest_is_not_a_rule() {
        let before =
            "[workspace.dependencies]\na = \"1\"\n\n[workspace.lints.rust]\nx = \"warn\"\n";
        let after = "# A comment.\n[workspace.dependencies]\na = \"2\"\nb = \"1\"\n\n\
                     [workspace.lints.rust]\nx = \"warn\"\n";
        assert!(
            !root_manifest_changes_rules(Some(before), Some(after)),
            "changing dependencies and comments only isn't a rule change"
        );
    }

    #[test]
    fn a_patch_change_in_the_root_manifest_is_not_a_rule() {
        let before = "[workspace.dependencies]
a = \"1\"

[patch.crates-io]
b = { git = \"x\", rev = \"1\" }
";
        let after = "[workspace.dependencies]
a = \"2\"

[patch.crates-io]
b = { git = \"x\", rev = \"2\" }
";
        assert!(
            !root_manifest_changes_rules(Some(before), Some(after)),
            "a dependency bump moves the patches with the dependencies, and isn't a rule change"
        );
    }

    #[test]
    fn other_root_manifest_changes_are_rules() {
        let before =
            "[workspace.dependencies]\na = \"1\"\n\n[workspace.lints.rust]\nx = \"warn\"\n";
        let after =
            "[workspace.dependencies]\na = \"1\"\n\n[workspace.lints.rust]\nx = \"allow\"\n";
        assert!(
            root_manifest_changes_rules(Some(before), Some(after)),
            "lowering a lint is a rule change"
        );
        assert!(
            root_manifest_changes_rules(Some(before), Some("not = [valid")),
            "an unparsable manifest counts as a rule change"
        );
        assert!(
            root_manifest_changes_rules(None, Some(after)),
            "a new root manifest is a rule change"
        );
    }
}
