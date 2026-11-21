//! The workspace keeps the shape the other checks rely on.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use toml::{Table, Value};

use crate::policy_error::PolicyError;
use crate::violation::Violation;
use crate::workspace::Workspace;
use crate::{case_insensitive, repository_files, source_path, toml_file, upstream};

/// Checks the shape of the workspace that the other checks rely on:
///
/// - the members are exactly the packages one directory below `crates/`, `crates/<name>`, since
///   `rule-changes` judges a crate's `build.rs`, its `LICENSE` files and its manifest by that
///   place. Each resolved member is checked, not the `members` key, because Cargo also makes a
///   member of every path dependency inside the workspace. Neither `crates` nor `crates/<name>`
///   may be a symbolic link, which would put the crate's files at another path, and `<name>` is
///   one Windows reads as written, with no `:` or `\` and no dot or space at its end;
/// - no two paths in the repository, the files git tracks or would add, the directories that
///   hold them and the entries of `crates/`, are one path to a file system that ignores case, as
///   macOS's and Windows' do. A checkout there writes only one of them, so it builds other files
///   than the checks read: a directory spelled `crates` with a long s (U+017F) for its last
///   letter replaces the files of `crates/` that it also holds;
/// - `exclude` in `[workspace]` is missing or empty, so no crate under the root escapes the
///   workspace lints;
/// - every crate pulled in by path is a workspace member, so no excluded crate or crate outside
///   the repository builds without the workspace's checks;
/// - `libraries` in `[workspace.metadata.dylint]` is exactly one entry, the trellis lints: either
///   `{ git = "https://github.com/265866/trellis", rev = "<commit>", pattern = "lints" }` with a
///   full 40-character commit hash, or `{ path = "<dir>" }` where `<dir>/Cargo.toml`, relative to
///   the workspace root or absolute, is the package `trellis_lints` and `<dir>` is outside the
///   workspace root, so `cargo dylint --all` loads those lints at a fixed revision and nothing
///   else. Lints inside the workspace would be files whose changes count as code, so a change to
///   the lints could land beside the code it lets through;
/// - `[profile.release]` keeps `overflow-checks = true` and `panic = "abort"`, and no
///   per-package override turns them off;
/// - `.cargo/config.toml` holds only the `+crt-static` `rustflags` of the Windows MSVC targets.
///
/// # Errors
///
/// Fails if `.cargo/config.toml` exists but can't be read or parsed, or if `crates/` exists but
/// can't be read.
pub fn check(workspace: &Workspace) -> Result<Vec<Violation>, PolicyError> {
    let manifest = workspace.manifest();
    let mut violations = Vec::new();
    let mut require = |keys: &[&str], expected: Value, why: &str| {
        let found = toml_file::lookup(manifest, keys);
        if found != Some(&expected) {
            let found = found.map_or_else(|| "missing".to_owned(), toml_file::render);
            violations.push(Violation::new(
                "Cargo.toml".to_owned(),
                format!(
                    "`{}` must be `{}` but is {found}. {why}",
                    keys.join("."),
                    toml_file::render(&expected)
                ),
            ));
        }
    };
    require(
        &["profile", "release", "overflow-checks"],
        Value::Boolean(true),
        "An arithmetic overflow must stop the program rather than wrap silently; set it back.",
    );
    require(
        &["profile", "release", "panic"],
        Value::String("abort".to_owned()),
        "A panic must stop the program, and nothing may catch it; set it back.",
    );
    if let Some(problem) = exclude_problem(manifest) {
        violations.push(Violation::new("Cargo.toml".to_owned(), problem));
    }
    if let Some(problem) = dylint_problem(workspace.root(), manifest) {
        violations.push(Violation::new("Cargo.toml".to_owned(), problem));
    }
    let release = toml_file::lookup(manifest, &["profile", "release"]).and_then(Value::as_table);
    if let Some(release) = release {
        package_overrides("Cargo.toml", release, &mut violations);
    }
    cargo_config(workspace, &mut violations)?;
    path_dependencies(workspace, &mut violations);
    member_places(workspace, &mut violations)?;
    Ok(violations)
}

