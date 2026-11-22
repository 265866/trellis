//! Which files a branch changes are rules, and which are the code the rules check.

use std::collections::{BTreeMap, BTreeSet};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::{fs, iter};

use toml::{Table, Value};

use crate::head::Head;
use crate::policy_error::PolicyError;
use crate::tool_config::ToolConfig;
use crate::{
    agent_files, case_insensitive, config_files, deploy_scripts, git, lint_expectations,
    source_path, toml_file,
};

/// Directories whose every file is a rule: the workflows.
const RULE_DIRECTORIES: [&str; 1] = [".github/"];

/// File names that are rules in any directory: the check recipes, and the settings files tools
/// read from the directory they check and its ancestors. An ignore file hides files from git,
/// `typos` and the checks that list the repository's files, and the Bun and npm settings change
/// what a web UI's check and install do. TypeScript's settings files, which
/// [`config_files::is_tsconfig`] names, are rules in any directory too.
const RULE_NAMES: [&str; 6] = [
    "justfile",
    ".justfile",
    ".gitignore",
    ".ignore",
    "bunfig.toml",
    ".npmrc",
];

/// The name of a deploy kit's directory, at the root or in a crate's directory, as
/// [`deploy_scripts::kit_of`] reads it.
const KIT_NAME: &str = "deploy";

/// The names of the files in a deploy kit that are rules all the same, since a tool reads them as
/// configuration: a composite action's definition, which a workflow can run with
/// `uses: ./deploy` or `uses: ./crates/<crate>/deploy`, and a justfile module, which a justfile
/// loads with `mod` or `import`.
fn is_deploy_rule(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    ["action.yml", "action.yaml"]
        .iter()
        .any(|rule| case_insensitive::eq(name, rule))
        || Path::new(name)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("just"))
}

/// The extensions of the images that the pages under `docs/` show, which are neither rules nor
/// code, like the pages.
const DOCS_IMAGE_EXTENSIONS: [&str; 6] = ["png", "svg", "jpg", "jpeg", "gif", "webp"];

/// What the listing adds to an untracked file that is a rule only because no list knows it, and
/// that no diff lists, so ignoring it takes it out of the change.
const UNTRACKED_REMEDY: &str =
    "(untracked; commit it in a rules pull request, or ignore it in .git/info/exclude)";