/// Reports each member that isn't `crates/<name>`, whose `<name>` isn't ASCII or is one Windows
/// reads as another, or whose `crates/<name>` or `crates` is a symbolic link, each package
/// `crates/<name>` that isn't a member, and each two paths a case-insensitive file system reads as
/// one.
///
/// # Errors
///
/// Fails if `crates/` exists but can't be read, or a member's directory can't be inspected.
fn member_places(
    workspace: &Workspace,
    violations: &mut Vec<Violation>,
) -> Result<(), PolicyError> {
    let mut places = BTreeSet::new();
    for member in workspace.members() {
        let place = workspace.relative(member.dir());
        let in_crates = place
            .strip_prefix("crates/")
            .is_some_and(|name| !name.is_empty() && !name.contains('/'));
        if in_crates {
            if !place.is_ascii() {
                violations.push(Violation::new(
                    place.clone(),
                    format!(
                        "The workspace member `{}` lives in `{place}`, whose name holds a \
                         character outside ASCII. File systems disagree on which such names are \
                         one directory, so no check can tell which files are the crate's \
                         everywhere. Give the directory an ASCII name.",
                        member.name()
                    ),
                ));
            }
            if source_path::windows_reads_as_another(&place) {
                violations.push(Violation::new(
                    place.clone(),
                    format!(
                        "The workspace member `{}` lives in `{place}`, whose name Windows reads as \
                         another name. Windows drops a dot or a space from the end of a name and \
                         reads a `:` or a `\\` as something else, so the crate's files sit at \
                         another path there. Give the directory a name without them.",
                        member.name()
                    ),
                ));
            }
            let path = workspace.root().join(&place);
            let metadata =
                fs::symlink_metadata(&path).map_err(|source| PolicyError::Read { path, source })?;
            if metadata.is_symlink() {
                violations.push(Violation::new(
                    place.clone(),
                    format!(
                        "The workspace member `{}` lives in `{place}`, which is a symbolic link, \
                         so the crate's files sit elsewhere and changes to them aren't judged as \
                         the crate's. Move the crate itself to `{place}`.",
                        member.name()
                    ),
                ));
            }
        } else {
            violations.push(Violation::new(
                workspace.relative(member.manifest_path()),
                format!(
                    "The workspace member `{}` lives in `{place}`, but every member lives one \
                     directory below `crates/`, as `crates/<name>`. Which changes are rules and \
                     which are code depends on that place: a crate's `build.rs`, its `LICENSE` \
                     files and its manifest are judged by it. Move the crate to `crates/<name>`, \
                     or, if it is a path dependency, make it a member there.",
                    member.name()
                ),
            ));
        }
        places.insert(place);
    }
    let directory = workspace.root().join("crates");
    let read_error = |source| PolicyError::Read {
        path: directory.clone(),
        source,
    };
    match fs::symlink_metadata(&directory) {
        Ok(metadata) if metadata.is_symlink() => violations.push(Violation::new(
            "crates".to_owned(),
            "This is a symbolic link, so the crates' files sit elsewhere and changes to them \
             aren't judged as theirs. Make `crates` a directory and move the crates into it."
                .to_owned(),
        )),
        Ok(_) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return merged_paths(workspace, &[], violations);
        }
        Err(error) => return Err(read_error(error)),
    }
    let mut entries = Vec::new();
    for entry in fs::read_dir(&directory).map_err(read_error)? {
        let entry = entry.map_err(read_error)?;
        let manifest = entry.path().join("Cargo.toml");
        let place = format!("crates/{}", entry.file_name().to_string_lossy());
        entries.push(place.clone());
        if manifest.is_file() && !places.contains(&place) {
            violations.push(Violation::new(
                format!("{place}/Cargo.toml"),
                "This package isn't a workspace member, so it builds without the workspace's \
                 lints and checks, though a change to it counts as code. Every package one \
                 directory below `crates/` is a member: keep `members = [\"crates/*\"]` in the \
                 root Cargo.toml, or move the package out of the repository."
                    .to_owned(),
            ));
        }
    }
    merged_paths(workspace, &entries, violations)
}

/// Reports each set of paths that a case-insensitive file system reads as one path, among the
/// files git tracks or would add, the directories that hold them, and `crate_entries`. A set
/// under two such directories isn't reported again, since everything below them merges too.
///
/// # Errors
///
/// Fails if the repository's files can't be listed.
fn merged_paths(
    workspace: &Workspace,
    crate_entries: &[String],
    violations: &mut Vec<Violation>,
) -> Result<(), PolicyError> {
    let files = repository_files::list(workspace.root())?;
    let mut spellings: BTreeMap<String, BTreeSet<&str>> = BTreeMap::new();
    for path in files.iter().chain(crate_entries) {
        let folded = case_insensitive::folded(path);
        let ends = |text: &str| -> Vec<usize> {
            let mut ends: Vec<usize> = text.match_indices('/').map(|(end, _)| end).collect();
            ends.push(text.len());
            ends
        };
        for (end, folded_end) in ends(path).into_iter().zip(ends(&folded)) {
            spellings
                .entry(folded[..folded_end].to_owned())
                .or_default()
                .insert(&path[..end]);
        }
    }
    for (folded, names) in &spellings {
        let merged_above = folded.rsplit_once('/').is_some_and(|(parent, _)| {
            spellings.get(parent).is_some_and(|parent| parent.len() > 1)
        });
        let Some(last) = names.last().filter(|_| names.len() > 1 && !merged_above) else {
            continue;
        };
        let quoted: Vec<String> = names.iter().map(|name| format!("`{name}`")).collect();
        let (others, final_name) = quoted.split_at(quoted.len() - 1);
        violations.push(Violation::new(
            (*last).to_owned(),
            format!(
                "{} and {} are one path to a file system that ignores case, as macOS's and \
                 Windows' do, so a checkout there holds only one of them and builds other files \
                 than the checks read. Rename all but one.",
                others.join(", "),
                final_name.concat()
            ),
        ));
    }
    Ok(())
}

/// What is wrong with `exclude` in `[workspace]` of the root manifest `manifest`, if anything.
fn exclude_problem(manifest: &Table) -> Option<String> {
    let exclude = toml_file::lookup(manifest, &["workspace", "exclude"])?;
    if exclude.as_array().is_some_and(Vec::is_empty) {
        return None;
    }
    Some(format!(
        "`workspace.exclude` must be missing or empty but is {}. A crate excluded from the \
         workspace builds without its lints and checks; make it a member under `crates/` \
         instead, and remove `exclude`.",
        toml_file::render(exclude)
    ))
}

/// The `git` of the one dylint library a workspace may load.
const LINTS_GIT: &str = upstream::REPOSITORY;

/// The package name of the trellis lints.
const LINTS_PACKAGE: &str = "trellis_lints";

/// What is wrong with `libraries` in `[workspace.metadata.dylint]` of the root manifest
/// `manifest`, in the workspace at `root`, if anything.
fn dylint_problem(root: &Path, manifest: &Table) -> Option<String> {
    let keys = ["workspace", "metadata", "dylint", "libraries"];
    let found = toml_file::lookup(manifest, &keys);
    let problem = match found.map(|value| value.as_array().map(Vec::as_slice)) {
        None => "it is missing".to_owned(),
        Some(Some([Value::Table(library)])) => library_problem(root, library)?,
        Some(_) => format!(
            "it is {}, but it must hold exactly one entry",
            found.map_or_else(String::new, toml_file::render)
        ),
    };
    Some(format!(
        "`{}`: {problem}. It must be exactly one entry, the trellis lints: either `[{{ git = \
         \"{LINTS_GIT}\", rev = \"<commit>\", pattern = \"lints\" }}]` with the full 40-character \
         lowercase commit hash, or `[{{ path = \"<dir>\" }}]` where `<dir>/Cargo.toml` is the \
         package `{LINTS_PACKAGE}` and `<dir>` is outside the workspace. `cargo dylint --all` \
         then loads those lints at a fixed revision, and nothing else.",
        keys.join(".")
    ))
}