/// What the listing adds to a submodule, or another git repository inside this one, whose files
/// no check sees, so its build script, manifest and tool settings could change with it.
const NESTED_REPOSITORY_NOTE: &str = "(another git repository, whose files no check sees)";

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
/// a member `Cargo.toml` that holds the build script's settings and the license, every submodule
/// or other git repository inside this one, whose files no check sees, and any file the lists
/// here don't know, which the listing marks when it is only untracked. The code is every other
/// change under `crates/`, every other change in a deploy kit, under the root `deploy/` or a
/// crate's `crates/<crate>/deploy/`, which ships with the code it deploys and that
/// `deploy_scripts` shellchecks, and every Rust file elsewhere. A Rust file whose change only adds,
/// removes, moves or changes lint expectations is neither, and so is every file of a crate whose
/// directory moved whole and of a root deploy kit that moved whole into a crate. A pull request
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
    /// repository that holds `repository`. A rename counts as both of its paths. `None` means
    /// there is nothing to compare: `base` isn't a commit and the repository has no
    /// remote-tracking branches, as before its first push.
    ///
    /// # Errors
    ///
    /// Fails if git fails, such as when `head` isn't a commit of the repository, or `base` isn't
    /// one and the repository has a remote-tracking branch.
    pub fn between(
        repository: &Path,
        base: &str,
        head: &Head,
    ) -> Result<Option<Self>, PolicyError> {
        let top = git::run(repository, &["rev-parse", "--show-toplevel"])?;
        let top = PathBuf::from(top.trim_end_matches(['\r', '\n']));
        if let Head::Commit(revision) = head {
            let head_commit = format!("{revision}^{{commit}}");
            git::run(&top, &["rev-parse", "--verify", &head_commit])?;
        }
        if !has_a_base(&top, base)? {
            return Ok(None);
        }
        let head_revision = match head {
            Head::WorkingTree => "HEAD",
            Head::Commit(revision) => revision.as_str(),
        };
        let merge_base = git::run(&top, &["merge-base", base, head_revision])?;
        let merge_base = merge_base.trim();
        let head_commit = match head {
            Head::WorkingTree => None,
            Head::Commit(revision) => Some(revision.as_str()),
        };
        let mut build_scripts = named_build_scripts(&top, Some(merge_base))?;
        build_scripts.extend(named_build_scripts(&top, head_commit)?);
        let mut changed_paths = ChangedPaths::between(&top, merge_base, head)?;
        changed_paths.leave_out(&moved_directories(&top, merge_base, head, &build_scripts)?);
        let mut changed = Self {
            rules: changed_paths.nested_repository_rules(),
            code: Vec::new(),
        };
        let mut nesting = BTreeMap::new();
        let versions = |path: &str| {
            let before = show(&top, merge_base, path);
            let after = match head {
                Head::WorkingTree => fs::read_to_string(top.join(path)).ok(),
                Head::Commit(revision) => show(&top, revision, path),
            };
            (before, after)
        };
        for path in changed_paths.paths {
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
                Role::Unlisted if changed_paths.untracked.contains(&path) => {
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
        Ok(Some(changed))
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

/// Whether there is a base to compare with in the repository at `top`: whether `base` is a commit,
/// or, when it isn't, whether the repository has a remote-tracking branch, so that `base` should
/// have been one.
///
/// # Errors
///
/// Fails if git fails.
fn has_a_base(top: &Path, base: &str) -> Result<bool, PolicyError> {
    let base_commit = format!("{base}^{{commit}}");
    if git::run(top, &["rev-parse", "--verify", "--quiet", &base_commit]).is_ok() {
        return Ok(true);
    }
    let remotes = git::run(top, &["for-each-ref", "--count=1", "refs/remotes"])?;
    Ok(!remotes.is_empty())
}

/// The paths a change touches.
struct ChangedPaths {
    /// Every changed path but the nested repositories, relative to the repository root.
    paths: BTreeSet<String>,
    /// The changed paths that are untracked files, which only a working-tree head has.
    untracked: BTreeSet<String>,
    /// The changed paths that are another git repository at either side: a submodule, or, in the
    /// working tree, an untracked repository.
    nested_repositories: BTreeSet<String>,
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
        let mut nested_repositories = BTreeSet::new();
        let diff = [
            "diff",
            "--raw",
            "--no-renames",
            "--ignore-submodules=none",
            "-z",
        ];
        let listings: Vec<Vec<&str>> = match head {
            Head::Commit(revision) => vec![[&diff[..], &[merge_base, revision.as_str()]].concat()],
            Head::WorkingTree => {
                let output = git::run(top, &["ls-files", "-z", "--others", "--exclude-standard"])?;
                for path in git::paths(&output) {
                    match path.strip_suffix('/') {
                        Some(repository) => {
                            nested_repositories.insert(repository.to_owned());
                            untracked.insert(repository.to_owned());
                        }
                        None => {
                            untracked.insert(path.to_owned());
                        }
                    }
                }
                vec![
                    [&diff[..], &["--cached", merge_base]].concat(),
                    [&diff[..], &[merge_base]].concat(),
                ]
            }
        };
        for arguments in listings {
            let output = git::run(top, &arguments)?;
            for entry in DiffEntry::parse(&output) {
                if entry.is_gitlink {
                    nested_repositories.insert(entry.path.to_owned());
                }
                paths.insert(entry.path.to_owned());
            }
        }
        untracked.retain(|path| !paths.contains(path));
        paths.extend(untracked.iter().cloned());
        paths.retain(|path| !nested_repositories.contains(path));
        Ok(Self {
            paths,
            untracked,
            nested_repositories,
        })
    }

    /// Leaves out every path below one of `directories`.
    fn leave_out(&mut self, directories: &[String]) {
        self.paths.retain(|path| {
            !directories
                .iter()
                .any(|directory| is_below(path, directory))
        });
    }

    /// The rule line of each nested repository the change touches.
    fn nested_repository_rules(&self) -> Vec<String> {
        self.nested_repositories
            .iter()
            .map(|path| format!("{path} {NESTED_REPOSITORY_NOTE}"))
            .collect()
    }
}

/// A path that `git diff --raw` lists.
struct DiffEntry<'output> {
    /// The path, relative to the repository root.
    path: &'output str,
    /// Whether the path is a submodule at either side.
    is_gitlink: bool,
}

impl<'output> DiffEntry<'output> {
    /// The entries of `output`, `git diff --raw --no-renames -z` output, where each entry is its
    /// metadata, `:<mode before> <mode after> ...`, then its path.
    fn parse(output: &'output str) -> impl Iterator<Item = Self> {
        let mut fields = git::paths(output);
        iter::from_fn(move || {
            let metadata = fields.next()?;
            let path = fields.next()?;
            let is_gitlink = metadata
                .trim_start_matches(':')
                .split(' ')
                .take(2)
                .any(|mode| mode == git::GITLINK_MODE);
            Some(Self { path, is_gitlink })
        })
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
    /// Code that the rules check: a crate's files, a deploy kit's and every Rust file.
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
/// looks for; the names that make a file code or neither match exactly. A path Windows reads as
/// another path is a rule that no list knows, whatever it names.
fn role(path: &str) -> Role {
    if path == "Cargo.toml" {
        return Role::RootManifest;
    }
    if OTHER_FILES.contains(&path) {
        return Role::Other;
    }
    if source_path::windows_reads_as_another(path) {
        return Role::Unlisted;
    }
    let name = path.rsplit('/').next().unwrap_or(path);
    if name == "Cargo.toml" && path.starts_with("crates/") {
        return Role::MemberManifest;
    }
    let is_deploy_file = deploy_scripts::kit_of(path).is_some();
    let is_rule = ToolConfig::of(path).is_some()
        || (is_deploy_file && is_deploy_rule(path))
        || agent_files::is_agent_file(path)
        || is_build_script(path)
        || RULE_NAMES
            .iter()
            .any(|rule| case_insensitive::eq(name, rule))
        || config_files::is_tsconfig(name)
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
    } else if path.starts_with("crates/") || is_deploy_file || source_path::is_rust(path) {
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

/// The directories of the crates that moved whole, `crates/<old>` at `merge_base` and
/// `crates/<new>` at the head, whose files are neither rules nor code. A crate moved whole when:
///
/// - `crates/<old>` at `merge_base` and `crates/<new>` at the head are the same git tree, so every
///   file in it has the same bytes, name and mode at both ends, its manifest included, so the
///   package keeps its name, its build script and its build dependencies;
/// - `<old>` and `<new>` differ, and are ASCII names every system reads as written;
/// - at `merge_base`, `crates/<old>` is the one entry of `crates/` that a case-insensitive file
///   system reads as `<old>` or `<new>`, and at the head `crates/<new>` is, so nothing else held
///   the new place or holds the old one, on any file system;
/// - no other entry of `crates/` is that tree at either end, so the move pairs one directory with
///   one;
/// - `crates/<old>/Cargo.toml` names a package, so `crates/<old>` is a directory;
/// - the tree holds no symbolic link or submodule, whose target or contents would depend on where
///   it sits;
/// - git gives every file the same attributes at both ends, read from each end's own
///   `.gitattributes` files, both where git matches names exactly and where it matches them in
///   any case, as on macOS and Windows, so a checkout writes the same bytes at the new place on
///   every system; and neither end holds a `.gitattributes` spelled in other letters, which a
///   case-insensitive file system can read as one. This needs git 2.40 or later; with an older
///   git, no crate moved whole.
///
/// A working-tree head is judged at `HEAD`, and no crate moved whole while a path below
/// `crates/<old>` or `crates/<new>`, or a file named `.gitattributes`, in any letters, is staged,
/// changed in the working tree or untracked, so the working tree is never looser than the commit
/// it makes: `HEAD` doesn't hold an uncommitted entry of `crates/` that spells either name in
/// other letters.
///
/// The directory's name is all that changes, but a file in the crate that reads its own location,
/// such as a build script that reads `CARGO_MANIFEST_DIR`, or that names `crates/<new>` by a
/// relative path, can mean something else after the move. A code change can already change what
/// such a file does by filling a path it names, so review weighs that file when it lands.
///
/// [`moved_directories`] applies the conditions on `.gitattributes` files and on a working-tree
/// head's uncommitted `.gitattributes`; `uncommitted` lists a working-tree head's uncommitted
/// paths, and `head_revision` is `HEAD` for one.
///
/// # Errors
///
/// Fails if git fails, other than when it can't read attributes.
fn moved_crates(
    top: &Path,
    merge_base: &str,
    head_revision: &str,
    uncommitted: &[String],
) -> Result<Vec<String>, PolicyError> {
    let before = crate_entries(top, merge_base)?;
    let after = crate_entries(top, head_revision)?;
    let mut moved = Vec::new();
    for old in &before {
        let Some(new) = only(after.iter().filter(|entry| entry.id == old.id)) else {
            continue;
        };
        if only(before.iter().filter(|entry| entry.id == old.id)).is_none() {
            continue;
        }
        let near = |entries: &[TreeEntry]| -> Vec<String> {
            entries
                .iter()
                .filter(|entry| {
                    case_insensitive::eq(&entry.name, &old.name)
                        || case_insensitive::eq(&entry.name, &new.name)
                })
                .map(|entry| entry.name.clone())
                .collect()
        };
        let old_directory = format!("crates/{}", old.name);
        let new_directory = format!("crates/{}", new.name);
        let touched = |directory: &str| {
            let below = format!("{directory}/");
            uncommitted
                .iter()
                .any(|path| case_insensitive::strip_prefix(path, &below).is_some())
        };
        let eligible = old.name != new.name
            && plain_name(&old.name)
            && plain_name(&new.name)
            && near(&before) == [old.name.clone()]
            && near(&after) == [new.name.clone()]
            && !touched(&old_directory)
            && !touched(&new_directory)
            && names_a_package(top, merge_base, &old_directory);
        if !eligible {
            continue;
        }
        let Some(files) = plain_files(top, &old.id)? else {
            continue;
        };
        let same_attributes = [false, true].iter().all(|&ignore_case| {
            let before = attributes(top, merge_base, &old_directory, &files, ignore_case);
            before.is_some()
                && before == attributes(top, head_revision, &new_directory, &files, ignore_case)
        });
        if same_attributes {
            moved.push(old_directory);
            moved.push(new_directory);
        }
    }
    Ok(moved)
}

/// The directories that moved whole from `merge_base` to `head`, whose files are neither rules
/// nor code: the crates' directories that [`moved_crates`] finds and the root deploy kit and its
/// new place that [`moved_kit`] finds, given `build_scripts`, the files a member's `build` key
/// names at either end. Nothing moved whole while a working-tree head holds a staged, changed or
/// untracked file named `.gitattributes`, in any letters, or while either end holds a
/// `.gitattributes` spelled in other letters, which a case-insensitive file system can read as
/// one.
///
/// # Errors
///
/// Fails if git fails, other than when it can't read attributes.
fn moved_directories(
    top: &Path,
    merge_base: &str,
    head: &Head,
    build_scripts: &BTreeSet<String>,
) -> Result<Vec<String>, PolicyError> {
    let head_revision = match head {
        Head::Commit(revision) => revision.as_str(),
        Head::WorkingTree => "HEAD",
    };
    let uncommitted = match head {
        Head::Commit(_) => Vec::new(),
        Head::WorkingTree => uncommitted_paths(top)?,
    };
    let attributes_changed = uncommitted.iter().any(|path| {
        let name = path.rsplit('/').next().unwrap_or(path);
        case_insensitive::eq(name, ".gitattributes")
    });
    if attributes_changed {
        return Ok(Vec::new());
    }
    let mut moved = moved_crates(top, merge_base, head_revision, &uncommitted)?;
    moved.extend(moved_kit(
        top,
        merge_base,
        head_revision,
        &uncommitted,
        build_scripts,
    )?);
    if !moved.is_empty()
        && (misspells_attributes(top, merge_base)? || misspells_attributes(top, head_revision)?)
    {
        return Ok(Vec::new());
    }
    Ok(moved)
}

/// The root deploy kit, `deploy` at `merge_base`, and its new place, `crates/<crate>/deploy` at
/// the head, when the kit moved there whole, so that their files are neither rules nor code. The
/// kit moved whole when:
///
/// - `deploy` at `merge_base` and `crates/<crate>/deploy` at the head are the same git tree, so
///   every file in it has the same bytes, name and mode at both ends;
/// - at `merge_base`, `deploy` is the one entry of the root that a case-insensitive file system
///   reads as `deploy`, and at the head the root holds none, so the old place is gone;
/// - at the head, `crates/<crate>` is the one entry of `crates/` that a case-insensitive file
///   system reads as `<crate>`, `<crate>` is an ASCII name every system reads as written, and
///   `deploy` is the one entry of `crates/<crate>` read as `deploy`; at `merge_base`, no entry of
///   `crates/` read as `<crate>` holds an entry read as `deploy`, so the new place was empty;
/// - no other `crates/<crate>/deploy` is that tree at the head, so the move pairs one directory
///   with one;
/// - the tree holds no symbolic link or submodule, whose target or contents would depend on where
///   it sits, and no file named `Cargo.toml` or `build.rs`, in any letters, or that a member's
///   `build` key names at either end, since a package or a build script below a crate can mean
///   something else than one below the root;
/// - git gives every file the same attributes at both ends, as [`moved_crates`] says.
///
/// A working-tree head is judged at `HEAD`, and the kit didn't move whole while a path below
/// `deploy` or `crates/<crate>/deploy`, in any letters, is staged, changed in the working tree or
/// untracked; `uncommitted` lists those paths, and `head_revision` is `HEAD` for one.
///
/// A rename only moves the kit, but a script that names its own place, such as `deploy/install.sh`
/// in a unit or a README, now names a path the kit left; a later code change fixes it, as review
/// of that change sees.
///
/// # Errors
///
/// Fails if git fails, other than when it can't read attributes.
fn moved_kit(
    top: &Path,
    merge_base: &str,
    head_revision: &str,
    uncommitted: &[String],
    build_scripts: &BTreeSet<String>,
) -> Result<Vec<String>, PolicyError> {
    let is_kit_name = |entry: &&TreeEntry| case_insensitive::eq(&entry.name, KIT_NAME);
    let before = tree_entries(top, merge_base)?;
    let Some(old) = only(before.iter().filter(is_kit_name)) else {
        return Ok(Vec::new());
    };
    if old.name != KIT_NAME
        || !old.is_tree
        || tree_entries(top, head_revision)?
            .iter()
            .any(|entry| is_kit_name(&entry))
    {
        return Ok(Vec::new());
    }
    let crates_after = crate_entries(top, head_revision)?;
    let mut places = Vec::new();
    let crate_directories = crates_after.iter().filter(|entry| entry.is_tree);
    for krate in crate_directories {
        let entries = tree_entries(top, &format!("{head_revision}:crates/{}", krate.name))?;
        let kits: Vec<&TreeEntry> = entries.iter().filter(is_kit_name).collect();
        if kits.iter().any(|kit| kit.id == old.id) {
            let one_kit = matches!(kits[..], [kit] if kit.name == KIT_NAME);
            places.push((krate, one_kit));
        }
    }
    let Some((krate, one_kit)) = only(places.into_iter()) else {
        return Ok(Vec::new());
    };
    let near_crate = |entry: &&TreeEntry| case_insensitive::eq(&entry.name, &krate.name);
    let mut was_empty = true;
    let crates_before = crate_entries(top, merge_base)?;
    let old_crate_entries = crates_before.iter().filter(near_crate);
    for entry in old_crate_entries {
        if entry.is_tree {
            let entries = tree_entries(top, &format!("{merge_base}:crates/{}", entry.name))?;
            was_empty &= !entries.iter().any(|entry| is_kit_name(&entry));
        }
    }
    let new_directory = format!("crates/{}/{KIT_NAME}", krate.name);
    let touched = |directory: &str| {
        let below = format!("{directory}/");
        uncommitted
            .iter()
            .any(|path| case_insensitive::strip_prefix(path, &below).is_some())
    };
    let eligible = was_empty
        && plain_name(&krate.name)
        && crates_after.iter().filter(near_crate).count() == 1
        && one_kit
        && !touched(KIT_NAME)
        && !touched(&new_directory);
    if !eligible {
        return Ok(Vec::new());
    }
    let Some(files) = plain_files(top, &old.id)? else {
        return Ok(Vec::new());
    };
    let builds = files.iter().any(|file| {
        let name = file.rsplit('/').next().unwrap_or(file);
        is_manifest(name)
            || is_named_build_script(name)
            || [KIT_NAME, new_directory.as_str()].iter().any(|directory| {
                let path = format!("{directory}/{file}");
                build_scripts
                    .iter()
                    .any(|script| case_insensitive::eq(script, &path))
            })
    });
    let same_attributes = [false, true].iter().all(|&ignore_case| {
        let before = attributes(top, merge_base, KIT_NAME, &files, ignore_case);
        before.is_some()
            && before == attributes(top, head_revision, &new_directory, &files, ignore_case)
    });
    if builds || !same_attributes {
        return Ok(Vec::new());
    }
    Ok(vec![KIT_NAME.to_owned(), new_directory])
}

/// Whether `path` lies below `directory`, matched exactly, as the paths git lists are.
fn is_below(path: &str, directory: &str) -> bool {
    path.strip_prefix(directory)
        .is_some_and(|rest| rest.starts_with('/'))
}

/// The one item of `items`, or `None` when there are none or several.
fn only<T>(mut items: impl Iterator<Item = T>) -> Option<T> {
    match (items.next(), items.next()) {
        (Some(item), None) => Some(item),
        _ => None,
    }
}

/// Whether `name`, a directory below `crates/`, is ASCII and a name Windows reads as written.
fn plain_name(name: &str) -> bool {
    name.is_ascii() && !source_path::windows_reads_as_another(name)
}

/// An entry of a git tree at one end of a change.
struct TreeEntry {
    /// Its name, without the directory that holds it.
    name: String,
    /// The id of its git object.
    id: String,
    /// Whether it is a directory.
    is_tree: bool,
}

/// The entries of `crates/` in the tree of `revision`, none when it has no `crates/`.
///
/// # Errors
///
/// Fails if git fails.
fn crate_entries(top: &Path, revision: &str) -> Result<Vec<TreeEntry>, PolicyError> {
    let output = git::run(top, &["ls-tree", "-z", revision, "crates/"])?;
    Ok(parse_tree_entries(&output, "crates/"))
}

/// The entries of the git tree `tree`, such as `<revision>` for its root or
/// `<revision>:crates/<crate>` for a directory in it.
///
/// # Errors
///
/// Fails if git fails, such as when `tree` names no tree.
fn tree_entries(top: &Path, tree: &str) -> Result<Vec<TreeEntry>, PolicyError> {
    let output = git::run(top, &["ls-tree", "-z", tree])?;
    Ok(parse_tree_entries(&output, ""))
}

/// The entries `git ls-tree -z` lists in `output`, each name without `prefix`.
fn parse_tree_entries(output: &str, prefix: &str) -> Vec<TreeEntry> {
    git::paths(output)
        .filter_map(|entry| {
            let (metadata, path) = entry.split_once('\t')?;
            let mut fields = metadata.split(' ');
            let kind = fields.nth(1)?;
            let id = fields.next()?;
            Some(TreeEntry {
                name: path.strip_prefix(prefix)?.to_owned(),
                id: id.to_owned(),
                is_tree: kind == "tree",
            })
        })
        .collect()
}

/// Whether `directory/Cargo.toml` at `revision` is a manifest with a package name.
fn names_a_package(top: &Path, revision: &str, directory: &str) -> bool {
    show(top, revision, &format!("{directory}/Cargo.toml"))
        .and_then(|text| toml_file::parse(&text).ok())
        .is_some_and(|manifest| {
            toml_file::lookup(&manifest, &["package", "name"]).is_some_and(Value::is_str)
        })
}

/// The paths of the files in the git tree `tree`, relative to it, or `None` if it holds a
/// symbolic link or a submodule.
///
/// # Errors
///
/// Fails if git fails.
fn plain_files(top: &Path, tree: &str) -> Result<Option<Vec<String>>, PolicyError> {
    let output = git::run(top, &["ls-tree", "-r", "-z", tree])?;
    let mut files = Vec::new();
    for entry in git::paths(&output) {
        let Some((metadata, path)) = entry.split_once('\t') else {
            return Ok(None);
        };
        if !["100644", "100755"]
            .iter()
            .any(|mode| metadata.starts_with(mode))
        {
            return Ok(None);
        }
        files.push(path.to_owned());
    }
    Ok(Some(files))
}

/// Every attribute git gives each of `files`, relative to `directory`, as `.gitattributes` files
/// at `revision` set them, matching names in any case when `ignore_case` holds: one
/// `<file> NUL <attribute> NUL <value>` line per attribute set, or `None` if git can't say, as
/// before git 2.40, which can't read attributes from a revision.
fn attributes(
    top: &Path,
    revision: &str,
    directory: &str,
    files: &[String],
    ignore_case: bool,
) -> Option<Vec<String>> {
    let mut input = String::new();
    for file in files {
        input.extend([directory, "/", file, "\0"]);
    }
    let source = format!("--source={revision}");
    let ignore_case = format!("core.ignorecase={ignore_case}");
    let arguments = [
        "-c",
        &ignore_case,
        "check-attr",
        "-z",
        "-a",
        "--stdin",
        &source,
    ];
    let output = git::run_with_input(top, &arguments, Some(&input)).ok()?;
    let mut fields = output.split('\0');
    let mut lines = Vec::new();
    while let (Some(path), Some(attribute), Some(value)) =
        (fields.next(), fields.next(), fields.next())
    {
        let file = path.strip_prefix(directory)?;
        lines.push(format!("{file}\0{attribute}\0{value}"));
    }
    Some(lines)
}

/// Whether the tree of `revision` holds a file named `.gitattributes` in other letters, such as
/// `.GitAttributes`.
///
/// # Errors
///
/// Fails if git fails.
fn misspells_attributes(top: &Path, revision: &str) -> Result<bool, PolicyError> {
    let output = git::run(top, &["ls-tree", "-r", "-z", "--name-only", revision])?;
    Ok(git::paths(&output).any(|path| {
        let name = path.rsplit('/').next().unwrap_or(path);
        name != ".gitattributes" && case_insensitive::eq(name, ".gitattributes")
    }))
}

/// The paths, relative to the root of the repository at `top`, that differ between `HEAD` and
/// the index or between `HEAD` and the working tree, or that are untracked and not ignored.
///
/// # Errors
///
/// Fails if git fails.
fn uncommitted_paths(top: &Path) -> Result<Vec<String>, PolicyError> {
    let diff = ["diff", "--name-only", "--no-renames", "-z"];
    let staged = git::run(top, &[&diff[..], &["--cached", "HEAD"]].concat())?;
    let unstaged = git::run(top, &[&diff[..], &["HEAD"]].concat())?;
    let untracked = git::run(top, &["ls-files", "-z", "--others", "--exclude-standard"])?;
    Ok(git::paths(&staged)
        .chain(git::paths(&unstaged))
        .chain(git::paths(&untracked))
        .map(|path| path.trim_end_matches('/').to_owned())
        .collect())
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
            "crates/app/web/tsconfig.base.json",
            "crates/app/web/tsconfig.app.json",
            "crates/app/web/TSCONFIG.Node.JSON",
            "crates/app/web/tsconfig.json.json",
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
    fn a_path_windows_reads_as_another_is_a_rule() {
        for path in [
            "crates/app/build.rs.",
            "crates/app/build.rs ",
            "crates/app/build.rs. .",
            "crates/app/build.rs::$DATA",
            "crates/app./Cargo.toml",
            "crates/app /src/lib.rs",
            "crates/app\\build.rs",
            "docs/notes.md.",
            "cargo~1.tom",
            "GITIGN~1",
            "crates/app/BUILD~1.RS",
            "crates/app~1/src/lib.rs",
            "docs/notes~1.md",
        ] {
            assert_eq!(role(path), Role::Unlisted, "{path:?} should be a rule");
        }
        for path in [
            "crates/app/src/lib.rs",
            "crates/app/src/.hidden.rs",
            "crates/app/src/notes~.rs",
        ] {
            assert_eq!(role(path), Role::Code, "{path:?} should be code");
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
            "crates/app/web/jsconfig.json",
            "crates/app/web/mytsconfig.json",
            "crates/app/web/tsconfig.jsonc",
            "crates/app/web/tsconfig",
            "deploy/install.sh",
            "deploy/common.sh",
            "deploy/app.container",
            "deploy/systemd/app-backup.timer",
            "deploy/README.md",
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
    fn rule_files_in_a_deploy_kit_stay_rules() {
        for path in [
            "deploy/justfile",
            "deploy/.gitignore",
            "deploy/build.rs",
            "deploy/AGENTS.md",
            "deploy/clippy.toml",
            "deploy/.gitattributes",
            "deploy/.cargo/config.toml",
            "deploy/tsconfig.json",
            "deploy/bunfig.toml",
            "deploy/.npmrc",
            "deploy/action.yml",
            "deploy/host/Action.YAML",
            "deploy/tasks.just",
        ] {
            assert_eq!(role(path), Role::Rule, "{path} should stay a rule");
        }
        for path in ["deploy/install.sh.", "deploy/INSTAL~1.SH", "deploy/a:b.sh"] {
            assert_eq!(role(path), Role::Unlisted, "{path:?} should be a rule");
        }
    }

    #[test]
    fn a_crate_s_deploy_kit_is_code_and_its_rule_files_stay_rules() {
        for path in [
            "crates/demo/deploy/install.sh",
            "crates/demo/deploy/app.container",
            "crates/demo/deploy/systemd/app-backup.timer",
            "crates/demo/deploy/README.md",
            "crates/demo/deploy/lib/common",
        ] {
            assert_eq!(role(path), Role::Code, "{path} should be code");
        }
        for path in [
            "crates/demo/deploy/justfile",
            "crates/demo/deploy/.gitignore",
            "crates/demo/deploy/build.rs",
            "crates/demo/deploy/AGENTS.md",
            "crates/demo/deploy/clippy.toml",
            "crates/demo/deploy/.gitattributes",
            "crates/demo/deploy/tsconfig.json",
            "crates/demo/deploy/bunfig.toml",
            "crates/demo/deploy/.npmrc",
            "crates/demo/deploy/action.yml",
            "crates/demo/deploy/host/Action.YAML",
            "crates/demo/deploy/tasks.just",
        ] {
            assert_eq!(role(path), Role::Rule, "{path} should stay a rule");
        }
        for path in ["crates/demo/src/action.yml", "crates/demo/web/tasks.just"] {
            assert_eq!(role(path), Role::Code, "{path} is outside a kit");
        }
        for path in [
            "crates/demo/deploy/install.sh.",
            "crates/demo/deploy/INSTAL~1.SH",
        ] {
            assert_eq!(role(path), Role::Unlisted, "{path:?} should be a rule");
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
            "Deploy/install.sh",
            "deployment/install.sh",
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