/// What is wrong with `library`, the one entry of the dylint libraries of the workspace at
/// `root`, if anything.
fn library_problem(root: &Path, library: &Table) -> Option<String> {
    let mut keys: Vec<&str> = library.keys().map(String::as_str).collect();
    keys.sort_unstable();
    let rendered = toml_file::render(&Value::Table(library.clone()));
    match keys.as_slice() {
        ["git", "pattern", "rev"] => {
            let git = library.get("git").and_then(Value::as_str);
            let rev = library.get("rev").and_then(Value::as_str);
            let pattern = library.get("pattern").and_then(Value::as_str);
            if git != Some(LINTS_GIT) {
                Some(format!("the entry {rendered} doesn't load `{LINTS_GIT}`"))
            } else if !rev.is_some_and(is_commit_hash) {
                Some(format!(
                    "the entry {rendered} doesn't pin a full lowercase commit hash in `rev`"
                ))
            } else if pattern != Some("lints") {
                Some(format!(
                    "the entry {rendered} doesn't load the pattern `lints`"
                ))
            } else {
                None
            }
        }
        ["path"] => {
            let Some(path) = library.get("path").and_then(Value::as_str) else {
                return Some(format!(
                    "the entry {rendered} has a `path` that isn't a string"
                ));
            };
            let lints = root.join(path);
            let lints_manifest = lints.join("Cargo.toml");
            let name = match toml_file::read(&lints_manifest) {
                Ok(lints_manifest) => toml_file::lookup(&lints_manifest, &["package", "name"])
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                Err(error) => return Some(format!("the entry {rendered}: {error}")),
            };
            match is_inside(root, &lints) {
                Ok(false) => {}
                Ok(true) => {
                    return Some(format!(
                        "the entry {rendered} names `{}`, which is inside the workspace, so a \
                         change to its sources counts as code and a change to the lints could \
                         land beside code. Point `path` at a trellis checkout outside the \
                         workspace, or use the `git` form",
                        lints.display()
                    ));
                }
                Err(error) => return Some(format!("the entry {rendered}: {error}")),
            }
            if name.as_deref() == Some(LINTS_PACKAGE) {
                None
            } else {
                Some(format!(
                    "the entry {rendered} names `{}`, whose `package.name` is {}, not \
                     `{LINTS_PACKAGE}`",
                    lints_manifest.display(),
                    name.map_or_else(|| "missing".to_owned(), |name| format!("`{name}`"))
                ))
            }
        }
        _ => Some(format!(
            "the entry {rendered} has neither exactly the keys `git`, `rev` and `pattern` nor \
             exactly the key `path`"
        )),
    }
}

/// Whether the directory `path` is `root` or inside it, once both resolve their symbolic links,
/// so neither `..` nor a link outside the workspace hides a directory in it.
///
/// # Errors
///
/// Fails if either path can't be resolved.
fn is_inside(root: &Path, path: &Path) -> Result<bool, PolicyError> {
    let resolve = |path: &Path| {
        fs::canonicalize(path).map_err(|source| PolicyError::Read {
            path: path.to_owned(),
            source,
        })
    };
    Ok(resolve(path)?.starts_with(resolve(root)?))
}

/// Whether `rev` is a full commit hash: 40 lowercase hexadecimal digits.
fn is_commit_hash(rev: &str) -> bool {
    rev.len() == 40
        && rev
            .chars()
            .all(|digit| digit.is_ascii_digit() || ('a'..='f').contains(&digit))
}

/// The targets whose builds `.cargo/config.toml` may link with the C runtime statically.
const STATIC_CRT_TARGETS: [&str; 2] = ["x86_64-pc-windows-msvc", "aarch64-pc-windows-msvc"];

/// Reports each setting in `.cargo/config.toml`, if it exists, besides the `+crt-static`
/// `rustflags` of the Windows MSVC targets. Cargo applies the file to every build, lint and test
/// run, so `[env]`, `[build]`, `rustflags`, `[profile]` and the rest could change what the checks
/// see.
fn cargo_config(workspace: &Workspace, violations: &mut Vec<Violation>) -> Result<(), PolicyError> {
    let path = workspace.root().join(".cargo").join("config.toml");
    let exists = fs::exists(&path).map_err(|source| PolicyError::Read {
        path: path.clone(),
        source,
    })?;
    if !exists {
        return Ok(());
    }
    let mut extra = Vec::new();
    extra_settings(
        "",
        &toml_file::read(&path)?,
        &allowed_cargo_config(),
        &mut extra,
    );
    for setting in extra {
        violations.push(Violation::new(
            ".cargo/config.toml".to_owned(),
            format!(
                "`{setting}` isn't allowed. Cargo applies this file to every build, lint and test \
                 run, so a setting here can change what the checks see; the file holds only \
                 `rustflags = [\"-C\", \"target-feature=+crt-static\"]` for the targets {}. \
                 Remove the setting. {}",
                STATIC_CRT_TARGETS
                    .map(|target| format!("`{target}`"))
                    .join(" and "),
                upstream::change_the_rule("policy/src/workspace_shape.rs")
            ),
        ));
    }
    Ok(())
}

/// The settings `.cargo/config.toml` may hold.
fn allowed_cargo_config() -> Table {
    let rustflags = Value::Array(vec![
        Value::String("-C".to_owned()),
        Value::String("target-feature=+crt-static".to_owned()),
    ]);
    let settings = Table::from_iter([("rustflags".to_owned(), rustflags)]);
    let targets =
        STATIC_CRT_TARGETS.map(|target| (target.to_owned(), Value::Table(settings.clone())));
    Table::from_iter([("target".to_owned(), Value::Table(Table::from_iter(targets)))])
}

/// Adds to `extra` each setting in `actual`, a table whose keys start with `prefix`, that `allowed`
/// doesn't hold with the same value, written as `key = value` with every table on the way in the
/// key.
fn extra_settings(prefix: &str, actual: &Table, allowed: &Table, extra: &mut Vec<String>) {
    for (key, value) in actual {
        let name = format!("{prefix}{}", toml_file::render_key(key));
        match (value, allowed.get(key)) {
            (Value::Table(actual), allowed) => {
                let allowed = allowed
                    .and_then(Value::as_table)
                    .cloned()
                    .unwrap_or_default();
                extra_settings(&format!("{name}."), actual, &allowed, extra);
            }
            (value, Some(allowed)) if value == allowed => {}
            (value, _) => extra.push(format!("{name} = {}", toml_file::render(value))),
        }
    }
}

/// Reports each per-package override in the release profile `release`, in the TOML file `file`,
/// that turns off overflow checks or aborting on panic.
fn package_overrides(file: &str, release: &Table, violations: &mut Vec<Violation>) {
    let Some(overrides) = release.get("package").and_then(Value::as_table) else {
        return;
    };
    for (package, settings) in overrides {
        if let Some(settings) = settings.as_table() {
            let name = format!("profile.release.package.{package}");
            weakened_settings(file, &name, settings, violations);
        }
    }
}

/// Reports a setting in the profile table `settings`, written `[name]` in the TOML file `file`,
/// that turns off overflow checks or aborting on panic.
fn weakened_settings(file: &str, name: &str, settings: &Table, violations: &mut Vec<Violation>) {
    if let Some(value) = settings.get("overflow-checks")
        && *value != Value::Boolean(true)
    {
        violations.push(Violation::new(
            file.to_owned(),
            format!(
                "`[{name}]` sets `overflow-checks = {}`. Release builds keep overflow checks, so an \
                 overflow stops the program rather than wraps; remove the setting.",
                toml_file::render(value)
            ),
        ));
    }
    if let Some(value) = settings.get("panic")
        && *value != Value::String("abort".to_owned())
    {
        violations.push(Violation::new(
            file.to_owned(),
            format!(
                "`[{name}]` sets `panic = {}`. Release builds abort on panic; remove the setting.",
                toml_file::render(value)
            ),
        ));
    }
}

/// Reports every package that `cargo metadata` found by path but that isn't a workspace member.
fn path_dependencies(workspace: &Workspace, violations: &mut Vec<Violation>) {
    let metadata = workspace.metadata();
    for package in &metadata.packages {
        if package.source.is_some() || metadata.workspace_members.contains(&package.id) {
            continue;
        }
        let mut dependents = BTreeSet::new();
        let nodes = metadata.resolve.iter().flat_map(|resolve| &resolve.nodes);
        for node in nodes {
            if node
                .deps
                .iter()
                .any(|dependency| dependency.pkg == package.id)
            {
                dependents.insert(format!("`{}`", metadata[&node.id].name));
            }
        }
        let dependents: Vec<String> = dependents.into_iter().collect();
        violations.push(Violation::new(
            workspace.relative(package.manifest_path.as_std_path()),
            format!(
                "`{}` is a path dependency of {}, but not a workspace member, so it builds without \
                 the workspace's lints and checks. Move it under `crates/` as a member, or remove \
                 the dependency.",
                package.name,
                dependents.join(", ")
            ),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extra_in(text: &str) -> Vec<String> {
        let mut extra = Vec::new();
        extra_settings(
            "",
            &toml_file::parse(text).unwrap(),
            &allowed_cargo_config(),
            &mut extra,
        );
        extra
    }

    const REV: &str = "0123456789abcdef0123456789abcdef01234567";

    fn dylint_problem_in(libraries: &str) -> Option<String> {
        let text = format!("[workspace.metadata.dylint]\nlibraries = {libraries}\n");
        dylint_problem(Path::new("/nonexistent"), &toml_file::parse(&text).unwrap())
    }

    #[test]
    fn a_missing_or_empty_exclude_passes() {
        for text in ["[workspace]\n", "[workspace]\nexclude = []\n"] {
            assert_eq!(
                exclude_problem(&toml_file::parse(text).unwrap()),
                None,
                "{text:?} excludes nothing"
            );
        }
    }

    #[test]
    fn any_excluded_crate_fails() {
        for text in [
            "[workspace]\nexclude = [\"lints\"]\n",
            "[workspace]\nexclude = \"tools\"\n",
        ] {
            let problem = exclude_problem(&toml_file::parse(text).unwrap());
            assert!(
                problem
                    .as_deref()
                    .is_some_and(|problem| problem.contains("must be missing or empty")),
                "{text:?} should fail, found {problem:?}"
            );
        }
    }

    #[test]
    fn the_pinned_trellis_lints_pass() {
        let libraries =
            format!("[{{ git = \"{LINTS_GIT}\", rev = \"{REV}\", pattern = \"lints\" }}]");
        assert_eq!(
            dylint_problem_in(&libraries),
            None,
            "the trellis lints at a full commit hash pass"
        );
    }

    #[test]
    fn other_dylint_libraries_fail() {
        let pinned = format!("git = \"{LINTS_GIT}\", rev = \"{REV}\", pattern = \"lints\"");
        let unreadable = format!(
            "couldn't read {}",
            Path::new("/nonexistent")
                .join("lints")
                .join("Cargo.toml")
                .display()
        );
        for (libraries, problem) in [
            ("[]".to_owned(), "must hold exactly one entry"),
            (
                format!("[{{ {pinned} }}, {{ path = \"x\" }}]"),
                "must hold exactly one entry",
            ),
            (
                format!("[{{ git = \"{LINTS_GIT}.git\", rev = \"{REV}\", pattern = \"lints\" }}]"),
                "doesn't load `https://github.com/265866/trellis`",
            ),
            (
                format!("[{{ git = \"{LINTS_GIT}\", rev = \"main\", pattern = \"lints\" }}]"),
                "full lowercase commit hash",
            ),
            (
                format!(
                    "[{{ git = \"{LINTS_GIT}\", rev = \"{}\", pattern = \"lints\" }}]",
                    REV.to_ascii_uppercase()
                ),
                "full lowercase commit hash",
            ),
            (
                format!("[{{ git = \"{LINTS_GIT}\", rev = \"{REV}\", pattern = \"other\" }}]"),
                "the pattern `lints`",
            ),
            (
                format!("[{{ git = \"{LINTS_GIT}\", rev = \"{REV}\" }}]"),
                "neither exactly the keys",
            ),
            (
                format!("[{{ {pinned}, branch = \"main\" }}]"),
                "neither exactly the keys",
            ),
            ("[{ path = 1 }]".to_owned(), "isn't a string"),
            ("[{ path = \"lints\" }]".to_owned(), unreadable.as_str()),
            ("\"lints\"".to_owned(), "must hold exactly one entry"),
        ] {
            let found = dylint_problem_in(&libraries);
            assert!(
                found
                    .as_deref()
                    .is_some_and(|found| found.contains(problem)),
                "{libraries} should fail with {problem:?}, found {found:?}"
            );
        }
        let found = dylint_problem(Path::new("/"), &Table::new());
        assert!(
            found
                .as_deref()
                .is_some_and(|found| found.contains("it is missing")),
            "a workspace without dylint libraries fails, found {found:?}"
        );
    }

    #[test]
    fn the_static_crt_flags_pass() {
        let text = "[target.x86_64-pc-windows-msvc]\nrustflags = [\"-C\", \
                    \"target-feature=+crt-static\"]\n\n[target.aarch64-pc-windows-msvc]\n\
                    rustflags = [\"-C\", \"target-feature=+crt-static\"]\n";
        assert_eq!(
            extra_in(text),
            Vec::<String>::new(),
            "the pinned settings pass"
        );
        assert_eq!(
            extra_in(
                "[target.x86_64-pc-windows-msvc]\nrustflags = [\"-C\", \"target-feature=+crt-static\"]\n"
            ),
            Vec::<String>::new(),
            "dropping a pinned setting loosens nothing"
        );
    }

    #[test]
    fn any_other_setting_fails() {
        for (text, setting) in [
            (
                "[env]\nCLIPPY_CONF_DIR = \"x\"\n",
                "env.CLIPPY_CONF_DIR = \"x\"",
            ),
            (
                "[build]\nrustflags = [\"-A\", \"warnings\"]\n",
                "build.rustflags = [\"-A\", \"warnings\"]",
            ),
            (
                "[target.'cfg(all())']\nrustflags = [\"-Awarnings\"]\n",
                "target.\"cfg(all())\".rustflags = [\"-Awarnings\"]",
            ),
            (
                "[target.x86_64-pc-windows-msvc]\nrustflags = [\"-C\", \"target-feature=+crt-static\", \"-Awarnings\"]\n",
                "target.x86_64-pc-windows-msvc.rustflags = [\"-C\", \"target-feature=+crt-static\", \"-Awarnings\"]",
            ),
            (
                "[target.x86_64-pc-windows-msvc]\nlinker = \"x\"\n",
                "target.x86_64-pc-windows-msvc.linker = \"x\"",
            ),
            (
                "[profile.release]\noverflow-checks = false\n",
                "profile.release.overflow-checks = false",
            ),
            ("[alias]\ntest = \"check\"\n", "alias.test = \"check\""),
        ] {
            assert_eq!(
                extra_in(text),
                [setting.to_owned()],
                "{text:?} should be reported as {setting}"
            );
        }
    }
}
