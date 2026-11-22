//! `ChangedFiles` on real git histories: which branches mix rule changes with code changes.

use std::path::Path;
use std::process::Command;
use std::{env, fs};

use tempfile::TempDir;
use trellis_policy::{ChangedFiles, Head};

/// A root `Cargo.toml` with a dependency table and a lint table.
const ROOT_MANIFEST: &str = "[workspace]\nmembers = [\"crates/*\"]\n\n\
                             [workspace.dependencies]\nserde = \"1.0.1\"\n\n\
                             [workspace.lints.rust]\nmissing_docs = \"warn\"\n";

/// A member manifest that inherits the workspace lints.
const MEMBER_MANIFEST: &str = "[package]\nname = \"app\"\n\n[lints]\nworkspace = true\n";

/// A git repository in a temporary directory, deleted on drop.
struct Repository {
    /// The repository's directory.
    directory: TempDir,
}

impl Repository {
    /// A repository with one commit holding a workspace with the member `crates/app`, and the
    /// branch `base` at that commit.
    fn new() -> Self {
        let repository = Self {
            directory: TempDir::new().expect("the temporary directory should be created"),
        };
        repository.git(&["init", "--quiet"]);
        repository.write("Cargo.toml", ROOT_MANIFEST);
        repository.write("crates/app/Cargo.toml", MEMBER_MANIFEST);
        repository.write("crates/app/src/lib.rs", "//! The app.\n");
        repository.write(".github/workflows/check.yml", "on: pull_request\n");
        repository.commit("base commit");
        repository.git(&["branch", "base"]);
        repository
    }

    /// The repository's directory.
    fn path(&self) -> &Path {
        self.directory.path()
    }

    /// Writes `contents` to `path` in the working tree.
    fn write(&self, path: &str, contents: &str) {
        let path = self.path().join(path);
        let directory = path
            .parent()
            .expect("a file in the repository has a directory");
        fs::create_dir_all(directory).expect("the directory should be created");
        fs::write(path, contents).expect("the file should be written");
    }

    /// Commits every change in the working tree.
    fn commit(&self, message: &str) {
        self.git(&["add", "--all"]);
        self.commit_index(message);
    }

    /// Commits git's index as it is, whatever the working tree holds.
    fn commit_index(&self, message: &str) {
        self.git(&["commit", "--quiet", "--no-verify", "--message", message]);
    }

    /// Puts `contents` in git's index at `path` with the file mode `mode`, such as `120000` for a
    /// symbolic link, without writing the working tree, so a path the file system can't hold, such
    /// as a name in other letters beside one it already holds or a name Windows reads as another,
    /// is in the next commit on every system.
    fn stage(&self, mode: &str, path: &str, contents: &str) {
        let id = git_with_input(self.path(), &["hash-object", "-w", "--stdin"], contents);
        self.stage_entry(mode, id.trim(), path);
    }

    /// Puts the git object `id` in git's index at `path` with the file mode `mode`.
    fn stage_entry(&self, mode: &str, id: &str, path: &str) {
        self.git(
            &[
                &INDEX_AS_WRITTEN[..],
                &[
                    "update-index",
                    "--add",
                    "--cacheinfo",
                    &format!("{mode},{id},{path}"),
                ],
            ]
            .concat(),
        );
    }

    /// Moves `crates/<from>` to `crates/<to>` in git's index only, and points the app's dependency
    /// at it there, so the move can go to a name the file system can't hold.
    fn move_in_index(&self, from: &str, to: &str) {
        self.rename_in_index(&format!("crates/{from}"), &format!("crates/{to}"));
        self.stage(
            "100644",
            "crates/app/Cargo.toml",
            &format!("{MEMBER_MANIFEST}\n[dependencies]\nengine = {{ path = \"../{to}\" }}\n"),
        );
    }

    /// Moves every entry below the directory `from` to the directory `to` in git's index only, so
    /// an entry the working tree doesn't hold, such as a symbolic link on Windows, moves too.
    fn rename_in_index(&self, from: &str, to: &str) {
        let from = format!("{from}/");
        let listed = git_with_input(
            self.path(),
            &[
                &INDEX_AS_WRITTEN[..],
                &["ls-files", "--stage", "-z", "--", &from],
            ]
            .concat(),
            "",
        );
        let entries = listed.split('\0').filter(|entry| !entry.is_empty());
        for entry in entries {
            let (metadata, path) = entry.split_once('\t').expect("an entry has a path");
            let mut fields = metadata.split(' ');
            let (Some(mode), Some(id)) = (fields.next(), fields.next()) else {
                panic!("an entry has a mode and an id: {entry}");
            };
            let rest = path
                .strip_prefix(&from)
                .expect("the entry is in the directory");
            self.stage_entry(mode, id, &format!("{to}/{rest}"));
            self.git(
                &[
                    &INDEX_AS_WRITTEN[..],
                    &["update-index", "--force-remove", "--", path],
                ]
                .concat(),
            );
        }
    }

    /// Runs git in the repository, as [`git`] does.
    fn git(&self, arguments: &[&str]) {
        git(self.path(), arguments);
    }

    /// Makes `path` a git repository of its own with one commit holding the package `sub`, which
    /// `git add` in this repository adds as a submodule.
    fn nested_repository(&self, path: &str) {
        self.write(&format!("{path}/Cargo.toml"), "[package]\nname = \"sub\"\n");
        self.write(
            &format!("{path}/src/lib.rs"),
            "//! A crate in a submodule.\n",
        );
        let nested = self.path().join(path);
        git(&nested, &["init", "--quiet"]);
        git(&nested, &["add", "--all"]);
        git(
            &nested,
            &["commit", "--quiet", "--no-verify", "--message", "nested"],
        );
    }

    /// The files the commits on `HEAD` change since `base`.
    fn changed_files(&self) -> ChangedFiles {
        self.changed_files_to(&Head::Commit("HEAD".to_owned()))
    }

    /// The files changed from the merge base of `base` and `head` to `head`.
    fn changed_files_to(&self, head: &Head) -> ChangedFiles {
        ChangedFiles::between(self.path(), "base", head)
            .expect("git should list the changed files")
            .expect("the base branch should resolve")
    }

    /// Whether comparing with the unknown base `origin/main` finds nothing to compare.
    fn has_nothing_to_compare(&self) -> bool {
        ChangedFiles::between(self.path(), "origin/main", &Head::WorkingTree)
            .expect("an unknown base without remote branches isn't an error")
            .is_none()
    }
}

/// Runs git in `directory`, with a fixed identity and no signing, so the user's git configuration
/// doesn't matter.
fn git(directory: &Path, arguments: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args([
            "-c",
            "user.name=Policy Test",
            "-c",
            "user.email=policy@example.com",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.autocrlf=false",
        ])
        .args(arguments)
        .output()
        .expect("git should run");
    assert!(
        output.status.success(),
        "git {arguments:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The settings that make git keep a path in its index as written: `core.ignorecase` off, so a
/// name in other letters stays apart, `core.protectNTFS` off, so Windows' git takes a name it
/// can't write to disk, and `core.precomposeunicode` off, so macOS's git keeps a name's form.
const INDEX_AS_WRITTEN: [&str; 6] = [
    "-c",
    "core.ignorecase=false",
    "-c",
    "core.protectNTFS=false",
    "-c",
    "core.precomposeunicode=false",
];

/// Runs git in `directory` with `arguments` and `input` on its standard input, and returns its
/// output.
fn git_with_input(directory: &Path, arguments: &[&str], input: &str) -> String {
    use std::io::Write as _;
    use std::process::Stdio;

    let mut child = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("git should run");
    child
        .stdin
        .take()
        .expect("git's input is piped")
        .write_all(input.as_bytes())
        .expect("git should read its input");
    let output = child.wait_with_output().expect("git should finish");
    assert!(
        output.status.success(),
        "git {arguments:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("git's output should be UTF-8")
}

#[test]
fn a_new_crate_is_code_and_not_a_rule_change() {
    let repository = Repository::new();
    repository.write(
        "crates/rules/Cargo.toml",
        "[package]\nname = \"rules\"\n\n[lints]\nworkspace = true\n",
    );
    repository.write("crates/rules/src/lib.rs", "//! Stop rules.\n");
    repository.write("crates/app/src/lib.rs", "//! The app, with rules.\n");
    repository.commit("add a crate");

    let changed = repository.changed_files();
    assert!(
        changed.rules().is_empty(),
        "a new crate isn't a rule change: {:?}",
        changed.rules()
    );
    assert_eq!(
        changed.code().len(),
        3,
        "the new crate's files and the changed source are code: {:?}",
        changed.code()
    );
    assert!(
        !changed.mixes_rules_and_code(),
        "a branch of code alone passes"
    );
}

#[test]
fn changing_a_member_lints_table_is_code() {
    let repository = Repository::new();
    repository.write("crates/app/Cargo.toml", "[package]\nname = \"app\"\n");
    repository.write("crates/app/src/lib.rs", "//! The app, changed.\n");
    repository.commit("drop the lints table");

    let changed = repository.changed_files();
    assert!(
        !changed.mixes_rules_and_code(),
        "the policy tests, not this check, enforce the shape of a member's `[lints]`: {:?}",
        changed.rules()
    );
}

#[test]
fn a_workspace_dependency_change_with_code_passes() {
    let repository = Repository::new();
    repository.write(
        "Cargo.toml",
        &ROOT_MANIFEST.replace("serde = \"1.0.1\"", "serde = \"1.0.2\"\ntoml = \"0.9\""),
    );
    repository.write("crates/app/src/lib.rs", "//! The app, on a newer serde.\n");
    repository.commit("bump serde");

    let changed = repository.changed_files();
    assert!(
        changed.rules().is_empty(),
        "a change inside [workspace.dependencies] isn't a rule change: {:?}",
        changed.rules()
    );
}

#[test]
fn a_root_manifest_change_outside_dependencies_with_code_fails() {
    let repository = Repository::new();
    repository.write(
        "Cargo.toml",
        &ROOT_MANIFEST.replace("missing_docs = \"warn\"", "missing_docs = \"allow\""),
    );
    repository.write("crates/app/src/lib.rs", "pub fn undocumented() {}\n");
    repository.commit("lower a lint");

    let changed = repository.changed_files();
    assert_eq!(
        changed.rules(),
        ["Cargo.toml (outside [workspace.dependencies] and [patch])".to_owned()],
        "lowering a workspace lint is a rule change"
    );
    assert!(
        changed.mixes_rules_and_code(),
        "a rule change with code fails"
    );
}

#[test]
fn a_tool_configuration_file_in_a_crate_with_code_fails() {
    let repository = Repository::new();
    repository.write(
        "crates/app/clippy.toml",
        "avoid-breaking-exported-api = true\n",
    );
    repository.write("crates/app/src/lib.rs", "//! The app, changed.\n");
    repository.commit("override clippy for one crate");

    let changed = repository.changed_files();
    assert_eq!(
        changed.rules(),
        ["crates/app/clippy.toml".to_owned()],
        "a nested clippy.toml is a rule change"
    );
    assert!(
        changed.mixes_rules_and_code(),
        "a rule change with code fails"
    );
}

#[test]
fn a_rename_counts_as_both_paths() {
    let repository = Repository::new();
    repository.git(&["mv", ".github/workflows/check.yml", "crates/app/check.yml"]);
    repository.commit("move the workflow into the app");

    let changed = repository.changed_files();
    assert_eq!(
        changed.rules(),
        [".github/workflows/check.yml".to_owned()],
        "the deleted rule file is a rule change, though git would see a rename"
    );
    assert_eq!(
        changed.code(),
        ["crates/app/check.yml".to_owned()],
        "the added file is code"
    );
}

#[test]
fn changes_on_the_base_branch_after_the_merge_base_dont_count() {
    let repository = Repository::new();
    repository.git(&["checkout", "--quiet", "-b", "feature"]);
    repository.write("crates/app/src/lib.rs", "//! The app, changed.\n");
    repository.commit("change the app");
    repository.git(&["checkout", "--quiet", "base"]);
    repository.write(
        "Cargo.toml",
        &ROOT_MANIFEST.replace("missing_docs = \"warn\"", "missing_docs = \"deny\""),
    );
    repository.commit("raise a lint on the base branch");
    repository.git(&["checkout", "--quiet", "feature"]);

    let changed = repository.changed_files();
    assert!(
        changed.rules().is_empty(),
        "a rule change made on the base branch isn't the feature branch's: {:?}",
        changed.rules()
    );
}

#[test]
fn an_unknown_base_fails_once_a_remote_branch_exists() {
    let repository = Repository::new();
    repository.git(&["update-ref", "refs/remotes/origin/base", "HEAD"]);
    let error =
        ChangedFiles::between(repository.path(), "no-such-branch", &Head::WorkingTree).unwrap_err();
    assert!(
        error.to_string().contains("merge-base"),
        "the error should name the failing git command: {error}"
    );
}

#[test]
fn an_unknown_base_without_a_remote_has_nothing_to_compare() {
    let repository = Repository::new();
    assert!(
        repository.has_nothing_to_compare(),
        "a repository with no remote has nothing to compare"
    );
}

#[test]
fn an_unknown_base_before_the_first_push_has_nothing_to_compare() {
    let repository = Repository::new();
    repository.git(&[
        "remote",
        "add",
        "origin",
        "https://github.com/example/app.git",
    ]);
    assert!(
        repository.has_nothing_to_compare(),
        "a remote that nothing was pushed to has no branch to compare with"
    );
}

/// A file whose name ends in a dot is a rule wherever it sits, since Windows reads the name
/// without the dot. An untracked one is read from the working tree, and Windows stores such a file
/// under the shorter name, so there the test has no such path and stops; Linux and macOS run it.
/// [`a_committed_path_windows_reads_as_another_is_a_rule_beside_code`] runs everywhere.
#[test]
fn a_path_windows_reads_as_another_is_a_rule_beside_code() {
    let repository = Repository::new();
    let path = "crates/app/src/notes.txt.";
    repository.write(path, "notes\n");
    let stored: Vec<_> = fs::read_dir(repository.path().join("crates/app/src"))
        .expect("the directory should be read")
        .map(|entry| entry.expect("the entry should be read").file_name())
        .collect();
    if !stored.iter().any(|name| name == "notes.txt.") {
        return;
    }
    repository.write("crates/app/src/lib.rs", "//! The app, changed.\n");

    let changed = repository.changed_files_to(&Head::WorkingTree);
    assert_eq!(
        changed.rules(),
        [format!(
            "{path} (untracked; commit it in a rules pull request, or ignore it in \
             .git/info/exclude)"
        )],
        "an untracked one says how to settle it"
    );
    assert!(changed.mixes_rules_and_code(), "beside code, it fails");

    repository.commit("add the file and change the code");
    let changed = repository.changed_files();
    assert_eq!(changed.rules(), [path], "a committed one is a rule too");
    assert_eq!(changed.code(), ["crates/app/src/lib.rs"]);
    assert!(changed.mixes_rules_and_code(), "beside code, it fails");
}

/// A committed file whose name ends in a dot is a rule too. The commit is built in git's index, so
/// the test runs on Windows as well.
#[test]
fn a_committed_path_windows_reads_as_another_is_a_rule_beside_code() {
    let repository = Repository::new();
    let path = "crates/app/src/notes.txt.";
    repository.stage("100644", path, "notes\n");
    repository.stage("100644", "crates/app/src/lib.rs", "//! The app, changed.\n");
    repository.commit_index("add the file and change the code");
    let changed = repository.changed_files();
    assert_eq!(changed.rules(), [path], "a committed one is a rule");
    assert_eq!(changed.code(), ["crates/app/src/lib.rs"]);
    assert!(changed.mixes_rules_and_code(), "beside code, it fails");
}

#[test]
fn the_working_tree_counts_staged_unstaged_and_untracked_files() {
    let repository = Repository::new();
    repository.write("crates/app/src/lib.rs", "//! The app, committed.\n");
    repository.commit("change the app");
    repository.write("clippy.toml", "too-many-arguments-threshold = 20\n");
    repository.git(&["add", "clippy.toml"]);
    repository.write(".github/workflows/check.yml", "on: push\n");
    repository.write("deny.toml", "[bans]\n");
    repository.write("target/debug/clippy.toml", "ignored by .gitignore\n");
    repository.write(".gitignore", "/target\n");

    let changed = repository.changed_files_to(&Head::WorkingTree);
    assert_eq!(
        changed.rules(),
        [
            ".github/workflows/check.yml",
            ".gitignore",
            "clippy.toml",
            "deny.toml"
        ],
        "staged, unstaged and untracked rule files count, and ignored ones don't"
    );
    assert_eq!(
        changed.code(),
        ["crates/app/src/lib.rs"],
        "the committed code change counts"
    );
    assert!(
        repository.changed_files().rules().is_empty(),
        "comparing the commits alone sees no rule change"
    );
}

#[test]
fn an_untracked_file_no_list_knows_is_named_with_its_remedy() {
    let repository = Repository::new();
    repository.write(".tool-versions", "just 1\n");
    repository.commit("add a tool versions file");
    repository.write(".tool-versions", "just 2\n");
    repository.write(".envrc", "export A=1\n");
    repository.write("crates/app/src/lib.rs", "//! The app, changed.\n");

    let changed = repository.changed_files_to(&Head::WorkingTree);
    assert_eq!(
        changed.rules(),
        [
            ".envrc (untracked; commit it in a rules pull request, or ignore it in \
             .git/info/exclude)",
            ".tool-versions",
        ],
        "an untracked file is still a rule, and the listing says how to get it out of the way"
    );
    assert!(
        changed.mixes_rules_and_code(),
        "the untracked file still counts"
    );
}

#[test]
fn an_untracked_file_whose_deletion_is_staged_has_no_remedy() {
    let repository = Repository::new();
    repository.write(".envrc", "export A=1\n");
    repository.commit("add an environment file");
    repository.git(&["branch", "--force", "base"]);
    repository.git(&["rm", "--cached", "--quiet", ".envrc"]);
    repository.write("crates/app/src/lib.rs", "//! The app, changed.\n");

    let changed = repository.changed_files_to(&Head::WorkingTree);
    assert_eq!(
        changed.rules(),
        [".envrc"],
        "ignoring the file leaves its staged deletion, so the listing names no remedy"
    );
    assert!(
        changed.mixes_rules_and_code(),
        "the staged deletion still counts"
    );
}

#[test]
fn a_build_script_of_a_member_outside_crates_is_a_rule() {
    let repository = Repository::new();
    repository.write(
        "Cargo.toml",
        &ROOT_MANIFEST.replace("[\"crates/*\"]", "[\"crates/*\", \"tools/gen\"]"),
    );
    repository.write(
        "tools/gen/Cargo.toml",
        "[package]\nname = \"gen\"\n\n[lints]\nworkspace = true\n",
    );
    repository.write("tools/gen/build.rs", "fn main() {}\n");
    repository.write("tools/gen/src/lib.rs", "//! Generates.\n");
    repository.commit("add a member outside crates");
    repository.git(&["branch", "--force", "base"]);
    repository.write(
        "tools/gen/build.rs",
        "fn main() {\n    println!(\"cargo::rustc-cfg=loose\");\n}\n",
    );
    repository.write("tools/gen/src/lib.rs", "//! Generates, changed.\n");
    repository.commit("change the build script and the code");

    let changed = repository.changed_files();
    assert_eq!(
        changed.rules(),
        ["tools/gen/build.rs"],
        "Cargo runs the build script of a member wherever the member lives"
    );
    assert_eq!(
        changed.code(),
        ["tools/gen/src/lib.rs"],
        "the member's source is code"
    );
    assert!(
        changed.mixes_rules_and_code(),
        "a build script change beside code fails"
    );
}

#[test]
fn a_module_named_build_is_code() {
    let repository = Repository::new();
    repository.write("crates/app_core/Cargo.toml", MEMBER_MANIFEST);
    repository.write(
        "crates/app_core/tests/fixtures/proj/Cargo.toml",
        NESTED_MANIFEST,
    );
    repository.commit("add a crate that nests a package");
    repository.git(&["branch", "--force", "base"]);
    repository.write("crates/app/src/lib.rs", "//! The app.\n\nmod build;\n");
    repository.write("crates/app/src/build.rs", "//! Builds things.\n");
    repository.write("crates/app/tests/build.rs", "//! Tests the build.\n");
    repository.commit("add a build module");

    let changed = repository.changed_files();
    assert!(
        changed.rules().is_empty(),
        "a crate that nests no package has no build script below `src/` or `tests/`, whatever \
         another crate nests: {:?}",
        changed.rules()
    );
    assert_eq!(
        changed.code(),
        [
            "crates/app/src/build.rs",
            "crates/app/src/lib.rs",
            "crates/app/tests/build.rs"
        ],
        "the module, its parent and the test are code"
    );
}

#[test]
fn the_build_script_of_a_package_nested_in_a_crate_is_a_rule() {
    for directory in ["crates/app/src/gen", "crates/app/tests/gen"] {
        let repository = Repository::new();
        repository.write(
            "Cargo.toml",
            &ROOT_MANIFEST.replace(
                "[\"crates/*\"]",
                &format!("[\"crates/*\", \"{directory}\"]"),
            ),
        );
        repository.write(
            &format!("{directory}/Cargo.toml"),
            "[package]\nname = \"gen\"\n\n[lints]\nworkspace = true\n",
        );
        repository.write(&format!("{directory}/build.rs"), "fn main() {}\n");
        repository.write(&format!("{directory}/src/lib.rs"), "//! Generates.\n");
        repository.commit("add a package inside a crate");
        repository.git(&["branch", "--force", "base"]);
        repository.write(&format!("{directory}/build.rs"), LOOSE_BUILD_SCRIPT);
        repository.write("crates/app/src/lib.rs", "//! The app, changed.\n");
        repository.commit("change the build script and the code");

        let changed = repository.changed_files();
        assert_eq!(
            changed.rules(),
            [format!("{directory}/build.rs")],
            "Cargo runs the `build.rs` beside a package's `Cargo.toml`"
        );
        assert_eq!(
            changed.code(),
            ["crates/app/src/lib.rs"],
            "the crate's own source is code"
        );
    }
}

/// A build script that loosens the code it builds.
const LOOSE_BUILD_SCRIPT: &str = "fn main() {\n    println!(\"cargo::rustc-cfg=loose\");\n}\n";

/// The manifest of a package nested inside a crate.
const NESTED_MANIFEST: &str = "[package]\nname = \"gen\"\n";

/// The paths of a nested package's manifest and of a `build.rs` that a case-insensitive file
/// system puts in the same directory, though their letters differ in the package's directory or
/// in the crate's.
const CASE_SPLIT_LAYOUTS: [(&str, &str); 2] = [
    (
        "crates/app/src/Gen/Cargo.toml",
        "crates/app/src/gen/build.rs",
    ),
    (
        "crates/App/src/gen/Cargo.toml",
        "crates/app/src/gen/build.rs",
    ),
];

/// Asserts that `rules` is the one build script written at `build_script` beside the package
/// manifest at `manifest`. A file system that keeps case reports it at `build_script`, in another
/// directory than the manifest. One that ignores case stores it under the spelling the manifest's
/// directory already has, so there the path matches in any case.
fn assert_the_build_script_is_the_rule(
    repository: &Repository,
    manifest: &str,
    build_script: &str,
    rules: &[String],
) {
    let beside = Path::new(build_script).with_file_name("Cargo.toml");
    let ignores_case = fs::exists(repository.path().join(beside))
        .expect("the file system should say whether the manifest is there");
    if ignores_case {
        assert!(
            matches!(rules, [rule] if rule.eq_ignore_ascii_case(build_script)),
            "a case-insensitive file system puts {build_script} beside {manifest}, but the rules \
             are {rules:?}"
        );
    } else {
        assert_eq!(
            rules,
            [build_script],
            "a case-sensitive file system keeps {build_script} apart from {manifest}"
        );
    }
}

/// A repository whose `base` adds the nested package manifest at `manifest`.
fn repository_with_nested_manifest(manifest: &str) -> Repository {
    let repository = Repository::new();
    repository.write(manifest, NESTED_MANIFEST);
    repository.commit("add a package inside a crate");
    repository.git(&["branch", "--force", "base"]);
    repository
}

#[test]
fn a_committed_build_rs_whose_package_directory_differs_in_case_is_a_rule() {
    for (manifest, build_script) in CASE_SPLIT_LAYOUTS {
        let repository = repository_with_nested_manifest(manifest);
        repository.write(build_script, LOOSE_BUILD_SCRIPT);
        repository.write("crates/app/src/lib.rs", "//! The app, changed.\n");
        repository.commit("add the build script and change the code");

        let changed = repository.changed_files();
        assert_the_build_script_is_the_rule(&repository, manifest, build_script, changed.rules());
        assert!(changed.mixes_rules_and_code(), "beside code, it fails");
    }
}

#[test]
fn a_working_tree_build_rs_whose_package_directory_differs_in_case_is_a_rule() {
    for (manifest, build_script) in CASE_SPLIT_LAYOUTS {
        let repository = repository_with_nested_manifest(manifest);
        repository.write(build_script, LOOSE_BUILD_SCRIPT);
        repository.write("crates/app/src/lib.rs", "//! The app, changed.\n");

        let changed = repository.changed_files_to(&Head::WorkingTree);
        assert_the_build_script_is_the_rule(&repository, manifest, build_script, changed.rules());
        assert!(changed.mixes_rules_and_code(), "beside code, it fails");
    }
}

#[test]
fn a_build_module_is_a_rule_in_a_crate_that_nests_a_package_elsewhere() {
    let repository = repository_with_nested_manifest("crates/app/tests/fixtures/proj/Cargo.toml");
    repository.write("crates/app/src/lib.rs", "//! The app.\n\nmod build;\n");
    repository.write("crates/app/src/build.rs", "//! Builds things.\n");
    repository.commit("add a build module");

    assert_eq!(
        repository.changed_files().rules(),
        ["crates/app/src/build.rs"],
        "a crate that nests a package anywhere may hold a build script anywhere below it"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn a_build_rs_is_a_rule_when_git_cannot_list_its_crate() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt as _;

    for (manifest, build_script) in [
        (
            Some("crates/app/src/gen/Cargo.toml"),
            "crates/app/src/gen/build.rs",
        ),
        (None, "crates/app/src/build.rs"),
    ] {
        let repository = Repository::new();
        if let Some(manifest) = manifest {
            repository.write(manifest, NESTED_MANIFEST);
        }
        repository.write(build_script, "fn main() {}\n");
        let directory = build_script
            .rsplit_once('/')
            .map_or("", |(directory, _)| directory);
        fs::write(
            repository
                .path()
                .join(directory)
                .join(OsStr::from_bytes(b"data\xff")),
            "",
        )
        .expect("Linux file systems accept a name that isn't UTF-8");
        repository.commit("add a build script beside a name that isn't UTF-8");
        repository.git(&["branch", "--force", "base"]);
        repository.write(build_script, LOOSE_BUILD_SCRIPT);
        repository.write("crates/app/src/lib.rs", "//! The app, changed.\n");
        repository.commit("change the build script and the code");

        assert_eq!(
            repository.changed_files().rules(),
            [build_script],
            "a listing git can't give as UTF-8 counts as a package beside {build_script}"
        );
    }
}

#[test]
fn a_manifest_on_either_side_makes_a_nested_build_rs_a_rule() {
    for manifest in ["Cargo.toml", "cargo.toml"] {
        let repository = Repository::new();
        repository.write(&format!("crates/app/src/gen/{manifest}"), MEMBER_MANIFEST);
        repository.write("crates/app/src/gen/build.rs", "fn main() {}\n");
        assert_eq!(
            repository.changed_files_to(&Head::WorkingTree).rules(),
            ["crates/app/src/gen/build.rs"],
            "a new package's build script is a rule when its {manifest} is on disk"
        );
        repository.commit("add a package inside a crate");
        repository.git(&["branch", "--force", "base"]);
        repository.git(&["rm", "--quiet", "-r", "crates/app/src/gen"]);
        repository.commit("delete the package");
        assert_eq!(
            repository.changed_files().rules(),
            ["crates/app/src/gen/build.rs"],
            "deleting a package's build script is a rule change when its {manifest} was at the \
             merge base"
        );
    }
}

#[test]
fn the_working_tree_root_manifest_is_compared_as_it_is_on_disk() {
    let repository = Repository::new();
    repository.write(
        "Cargo.toml",
        &ROOT_MANIFEST.replace("missing_docs = \"warn\"", "missing_docs = \"allow\""),
    );

    let changed = repository.changed_files_to(&Head::WorkingTree);
    assert_eq!(
        changed.rules(),
        ["Cargo.toml (outside [workspace.dependencies] and [patch])".to_owned()],
        "an uncommitted lint change in the root manifest is a rule change"
    );
}

#[test]
fn a_head_commit_is_judged_without_checking_it_out() {
    let repository = Repository::new();
    repository.git(&["checkout", "--quiet", "-b", "feature"]);
    repository.write("deny.toml", "[bans]\n");
    repository.write("crates/app/src/lib.rs", "//! The app, changed.\n");
    repository.commit("mix a rule and code");
    repository.git(&["checkout", "--quiet", "base"]);

    let changed = repository.changed_files_to(&Head::Commit("feature".to_owned()));
    assert!(
        changed.mixes_rules_and_code(),
        "the feature branch mixes rules and code, though `base` is checked out: {:?} {:?}",
        changed.rules(),
        changed.code()
    );
    assert!(
        !repository
            .changed_files_to(&Head::WorkingTree)
            .mixes_rules_and_code(),
        "the checked-out base branch changes nothing"
    );
}

#[test]
fn rust_files_outside_crates_are_code_and_the_rule_documents_are_rules() {
    let repository = Repository::new();
    repository.write("docs/notes.rs", "pub fn hidden() {}\n");
    repository.write("AGENTS.md", "# Working on the app\n");
    repository.commit("move code out of crates and change the guidance");

    let changed = repository.changed_files();
    assert_eq!(
        changed.code(),
        ["docs/notes.rs"],
        "Rust outside crates/ is code"
    );
    assert_eq!(changed.rules(), ["AGENTS.md"], "AGENTS.md is a rule");
    assert!(changed.mixes_rules_and_code(), "the two together fail");
}

#[test]
fn docs_and_release_notes_land_with_code() {
    let repository = Repository::new();
    for path in [
        "README.md",
        "CHANGELOG.md",
        "SECURITY.md",
        "LICENSE",
        "Cargo.lock",
        ".editorconfig",
        "docs/install.md",
        "docs/guides/setup.md",
        "docs/img/arch.png",
        "docs/img/arch.svg",
    ] {
        repository.write(path, "Text.\n");
    }
    repository.write("crates/app/src/lib.rs", "//! The app, documented.\n");
    repository.commit("document the app");

    let changed = repository.changed_files();
    assert!(
        changed.rules().is_empty(),
        "the readme, release notes, licenses, lockfile, `.editorconfig`, docs and their images \
         aren't rules: {:?}",
        changed.rules()
    );
    assert_eq!(
        changed.code(),
        ["crates/app/src/lib.rs"],
        "only the crate's source is code"
    );
}

#[test]
fn the_template_answers_are_a_rule() {
    let repository = Repository::new();
    repository.write(".copier-answers.yml", "_commit: v2\n");
    repository.write("crates/app/src/lib.rs", "//! The app, updated.\n");
    repository.commit("update the template and the app");

    let changed = repository.changed_files();
    assert_eq!(
        changed.rules(),
        [".copier-answers.yml"],
        "`copier update` changes the rules through the answers file"
    );
    assert!(changed.mixes_rules_and_code(), "the two together fail");
}

#[test]
fn adding_a_build_script_with_its_build_dependency_is_one_rule_change() {
    let repository = Repository::new();
    repository.write(
        "Cargo.toml",
        &format!("{ROOT_MANIFEST}\n[workspace.metadata.policy]\nbuild_script_crates = [\"app\"]\n"),
    );
    repository.write("crates/app/build.rs", "fn main() {}\n");
    repository.write(
        "crates/app/Cargo.toml",
        "[package]\nname = \"app\"\nbuild = \"build.rs\"\n\n[lints]\nworkspace = true\n\n\
         [build-dependencies]\ncc = \"1\"\n\n[target.'cfg(windows)'.build-dependencies]\n\
         winres = \"0.1\"\n",
    );
    repository.commit("add a build script");

    let changed = repository.changed_files();
    assert_eq!(
        changed.rules(),
        [
            "Cargo.toml (outside [workspace.dependencies] and [patch])".to_owned(),
            "crates/app/Cargo.toml ([build-dependencies], the build key and the license key)"
                .to_owned(),
            "crates/app/build.rs".to_owned(),
        ],
        "the build script, its build dependencies and its policy entry are rules"
    );
    assert!(
        changed.code().is_empty(),
        "nothing else changed: {:?}",
        changed.code()
    );
}

#[test]
fn a_member_manifest_change_beside_its_build_dependencies_is_code() {
    let repository = Repository::new();
    repository.write(
        "crates/app/Cargo.toml",
        &format!(
            "{MEMBER_MANIFEST}\n[dependencies]\nserde = \"1\"\n\n[build-dependencies]\ncc = \"1\"\n"
        ),
    );
    repository.write("crates/app/build.rs", "fn main() {}\n");
    repository.commit("add a build script and a dependency");

    let changed = repository.changed_files();
    assert_eq!(
        changed.code(),
        ["crates/app/Cargo.toml".to_owned()],
        "a manifest change outside the build tables is code"
    );
    assert!(
        changed.mixes_rules_and_code(),
        "beside the build script, it mixes rules and code"
    );
}

#[test]
fn a_comment_in_a_member_manifest_is_neither() {
    let repository = Repository::new();
    repository.write(
        "crates/app/Cargo.toml",
        &format!("# The app.\n{MEMBER_MANIFEST}"),
    );
    repository.commit("comment the manifest");

    let changed = repository.changed_files();
    assert!(
        changed.rules().is_empty() && changed.code().is_empty(),
        "a change that doesn't parse differently is neither: {:?} {:?}",
        changed.rules(),
        changed.code()
    );
}

#[test]
fn a_proc_macro_lands_as_a_rule_change_then_a_code_change() {
    let repository = Repository::new();
    let listed = format!(
        "{ROOT_MANIFEST}\n[workspace.metadata.policy]\nproc_macro_crates = [\"serde_derive\"]\n"
    );
    repository.write("Cargo.toml", &listed);
    repository.commit("list a proc macro");
    let changed = repository.changed_files();
    assert_eq!(
        changed.rules(),
        ["Cargo.toml (outside [workspace.dependencies] and [patch])".to_owned()],
        "the list entry is a rule"
    );
    assert!(changed.code().is_empty(), "{:?}", changed.code());

    repository.git(&["branch", "--force", "base"]);
    repository.write(
        "crates/app/Cargo.toml",
        &format!("{MEMBER_MANIFEST}\n[dependencies]\nserde_derive = \"1\"\n"),
    );
    repository.write(
        "crates/app/src/lib.rs",
        "//! The app.\n\n/// A point.\n#[derive(serde_derive::Serialize)]\npub struct Point;\n",
    );
    repository.commit("derive with the proc macro");
    let changed = repository.changed_files();
    assert!(
        changed.rules().is_empty(),
        "the dependency on a listed proc macro isn't a rule: {:?}",
        changed.rules()
    );
    assert_eq!(
        changed.code(),
        [
            "crates/app/Cargo.toml".to_owned(),
            "crates/app/src/lib.rs".to_owned(),
        ],
        "the dependency lands with its first use"
    );
}

/// The library source the expectation tests start from.
const LIBRARY: &str = "//! The app.\n\npub fn add(a: u8, b: u8) -> u8 {\n    a + b\n}\n";

/// `LIBRARY` with a lint expectation on `add`.
const EXPECTING_LIBRARY: &str = "//! The app.\n\n#[expect(\n    unreachable_pub,\n    \
                                 reason = \"the binary calls it through the plugin table\"\n)]\n\
                                 pub fn add(a: u8, b: u8) -> u8 {\n    a + b\n}\n";

/// A repository whose `crates/app` has the sources `lib.rs`, from `LIBRARY`, and `parse.rs`, with
/// the branch `base` at that commit.
fn repository_with_sources() -> Repository {
    let repository = Repository::new();
    repository.write("crates/app/src/lib.rs", LIBRARY);
    repository.write("crates/app/src/parse.rs", "pub fn parse() {}\n");
    repository.commit("write the app");
    repository.git(&["branch", "--force", "base"]);
    repository
}

#[test]
fn a_change_to_only_lint_expectations_is_neither() {
    let repository = repository_with_sources();
    repository.write("crates/app/src/lib.rs", EXPECTING_LIBRARY);
    repository.commit("expect a lint");

    let changed = repository.changed_files();
    assert!(
        changed.rules().is_empty() && changed.code().is_empty(),
        "an expectation alone is neither: {:?} {:?}",
        changed.rules(),
        changed.code()
    );
}

#[test]
fn a_new_lint_with_expectations_in_existing_code_is_one_rule_change() {
    let repository = repository_with_sources();
    repository.write(
        "Cargo.toml",
        &ROOT_MANIFEST.replace(
            "missing_docs = \"warn\"",
            "missing_docs = \"warn\"\nunreachable_pub = \"warn\"",
        ),
    );
    repository.write("crates/app/src/lib.rs", EXPECTING_LIBRARY);
    repository.write(
        "crates/app/src/parse.rs",
        "#[cfg_attr(dylint_lib = \"trellis_lints\", expect(local_doc_hidden, reason = \"the \
         parser is public only for the fuzz target\"))]\npub fn parse() {}\n",
    );
    repository.commit("add a lint and the expectations it needs");

    let changed = repository.changed_files();
    assert_eq!(
        changed.rules(),
        ["Cargo.toml (outside [workspace.dependencies] and [patch])".to_owned()],
        "the lint is a rule"
    );
    assert!(
        changed.code().is_empty(),
        "the expectations aren't code: {:?}",
        changed.code()
    );
    assert!(!changed.mixes_rules_and_code(), "the pull request passes");
}

#[test]
fn a_code_change_with_an_expectation_it_forces_elsewhere_is_code() {
    let repository = repository_with_sources();
    repository.write("crates/app/src/lib.rs", EXPECTING_LIBRARY);
    repository.write(
        "crates/app/src/parse.rs",
        "pub fn parse() -> u8 {\n    1\n}\n",
    );
    repository.commit("change the parser and expect a lint it raises in the library");

    let changed = repository.changed_files();
    assert!(
        changed.rules().is_empty(),
        "the expectation isn't a rule: {:?}",
        changed.rules()
    );
    assert_eq!(
        changed.code(),
        ["crates/app/src/parse.rs".to_owned()],
        "the code change is code"
    );
    assert!(!changed.mixes_rules_and_code(), "the pull request passes");
}

#[test]
fn an_expectation_beside_a_code_change_in_one_file_is_code() {
    let repository = repository_with_sources();
    repository.write(
        "crates/app/src/lib.rs",
        &EXPECTING_LIBRARY.replace("a + b", "a.wrapping_add(b)"),
    );
    repository.commit("change the library and expect a lint");

    let changed = repository.changed_files();
    assert_eq!(
        changed.code(),
        ["crates/app/src/lib.rs".to_owned()],
        "an expectation beside a code change is code"
    );
    assert!(changed.rules().is_empty(), "{:?}", changed.rules());
}

#[test]
fn inheriting_the_workspace_license_is_one_rule_change() {
    let repository = Repository::new();
    repository.write(
        "Cargo.toml",
        &ROOT_MANIFEST.replace(
            "\n[workspace.dependencies]",
            "\n[workspace.package]\nlicense = \"MIT\"\n\n[workspace.dependencies]",
        ),
    );
    repository.write(
        "crates/app/Cargo.toml",
        &MEMBER_MANIFEST.replace(
            "name = \"app\"\n",
            "name = \"app\"\nlicense.workspace = true\n",
        ),
    );
    repository.write("LICENSE", "MIT License\n");
    repository.commit("license the project");

    let changed = repository.changed_files();
    assert_eq!(
        changed.rules(),
        [
            "Cargo.toml (outside [workspace.dependencies] and [patch])".to_owned(),
            "crates/app/Cargo.toml ([build-dependencies], the build key and the license key)"
                .to_owned(),
        ],
        "the license answer and each crate's license line land in one rule change"
    );
    assert!(
        changed.code().is_empty(),
        "nothing else changed: {:?}",
        changed.code()
    );
}

#[test]
fn nested_tool_settings_are_rules_and_a_web_app_is_code() {
    let repository = Repository::new();
    repository.write("crates/app/src/.ignore", "lib.rs\n");
    repository.write("crates/app/.gitignore", "/src/lib.rs\n");
    repository.write(
        "crates/app/web/tsconfig.json",
        "{ \"compilerOptions\": {} }\n",
    );
    repository.write("crates/app/web/bunfig.toml", "[install]\n");
    repository.write("crates/app/web/.npmrc", "ignore-scripts=false\n");
    repository.write(
        "crates/app/web/package.json",
        "{ \"scripts\": { \"check\": \"true\" } }\n",
    );
    repository.write("crates/app/web/src/main.ts", "export {};\n");
    repository.commit("add a web app and settings files");

    let changed = repository.changed_files();
    assert_eq!(
        changed.rules(),
        [
            "crates/app/.gitignore",
            "crates/app/src/.ignore",
            "crates/app/web/.npmrc",
            "crates/app/web/bunfig.toml",
            "crates/app/web/tsconfig.json",
        ],
        "a tool's settings file is a rule at any depth"
    );
    assert_eq!(
        changed.code(),
        ["crates/app/web/package.json", "crates/app/web/src/main.ts"],
        "the web app's manifest and sources are code"
    );
}

/// The member manifest of `app` with `build = "gen.rs"`.
const GEN_MANIFEST: &str =
    "[package]\nname = \"app\"\nbuild = \"gen.rs\"\n\n[lints]\nworkspace = true\n";

/// A repository whose `base` has the member `app` with the build script `gen.rs`, which its
/// `build` key names, and the helper module `helper.rs` the script uses.
fn repository_with_a_named_build_script() -> Repository {
    let repository = Repository::new();
    repository.write("crates/app/Cargo.toml", GEN_MANIFEST);
    repository.write("crates/app/gen.rs", "mod helper;\n\nfn main() {}\n");
    repository.write("crates/app/helper.rs", "pub fn help() {}\n");
    repository.commit("add a build script under another name");
    repository.git(&["branch", "--force", "base"]);
    repository
}

#[test]
fn a_build_script_the_build_key_names_is_a_rule() {
    let repository = repository_with_a_named_build_script();
    repository.write(
        "crates/app/gen.rs",
        "mod helper;\n\nfn main() {\n    println!(\"cargo::rustc-cfg=hidden\");\n}\n",
    );
    repository.write(
        "crates/app/src/lib.rs",
        "//! The app.\n\n#[cfg(hidden)]\npub fn unchecked() {}\n",
    );
    repository.commit("hide code behind a cfg the build script sets");

    let changed = repository.changed_files();
    assert_eq!(
        changed.rules(),
        ["crates/app/gen.rs (the build script its crate's build key names)"],
        "the file the build key names is the crate's build script"
    );
    assert!(
        changed.mixes_rules_and_code(),
        "a build script change beside code fails"
    );
}

#[test]
fn a_build_script_named_at_either_end_is_a_rule() {
    let repository = repository_with_a_named_build_script();
    repository.write(
        "crates/app/Cargo.toml",
        &GEN_MANIFEST.replace("build = \"gen.rs\"", "build = \"./scripts/../make.rs\""),
    );
    repository.git(&["mv", "crates/app/gen.rs", "crates/app/make.rs"]);
    repository.commit("rename the build script");

    let changed = repository.changed_files();
    assert_eq!(
        changed.rules(),
        [
            "crates/app/Cargo.toml ([build-dependencies], the build key and the license key)",
            "crates/app/gen.rs (the build script its crate's build key names)",
            "crates/app/make.rs (the build script its crate's build key names)",
        ],
        "the script the base names and the one the head names are both rules"
    );
    assert!(
        changed.code().is_empty(),
        "nothing else changed: {:?}",
        changed.code()
    );

    repository.write("crates/app/make.rs", "fn main() {}\n");
    let changed = repository.changed_files_to(&Head::WorkingTree);
    assert!(
        changed.rules().contains(
            &"crates/app/make.rs (the build script its crate's build key names)".to_owned()
        ),
        "the working tree's manifest names the script too: {:?}",
        changed.rules()
    );
}

#[test]
fn a_build_script_module_stays_code() {
    let repository = repository_with_a_named_build_script();
    repository.write(
        "crates/app/helper.rs",
        "pub fn help() {}\npub fn more() {}\n",
    );
    repository.commit("change the build script's module");

    let changed = repository.changed_files();
    assert!(
        changed.rules().is_empty(),
        "only the file the build key names is a rule, not the modules it reads: {:?}",
        changed.rules()
    );
    assert_eq!(
        changed.code(),
        ["crates/app/helper.rs"],
        "the module is code"
    );
}

/// What the listing adds to a nested repository.
const NESTED_REPOSITORY_NOTE: &str = "(another git repository, whose files no check sees)";

/// A repository whose `base` holds a submodule at `path`, a package with a manifest and a
/// library.
fn repository_with_a_submodule(path: &str) -> Repository {
    let repository = Repository::new();
    repository.nested_repository(path);
    repository.commit("add a submodule");
    repository.git(&["branch", "--force", "base"]);
    repository
}

/// Commits, in the submodule at `path` of `repository`, a build script and the `build` key that
/// names it, which the superproject sees only as a new commit of the submodule.
fn add_a_build_script_in_the_submodule(repository: &Repository, path: &str) {
    repository.write(&format!("{path}/gen.rs"), LOOSE_BUILD_SCRIPT);
    repository.write(
        &format!("{path}/Cargo.toml"),
        "[package]\nname = \"sub\"\nbuild = \"gen.rs\"\n",
    );
    let nested = repository.path().join(path);
    git(&nested, &["add", "--all"]);
    git(
        &nested,
        &["commit", "--quiet", "--no-verify", "--message", "build"],
    );
}

/// `path` as the file system of `repository` stores it. Where the file system ignores case,
/// `CRATES/sub` is the directory `crates/sub`, since `crates` was there first, and git reports
/// that spelling.
fn stored_spelling(repository: &Repository, path: &str) -> String {
    let ignores_case = fs::exists(repository.path().join("CRATES/app"))
        .expect("the file system should say whether the crate is there");
    if ignores_case && path.starts_with("CRATES/") {
        path.to_ascii_lowercase()
    } else {
        path.to_owned()
    }
}

#[test]
fn moving_a_submodule_is_a_rule_change() {
    for path in [
        "crates/sub",
        "crates/app/vendor/sub",
        "tools/sub",
        "CRATES/sub",
    ] {
        let repository = repository_with_a_submodule(path);
        add_a_build_script_in_the_submodule(&repository, path);
        repository.write("crates/app/src/lib.rs", "//! The app, changed.\n");
        let stored = stored_spelling(&repository, path);
        let expected = [format!("{stored} {NESTED_REPOSITORY_NOTE}")];

        let changed = repository.changed_files_to(&Head::WorkingTree);
        assert_eq!(
            changed.rules(),
            expected,
            "the working tree moves the submodule at {path} to a commit no check has seen"
        );
        assert_eq!(changed.code(), ["crates/app/src/lib.rs"], "{path}");

        repository.commit("move the submodule and change the code");
        let changed = repository.changed_files();
        assert_eq!(
            changed.rules(),
            expected,
            "the commit moves the submodule at {path} to a commit no check has seen"
        );
        assert!(changed.mixes_rules_and_code(), "beside code, it fails");
    }
}

#[test]
fn a_submodule_git_is_told_to_ignore_still_counts() {
    let repository = Repository::new();
    repository.nested_repository("tools/sub");
    repository.write(
        ".gitmodules",
        "[submodule \"sub\"]\n\tpath = tools/sub\n\turl = ./tools/sub\n\tignore = all\n",
    );
    repository.commit("add a submodule that diffs leave out");
    repository.git(&["branch", "--force", "base"]);
    add_a_build_script_in_the_submodule(&repository, "tools/sub");
    repository.write("crates/app/src/lib.rs", "//! The app, changed.\n");
    let expected = [format!("tools/sub {NESTED_REPOSITORY_NOTE}")];
    assert_eq!(
        repository.changed_files_to(&Head::WorkingTree).rules(),
        expected,
        "`ignore = all` in .gitmodules hides the move from a plain `git diff`"
    );
    repository.git(&["add", "--force", "tools/sub", "crates"]);
    repository.git(&["commit", "--quiet", "--no-verify", "--message", "move it"]);
    let changed = repository.changed_files();
    assert_eq!(changed.rules(), expected, "and from a diff of two commits");
    assert!(changed.mixes_rules_and_code(), "beside code, it fails");
}

#[test]
fn adding_or_removing_a_submodule_is_a_rule_change() {
    let repository = Repository::new();
    repository.nested_repository("crates/sub");
    assert_eq!(
        repository.changed_files_to(&Head::WorkingTree).rules(),
        [format!("crates/sub {NESTED_REPOSITORY_NOTE}")],
        "an untracked repository in the working tree is one too"
    );
    repository.commit("add a submodule");
    assert_eq!(
        repository.changed_files().rules(),
        [format!("crates/sub {NESTED_REPOSITORY_NOTE}")],
        "adding a submodule is a rule change"
    );
    repository.git(&["branch", "--force", "base"]);
    repository.git(&["rm", "--quiet", "-r", "--cached", "crates/sub"]);
    repository.git(&["commit", "--quiet", "--no-verify", "--message", "remove"]);
    assert_eq!(
        repository.changed_files().rules(),
        [format!("crates/sub {NESTED_REPOSITORY_NOTE}")],
        "removing a submodule is a rule change"
    );
}

/// The manifest of the crate `engine` that the tests moving a crate give it, with a build script.
const ENGINE_MANIFEST: &str = "[package]\nname = \"engine\"\n\n[lints]\nworkspace = true\n";

/// The rule files `crates/engine` holds in the tests that move a crate, beside its code: a build
/// script, a web UI's settings, a nested ignore file and a recipe file.
const ENGINE_RULES: [(&str, &str); 6] = [
    ("build.rs", LOOSE_BUILD_SCRIPT),
    ("web/tsconfig.json", "{ \"include\": [\"src\"] }\n"),
    ("web/bunfig.toml", "[install]\nexact = true\n"),
    ("web/.npmrc", "save-exact=true\n"),
    (".gitignore", "/generated/\n"),
    ("justfile", "check:\n    cargo check\n"),
];

/// A repository whose `base` holds the crate `crates/engine`, with the rule files of
/// [`ENGINE_RULES`] and code, and the crate `crates/app`, which depends on it by path.
fn repository_with_an_engine() -> Repository {
    let repository = Repository::new();
    repository.write("crates/engine/Cargo.toml", ENGINE_MANIFEST);
    repository.write("crates/engine/src/lib.rs", "//! The engine.\n");
    repository.write("crates/engine/web/src/main.ts", "export {};\n");
    for (path, contents) in ENGINE_RULES {
        repository.write(&format!("crates/engine/{path}"), contents);
    }
    repository.write(
        "crates/app/Cargo.toml",
        &format!("{MEMBER_MANIFEST}\n[dependencies]\nengine = {{ path = \"../engine\" }}\n"),
    );
    repository.commit("add the engine");
    repository.git(&["branch", "--force", "base"]);
    repository
}

/// A change a test makes to a repository.
struct Change {
    /// What it is.
    why: &'static str,
    /// Makes it.
    make: fn(&Repository),
}

/// Moves `crates/engine` to `crates/<to>` and points the app's dependency at it, uncommitted.
fn move_the_engine(repository: &Repository, to: &str) {
    move_the_engine_from(repository, "engine", to);
}

/// Moves the engine from `crates/<from>` to `crates/<to>` and points the app's dependency at it,
/// uncommitted.
fn move_the_engine_from(repository: &Repository, from: &str, to: &str) {
    repository.git(&["mv", &format!("crates/{from}"), &format!("crates/{to}")]);
    repository.write(
        "crates/app/Cargo.toml",
        &format!("{MEMBER_MANIFEST}\n[dependencies]\nengine = {{ path = \"../{to}\" }}\n"),
    );
}

/// Copies every file of the engine at `crates/<from>` to `crates/<to>`.
fn copy_the_engine(repository: &Repository, from: &str, to: &str) {
    let code = ["Cargo.toml", "src/lib.rs", "web/src/main.ts"];
    for path in ENGINE_RULES.map(|(path, _)| path).iter().chain(&code) {
        let to = repository.path().join("crates").join(to).join(path);
        fs::create_dir_all(to.parent().expect("a file has a directory"))
            .expect("the directory should be created");
        fs::copy(repository.path().join("crates").join(from).join(path), to)
            .expect("the file should be copied");
    }
}

/// Asserts that `changed` lists the rule files of the engine at `crates/<to>` among its rules,
/// so a move that changed something besides the directory's name is judged as before.
fn assert_the_engine_rules_count(changed: &ChangedFiles, to: &str, why: &str) {
    let build_script = format!("crates/{to}/build.rs");
    assert!(
        changed.rules().iter().any(|rule| rule == &build_script),
        "{why}: the moved build script is a rule again: {:?}",
        changed.rules()
    );
    assert!(
        changed.mixes_rules_and_code(),
        "{why}: beside code, it fails"
    );
}

#[test]
fn a_crate_directory_that_moves_whole_is_neither_rule_nor_code() {
    let repository = repository_with_an_engine();
    move_the_engine(&repository, "motor");
    repository.commit("rename the engine's directory");

    let changed = repository.changed_files();
    assert!(
        changed.rules().is_empty(),
        "the moved crate's rule files are the same files: {:?}",
        changed.rules()
    );
    assert_eq!(
        changed.code(),
        ["crates/app/Cargo.toml"],
        "the dependency's new path is code"
    );
    let changed = repository.changed_files_to(&Head::WorkingTree);
    assert!(
        changed.rules().is_empty(),
        "a clean working tree after the commit agrees: {:?}",
        changed.rules()
    );
}

/// The rename goes through another name, since `git mv crates/engine crates/Engine` on a file
/// system that ignores case finds the destination there already and moves the crate into it.
#[test]
fn a_crate_directory_renamed_only_in_letter_case_moves_whole() {
    let repository = repository_with_an_engine();
    repository.git(&["mv", "crates/engine", "crates/engine-on-its-way"]);
    move_the_engine_from(&repository, "engine-on-its-way", "Engine");
    repository.commit("rename the engine's directory");

    assert!(
        repository.changed_files().rules().is_empty(),
        "the same crate in a directory a case-insensitive file system already called that: {:?}",
        repository.changed_files().rules()
    );
}

#[test]
fn a_move_that_changes_anything_in_the_crate_is_judged_as_before() {
    let edits: [Change; 6] = [
        Change {
            why: "a byte in a rule file",
            make: |repository| {
                repository.write("crates/motor/web/tsconfig.json", "{ \"include\": [] }\n");
                repository.git(&["add", "--all"]);
            },
        },
        Change {
            why: "a byte in a code file",
            make: |repository| {
                repository.write("crates/motor/src/lib.rs", "//! The motor.\n");
                repository.git(&["add", "--all"]);
            },
        },
        Change {
            why: "an added file",
            make: |repository| {
                repository.write("crates/motor/src/extra.rs", "//! More.\n");
                repository.git(&["add", "--all"]);
            },
        },
        Change {
            why: "a removed file",
            make: |repository| {
                repository.git(&["rm", "--quiet", "--force", "crates/motor/web/src/main.ts"]);
            },
        },
        Change {
            why: "a file renamed inside the crate",
            make: |repository| {
                repository.git(&["mv", "crates/motor/src/lib.rs", "crates/motor/src/core.rs"]);
            },
        },
        Change {
            why: "a file made executable",
            make: |repository| {
                repository.git(&["update-index", "--chmod=+x", "crates/motor/src/lib.rs"]);
            },
        },
    ];
    for Change { why, make } in edits {
        let repository = repository_with_an_engine();
        move_the_engine(&repository, "motor");
        repository.git(&["add", "--all"]);
        make(&repository);
        repository.git(&["commit", "--quiet", "--no-verify", "--message", why]);
        assert_the_engine_rules_count(&repository.changed_files(), "motor", why);
    }
}

#[test]
fn a_rule_edit_in_a_second_commit_after_the_move_counts() {
    let repository = repository_with_an_engine();
    move_the_engine(&repository, "motor");
    repository.commit("rename the engine's directory");
    repository.write("crates/motor/.gitignore", "/generated/\n/src/\n");
    repository.commit("ignore the sources");

    assert_the_engine_rules_count(&repository.changed_files(), "motor", "the ends differ");
}

#[test]
fn a_move_whose_files_check_out_differently_counts() {
    let repository = repository_with_an_engine();
    repository.write(".gitattributes", "crates/motor/** ident\n");
    repository.commit("expand $Id$ in a directory that doesn't exist yet");
    repository.git(&["branch", "--force", "base"]);
    move_the_engine(&repository, "motor");
    repository.commit("rename the engine's directory");

    assert_the_engine_rules_count(
        &repository.changed_files(),
        "motor",
        "the same blobs check out as other bytes at the new place",
    );
}

#[test]
fn a_move_into_a_directory_another_crate_held_counts() {
    let repository = repository_with_an_engine();
    repository.write(
        "crates/motor/Cargo.toml",
        "[package]\nname = \"motor\"\n\n[lints]\nworkspace = true\n",
    );
    repository.write("crates/motor/src/lib.rs", "//! The motor.\n");
    repository.commit("add the motor");
    repository.git(&["branch", "--force", "base"]);
    repository.git(&["rm", "--quiet", "-r", "crates/motor"]);
    move_the_engine(&repository, "motor");
    repository.commit("replace the motor with the engine");

    assert_the_engine_rules_count(&repository.changed_files(), "motor", "the place was taken");
}

#[test]
fn a_crate_copied_or_swapped_counts() {
    let repository = repository_with_an_engine();
    repository.git(&["mv", "crates/engine", "crates/motor"]);
    repository.git(&["mv", "crates/app", "crates/engine"]);
    repository.git(&["mv", "crates/motor", "crates/app"]);
    repository.commit("swap the crates");
    let changed = repository.changed_files();
    assert!(
        changed
            .rules()
            .iter()
            .any(|rule| rule == "crates/app/build.rs"),
        "a swap keeps both directories, so it isn't a move: {:?}",
        changed.rules()
    );

    let repository = repository_with_an_engine();
    fs::create_dir_all(repository.path().join("crates/motor/src"))
        .expect("the directory should be created");
    for path in ["Cargo.toml", "src/lib.rs", "build.rs"] {
        fs::copy(
            repository.path().join("crates/engine").join(path),
            repository.path().join("crates/motor").join(path),
        )
        .expect("the file should be copied");
    }
    repository.commit("copy part of the engine");
    let changed = repository.changed_files();
    assert!(
        changed
            .rules()
            .iter()
            .any(|rule| rule == "crates/motor/build.rs"),
        "a copy keeps the old directory: {:?}",
        changed.rules()
    );
}

/// The commits are built in git's index, so the test runs on a file system that ignores case too.
#[test]
fn a_move_beside_a_directory_that_differs_only_in_case_counts() {
    let repository = repository_with_an_engine();
    repository.stage("100644", "crates/MOTOR/src/lib.rs", "//! Another crate.\n");
    repository.stage(
        "100644",
        "crates/MOTOR/Cargo.toml",
        "[package]\nname = \"other\"\n\n[lints]\nworkspace = true\n",
    );
    repository.move_in_index("engine", "motor");
    repository.commit_index("move the engine beside a directory a Mac merges with it");
    assert_the_engine_rules_count(&repository.changed_files(), "motor", "MOTOR at the head");

    let repository = repository_with_an_engine();
    repository.stage("100644", "crates/MOTOR/src/lib.rs", "//! Another crate.\n");
    repository.commit_index("add a directory a Mac reads as motor");
    repository.git(&["branch", "--force", "base"]);
    repository.git(
        &[
            &INDEX_AS_WRITTEN[..],
            &[
                "update-index",
                "--force-remove",
                "--",
                "crates/MOTOR/src/lib.rs",
            ],
        ]
        .concat(),
    );
    repository.move_in_index("engine", "motor");
    repository.commit_index("move the engine where MOTOR was");
    assert_the_engine_rules_count(&repository.changed_files(), "motor", "MOTOR at the base");
}

/// The commit is built in git's index, so the test runs on Windows too.
#[test]
fn a_move_to_a_name_windows_reads_as_another_counts() {
    for name in ["motor.", "motor~1"] {
        let repository = repository_with_an_engine();
        repository.move_in_index("engine", name);
        repository.commit_index("move the engine to a name Windows reads as another");
        let changed = repository.changed_files();
        assert!(
            changed
                .rules()
                .iter()
                .any(|rule| rule == "crates/engine/build.rs"),
            "the move to {name:?} isn't one: {:?}",
            changed.rules()
        );
    }
}

/// The link is a commit's entry, built in git's index, so the test runs where the file system
/// can't hold a link, or the user may not make one, as on Windows.
#[test]
fn a_crate_holding_a_symbolic_link_counts() {
    let repository = repository_with_an_engine();
    repository.stage("120000", "crates/engine/link", "../app/Cargo.toml");
    repository.commit_index("link to a sibling");
    repository.git(&["branch", "--force", "base"]);
    repository.move_in_index("engine", "motor");
    repository.commit_index("rename the engine's directory");
    assert_the_engine_rules_count(
        &repository.changed_files(),
        "motor",
        "a relative link can point elsewhere from the new place",
    );
}

#[test]
fn an_uncommitted_change_in_the_working_tree_stops_the_move() {
    let uncommitted: [Change; 13] = [
        Change {
            why: "an untracked file named as the old directory",
            make: |repository| {
                repository.write("crates/engine", "x\n");
            },
        },
        Change {
            why: "a staged file named as the old directory in other letters",
            make: |repository| {
                repository.stage("100644", "crates/ENGINE", "x\n");
            },
        },
        Change {
            why: "an unstaged edit",
            make: |repository| {
                repository.write("crates/motor/web/tsconfig.json", "{ \"include\": [] }\n");
            },
        },
        Change {
            why: "a staged edit",
            make: |repository| {
                repository.write("crates/motor/justfile", "check:\n");
                repository.git(&["add", "crates/motor/justfile"]);
            },
        },
        Change {
            why: "a staged edit that the working tree undoes",
            make: |repository| {
                repository.write("crates/motor/build.rs", "fn main() {}\n");
                repository.git(&["add", "crates/motor/build.rs"]);
                repository.write("crates/motor/build.rs", LOOSE_BUILD_SCRIPT);
            },
        },
        Change {
            why: "an untracked file",
            make: |repository| {
                repository.write("crates/motor/web/.npmrc.local", "x\n");
            },
        },
        Change {
            why: "an untracked file where the crate was",
            make: |repository| {
                repository.write("crates/engine/.gitignore", "/src/\n");
            },
        },
        Change {
            why: "a staged file below the new directory in other letters",
            make: |repository| {
                repository.stage("100644", "crates/MOTOR/src/lib.rs", "pub fn x() {}\n");
            },
        },
        Change {
            why: "an untracked file below the new directory in other letters",
            make: |repository| {
                repository.write("crates/MOTOR/src/lib.rs", "pub fn x() {}\n");
            },
        },
        Change {
            why: "an untracked file below the old directory in other letters",
            make: |repository| {
                repository.write("crates/Engine/src/lib.rs", "pub fn x() {}\n");
            },
        },
        Change {
            why: "an uncommitted attributes file",
            make: |repository| {
                repository.write(".gitattributes", "crates/motor/** ident\n");
            },
        },
        Change {
            why: "an uncommitted attributes file in other letters",
            make: |repository| {
                repository.write(".GitAttributes", "crates/motor/** ident\n");
            },
        },
        Change {
            why: "a staged attributes file that the working tree lacks",
            make: |repository| {
                repository.write(".gitattributes", "crates/motor/** ident\n");
                repository.git(&["add", ".gitattributes"]);
                fs::remove_file(repository.path().join(".gitattributes"))
                    .expect("the attributes file should be deleted");
            },
        },
    ];
    let mut missed = Vec::new();
    for Change { why, make } in uncommitted {
        let repository = repository_with_an_engine();
        move_the_engine(&repository, "motor");
        repository.commit("rename the engine's directory");
        make(&repository);
        let changed = repository.changed_files_to(&Head::WorkingTree);
        if !changed
            .rules()
            .iter()
            .any(|rule| rule == "crates/motor/build.rs")
        {
            missed.push(format!("{why}: {:?}", changed.rules()));
        }
    }
    assert!(
        missed.is_empty(),
        "the working tree judges the move as before, but these let it through: {missed:#?}"
    );
}

#[test]
fn two_identical_directories_that_become_one_count() {
    let repository = repository_with_an_engine();
    repository.git(&["rm", "--quiet", "-r", "crates/app"]);
    copy_the_engine(&repository, "engine", "spare");
    repository.commit("hold the engine twice");
    repository.git(&["branch", "--force", "base"]);
    repository.git(&["rm", "--quiet", "-r", "crates/spare"]);
    repository.git(&["mv", "crates/engine", "crates/motor"]);
    repository.commit("keep one, moved");
    let changed = repository.changed_files();
    assert!(
        changed
            .rules()
            .iter()
            .any(|rule| rule == "crates/spare/.gitignore"),
        "deleting the second copy's rule files still counts: {:?}",
        changed.rules()
    );
}

#[test]
fn a_directory_that_holds_no_package_counts() {
    let repository = Repository::new();
    repository.write("crates/assets/justfile", "check:\n    true\n");
    repository.write("crates/assets/logo.txt", "logo\n");
    repository.commit("add assets");
    repository.git(&["branch", "--force", "base"]);
    repository.git(&["mv", "crates/assets", "crates/media"]);
    repository.write("crates/app/src/lib.rs", "//! The app, with media.\n");
    repository.commit("move the assets");
    let changed = repository.changed_files();
    assert!(
        changed
            .rules()
            .iter()
            .any(|rule| rule == "crates/media/justfile"),
        "only a package's directory moves whole: {:?}",
        changed.rules()
    );
}

#[test]
fn a_submodule_that_moves_in_crates_still_counts() {
    let repository = repository_with_a_submodule("crates/sub");
    repository.git(&["mv", "crates/sub", "crates/moved"]);
    repository.write("crates/app/src/lib.rs", "//! The app, changed.\n");
    repository.commit("move the submodule");
    let changed = repository.changed_files();
    assert!(
        changed
            .rules()
            .contains(&format!("crates/moved {NESTED_REPOSITORY_NOTE}")),
        "a submodule isn't a directory of this repository: {:?}",
        changed.rules()
    );
}

#[test]
fn a_move_whose_files_check_out_differently_where_case_is_ignored_counts() {
    for (path, contents) in [
        (".gitattributes", "CRATES/MOTOR/** ident\n"),
        (".GitAttributes", "crates/motor/** ident\n"),
    ] {
        let repository = repository_with_an_engine();
        repository.write(path, contents);
        repository.commit("expand $Id$ where macOS and Windows match names in any case");
        repository.git(&["branch", "--force", "base"]);
        move_the_engine(&repository, "motor");
        repository.commit("rename the engine's directory");

        assert_the_engine_rules_count(
            &repository.changed_files(),
            "motor",
            &format!("{path} with {contents:?} applies on a case-insensitive checkout"),
        );
    }
}

#[test]
fn a_move_whose_files_check_out_differently_where_case_counts() {
    let repository = repository_with_an_engine();
    repository.write(
        ".gitattributes",
        "crates/motor/** ident\nCRATES/MOTOR/** !ident\n",
    );
    repository.commit("expand $Id$ only where git matches names exactly");
    repository.git(&["branch", "--force", "base"]);
    move_the_engine(&repository, "motor");
    repository.commit("rename the engine's directory");

    assert_the_engine_rules_count(
        &repository.changed_files(),
        "motor",
        "a checkout that matches names exactly, as on Linux, expands $Id$ at the new place",
    );
}

#[test]
fn a_misspelled_attributes_file_at_either_end_stops_the_move() {
    let repository = repository_with_an_engine();
    repository.write("crates/.GitAttributes", "engine/** ident\n");
    repository.commit("expand $Id$ in the engine where case is ignored");
    repository.git(&["branch", "--force", "base"]);
    repository.git(&["rm", "--quiet", "crates/.GitAttributes"]);
    move_the_engine(&repository, "motor");
    repository.commit("rename the engine's directory and stop expanding");
    assert_the_engine_rules_count(
        &repository.changed_files(),
        "motor",
        "the merge base held the misspelled file",
    );

    let repository = repository_with_an_engine();
    move_the_engine(&repository, "motor");
    repository.write("crates/.GitAttributes", "motor/** ident\n");
    repository.commit("rename the engine's directory and expand $Id$ where case is ignored");
    assert_the_engine_rules_count(
        &repository.changed_files(),
        "motor",
        "the head holds the misspelled file",
    );
}

/// The commits are built in git's index, so the test runs on Windows too.
#[test]
fn a_move_from_a_name_windows_reads_as_another_counts() {
    for name in ["engine.", "engine~1"] {
        let repository = repository_with_an_engine();
        repository.move_in_index("engine", name);
        repository.commit_index("move the engine to a name Windows reads as another");
        repository.git(&["branch", "--force", "base"]);
        repository.move_in_index(name, "motor");
        repository.commit_index("move the engine out of it");
        assert_the_engine_rules_count(
            &repository.changed_files(),
            "motor",
            "the merge base's directory isn't one name everywhere",
        );
    }
}

#[test]
fn a_crate_copied_to_two_places_counts() {
    let repository = repository_with_an_engine();
    repository.git(&["mv", "crates/engine", "crates/motor"]);
    copy_the_engine(&repository, "motor", "spare");
    repository.commit("move the engine and copy it");
    assert_the_engine_rules_count(
        &repository.changed_files(),
        "motor",
        "two places hold the tree at the head, so neither is the engine's",
    );
}

/// An old git is faked by a shell script named `git` early on `PATH`, which Windows can't run in
/// place of `git.exe`, so the test runs on Linux and macOS only.
#[cfg(unix)]
#[test]
fn a_git_that_cant_read_attributes_from_a_revision_moves_nothing() {
    use std::fs::Permissions;
    use std::os::unix::fs::PermissionsExt as _;

    let repository = repository_with_an_engine();
    move_the_engine(&repository, "motor");
    repository.commit("rename the engine's directory");
    let tools = TempDir::new().expect("the temporary directory should be created");
    let old_git = tools.path().join("git");
    fs::write(
        &old_git,
        "#!/bin/sh\nfor argument in \"$@\"; do\n  case \"$argument\" in\n    --source=*) \
         echo \"error: unknown option \\`source'\" >&2; exit 129 ;;\n  esac\ndone\n\
         exec \"$REAL_GIT\" \"$@\"\n",
    )
    .expect("the script should be written");
    fs::set_permissions(&old_git, Permissions::from_mode(0o755))
        .expect("the script should be made executable");
    let real_git = Command::new("sh")
        .args(["-c", "command -v git"])
        .output()
        .expect("sh should run");
    let path = env::var_os("PATH").expect("the tests run with a PATH");
    let mut paths = vec![tools.path().to_owned()];
    paths.extend(env::split_paths(&path));
    let output = Command::new(env!("CARGO_BIN_EXE_trellis-policy"))
        .args(["rule-changes", "base", "--head", "HEAD"])
        .current_dir(repository.path())
        .env(
            "PATH",
            env::join_paths(paths).expect("the paths should join"),
        )
        .env("REAL_GIT", String::from_utf8_lossy(&real_git.stdout).trim())
        .output()
        .expect("trellis-policy should run");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        output.status.code(),
        Some(1),
        "git before 2.40 can't compare attributes, so the move is judged as before: {stdout}{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        stdout.contains("crates/motor/build.rs"),
        "the moved build script is a rule again: {stdout}"
    );
}

/// A deploy kit ships with the code it deploys, so a change to its scripts and units lands with a
/// code change. A file that is a rule by its name stays a rule there.
#[test]
fn a_deploy_kit_change_is_code() {
    let repository = Repository::new();
    repository.write("deploy/install.sh", "#!/bin/sh\necho install\n");
    repository.write("deploy/app.container", "[Container]\nImage=app\n");
    repository.commit("add the deploy kit");
    repository.git(&["branch", "--force", "base"]);
    repository.write("deploy/install.sh", "#!/bin/sh\necho install v2\n");
    repository.write("deploy/backup.sh", "#!/bin/sh\necho backup\n");
    repository.write("crates/app/src/lib.rs", "//! The app, with backups.\n");
    repository.commit("back up the data");

    let changed = repository.changed_files();
    assert!(
        changed.rules().is_empty(),
        "a kit change isn't a rule change: {:?}",
        changed.rules()
    );
    assert_eq!(changed.code().len(), 3, "{:?}", changed.code());
    assert!(
        !changed.mixes_rules_and_code(),
        "a kit change with code passes"
    );

    repository.write("deploy/.gitignore", "*.bak\n");
    repository.commit("ignore the kit's backups");
    let changed = repository.changed_files();
    assert_eq!(changed.rules(), ["deploy/.gitignore".to_owned()]);
    assert!(
        changed.mixes_rules_and_code(),
        "a rule file in the kit is still a rule"
    );
}

/// A crate's deploy kit is code, as the root kit is, with the files that are rules by their names
/// in a kit still rules.
#[test]
fn a_crate_kit_change_is_code() {
    let repository = Repository::new();
    repository.write("crates/app/deploy/install.sh", "#!/bin/sh\necho install\n");
    repository.write(
        "crates/app/deploy/app.service",
        "[Service]\nExecStart=/srv/app\n",
    );
    repository.commit("add the deploy kit");

    let changed = repository.changed_files();
    assert!(
        changed.rules().is_empty(),
        "adding the kit isn't a rule change: {:?}",
        changed.rules()
    );
    assert_eq!(changed.code().len(), 2, "{:?}", changed.code());

    repository.git(&["branch", "--force", "base"]);
    repository.write(
        "crates/app/deploy/install.sh",
        "#!/bin/sh\necho install v2\n",
    );
    repository.write("crates/app/src/lib.rs", "//! The app, installed.\n");
    repository.commit("install v2");
    let changed = repository.changed_files();
    assert!(changed.rules().is_empty(), "{:?}", changed.rules());
    assert!(!changed.mixes_rules_and_code());

    for rule in [
        "crates/app/deploy/action.yml",
        "crates/app/deploy/tasks.just",
    ] {
        repository.git(&["branch", "--force", "base"]);
        repository.write(rule, "x\n");
        repository.write(
            "crates/app/deploy/install.sh",
            &format!("#!/bin/sh\necho {rule}\n"),
        );
        repository.commit("a rule beside the kit");
        let changed = repository.changed_files();
        assert_eq!(changed.rules(), [rule.to_owned()]);
        assert!(changed.mixes_rules_and_code(), "{rule} stays a rule");
    }
}

/// The files of the root deploy kit in the tests that move it into a crate: a script and a unit,
/// which are code, and a composite action and a justfile module, which are rules by their names.
const KIT_FILES: [(&str, &str); 4] = [
    ("install.sh", "#!/bin/sh\necho install\n"),
    ("app.service", "[Service]\nExecStart=/srv/app\n"),
    ("action.yml", "runs:\n  using: composite\n  steps: []\n"),
    ("tasks.just", "install:\n    sh deploy/install.sh\n"),
];

/// The kit's rule files at both of its places, as a move lists them.
const MOVED_KIT_RULES: [&str; 4] = [
    "crates/app/deploy/action.yml",
    "crates/app/deploy/tasks.just",
    "deploy/action.yml",
    "deploy/tasks.just",
];

/// A workflow that runs the kit's composite action from `directory`.
fn deploy_workflow(directory: &str) -> String {
    format!(
        "on: push\njobs:\n  deploy:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: ./{directory}\n"
    )
}

/// A repository whose `base` holds the root deploy kit of [`KIT_FILES`], a workflow that runs its
/// action and a justfile that imports its module.
fn repository_with_a_kit() -> Repository {
    let repository = Repository::new();
    for (path, contents) in KIT_FILES {
        repository.write(&format!("deploy/{path}"), contents);
    }
    repository.write(".github/workflows/deploy.yml", &deploy_workflow("deploy"));
    repository.write("justfile", "import 'deploy/tasks.just'\n");
    repository.commit("add the deploy kit");
    repository.git(&["branch", "--force", "base"]);
    repository
}

/// Moves the root kit to `crates/app/deploy` in git's index and the working tree.
fn move_the_kit(repository: &Repository) {
    repository.git(&["mv", "deploy", "crates/app/deploy"]);
}

/// Whether `changed` lists the kit's script at `crates/app/deploy` among its code, so a move that
/// changed the kit's code, or one a guard refuses, is judged as before.
fn the_kit_counts(changed: &ChangedFiles) -> bool {
    changed
        .code()
        .iter()
        .any(|code| code == "crates/app/deploy/install.sh")
}

/// Asserts that `missed`, the cases whose kit didn't count, is empty.
fn assert_no_kit_move_missed(missed: &[String]) {
    assert!(
        missed.is_empty(),
        "the move is judged as before, but these let it through: {missed:#?}"
    );
}

#[test]
fn a_root_kit_moved_whole_into_a_crate_leaves_out_its_code() {
    let repository = repository_with_a_kit();
    move_the_kit(&repository);
    repository.commit("move the kit into the crate");

    let changed = repository.changed_files();
    assert!(
        changed.code().is_empty(),
        "the moved kit's code is the same code: {:?}",
        changed.code()
    );
    assert_eq!(
        changed.rules(),
        MOVED_KIT_RULES,
        "the kit's rule files are judged at both paths"
    );
    assert!(!changed.mixes_rules_and_code());
    let changed = repository.changed_files_to(&Head::WorkingTree);
    assert!(
        changed.code().is_empty(),
        "a clean working tree after the commit agrees: {changed:?}"
    );
}

#[test]
fn a_kit_move_with_rule_file_changes_is_a_rules_change() {
    let edits: [Change; 3] = [
        Change {
            why: "the references to the kit's path and the module's path to its script",
            make: |repository| {
                repository.write(
                    ".github/workflows/deploy.yml",
                    &deploy_workflow("crates/app/deploy"),
                );
                repository.write("justfile", "import 'crates/app/deploy/tasks.just'\n");
                repository.write(
                    "crates/app/deploy/tasks.just",
                    "install:\n    sh crates/app/deploy/install.sh\n",
                );
            },
        },
        Change {
            why: "the action removed",
            make: |repository| {
                fs::remove_file(repository.path().join("crates/app/deploy/action.yml"))
                    .expect("the action should be deleted");
            },
        },
        Change {
            why: "a module added",
            make: |repository| {
                repository.write("crates/app/deploy/backup.just", "backup:\n");
            },
        },
    ];
    for Change { why, make } in edits {
        let repository = repository_with_a_kit();
        move_the_kit(&repository);
        make(&repository);
        repository.commit(why);
        let changed = repository.changed_files();
        assert!(
            changed.code().is_empty(),
            "{why}: the kit's code moved whole: {:?}",
            changed.code()
        );
        assert!(
            !changed.mixes_rules_and_code(),
            "{why}: the move is a rules change: {:?}",
            changed.rules()
        );
    }
}

#[test]
fn a_kit_move_that_changes_its_code_or_its_place_is_judged_as_before() {
    let edits: [Change; 10] = [
        Change {
            why: "a script edited",
            make: |repository| {
                repository.write("crates/app/deploy/install.sh", "#!/bin/sh\necho v2\n");
                repository.git(&["add", "--all"]);
            },
        },
        Change {
            why: "a code file added",
            make: |repository| {
                repository.write("crates/app/deploy/backup.sh", "#!/bin/sh\n");
                repository.git(&["add", "--all"]);
            },
        },
        Change {
            why: "a code file removed",
            make: |repository| {
                fs::remove_file(repository.path().join("crates/app/deploy/app.service"))
                    .expect("the unit should be deleted");
                repository.git(&["add", "--all"]);
            },
        },
        Change {
            why: "a script made executable",
            make: |repository| {
                repository.git(&["update-index", "--chmod=+x", "crates/app/deploy/install.sh"]);
            },
        },
        Change {
            why: "the root kit kept as a copy",
            make: |repository| {
                for (path, contents) in KIT_FILES {
                    repository.write(&format!("deploy/{path}"), contents);
                }
                repository.git(&["add", "--all"]);
            },
        },
        Change {
            why: "a copy in a second crate",
            make: |repository| {
                for (path, contents) in KIT_FILES {
                    repository.write(&format!("crates/spare/deploy/{path}"), contents);
                }
                repository.git(&["add", "--all"]);
            },
        },
        Change {
            why: "a root entry in other letters",
            make: |repository| {
                repository.stage("100644", "Deploy/install.sh", "#!/bin/sh\n");
            },
        },
        Change {
            why: "a second kit in the crate in other letters",
            make: |repository| {
                repository.stage("100644", "crates/app/Deploy/notes.txt", "x\n");
            },
        },
        Change {
            why: "a second crate directory in other letters",
            make: |repository| {
                repository.stage("100644", "crates/App/notes.txt", "x\n");
            },
        },
        Change {
            why: "attributes that change the new place's checkout",
            make: |repository| {
                repository.write(".gitattributes", "crates/app/deploy/** ident\n");
                repository.git(&["add", "--all"]);
            },
        },
    ];
    let mut missed = Vec::new();
    for Change { why, make } in edits {
        let repository = repository_with_a_kit();
        move_the_kit(&repository);
        repository.git(&["add", "--all"]);
        make(&repository);
        repository.commit_index(why);
        let changed = repository.changed_files();
        if !the_kit_counts(&changed) {
            missed.push(format!("{why}: {changed:?}"));
        }
    }
    assert_no_kit_move_missed(&missed);
}

/// A kit that a guard keeps from moving whole at `base` already, so only the move changes, is
/// judged as before. The move is made in git's index, so a symbolic link or a submodule moves too.
#[test]
fn a_kit_that_a_guard_refuses_is_judged_as_before() {
    let setups: [Change; 9] = [
        Change {
            why: "attributes for the new place",
            make: |repository| {
                repository.write(".gitattributes", "crates/app/deploy/** ident\n");
                repository.commit("expand $Id$ in a directory that doesn't exist yet");
            },
        },
        Change {
            why: "an attributes file in other letters",
            make: |repository| {
                repository.write(".GitAttributes", "*.md text\n");
                repository.commit("an attributes file a case-insensitive system reads");
            },
        },
        Change {
            why: "a symbolic link",
            make: |repository| {
                repository.stage("120000", "deploy/current", "install.sh");
                repository.commit_index("link the current script");
            },
        },
        Change {
            why: "a submodule",
            make: |repository| {
                repository.nested_repository("deploy/vendor");
                repository.commit("vendor a kit");
            },
        },
        Change {
            why: "a manifest in other letters, which role reads as code",
            make: |repository| {
                repository.write("deploy/cargo.toml", "[package]\nname = \"kit\"\n");
                repository.commit("make the kit a package");
            },
        },
        Change {
            why: "a file the crate's build key names at the new place",
            make: |repository| {
                repository.write("deploy/gen.rs", "fn main() {}\n");
                repository.write(
                    "crates/app/Cargo.toml",
                    &MEMBER_MANIFEST
                        .replace("[package]\n", "[package]\nbuild = \"deploy/gen.rs\"\n"),
                );
                repository.commit("name a build script in the kit's new place");
            },
        },
        Change {
            why: "a file the crate's build key names at the old place",
            make: |repository| {
                repository.write("deploy/gen.rs", "fn main() {}\n");
                repository.write(
                    "crates/app/Cargo.toml",
                    &MEMBER_MANIFEST.replace(
                        "[package]\n",
                        "[package]\nbuild = \"../../deploy/gen.rs\"\n",
                    ),
                );
                repository.commit("name a build script in the kit");
            },
        },
        Change {
            why: "a root entry in other letters at the base",
            make: |repository| {
                repository.stage("100644", "Deploy/notes.txt", "x\n");
                repository.commit_index("a second kit a case-insensitive system merges");
            },
        },
        Change {
            why: "a kit entry in the crate at the base",
            make: |repository| {
                repository.stage("100644", "crates/app/Deploy/notes.txt", "x\n");
                repository.commit_index("a kit where the root kit moves");
            },
        },
    ];
    let mut missed = Vec::new();
    for Change { why, make } in setups {
        let repository = repository_with_a_kit();
        make(&repository);
        repository.git(&["branch", "--force", "base"]);
        for stale in ["Deploy/notes.txt", "crates/app/Deploy/notes.txt"] {
            repository.git(
                &[
                    &INDEX_AS_WRITTEN[..],
                    &["update-index", "--force-remove", "--", stale],
                ]
                .concat(),
            );
        }
        repository.rename_in_index("deploy", "crates/app/deploy");
        repository.commit_index("move the kit into the crate");
        let changed = repository.changed_files();
        if !the_kit_counts(&changed) {
            missed.push(format!("{why}: {changed:?}"));
        }
    }
    assert_no_kit_move_missed(&missed);
}

/// A crate directory whose name isn't ASCII can mean another directory where the file system
/// normalizes names, so the kit doesn't move whole into one.
#[test]
fn a_kit_moved_into_a_crate_with_a_name_that_isnt_plain_is_judged_as_before() {
    let repository = repository_with_a_kit();
    repository.rename_in_index("deploy", "crates/mot\u{f6}r/deploy");
    repository.commit_index("move the kit into a new crate");
    let changed = repository.changed_files();
    assert!(
        changed
            .code()
            .iter()
            .any(|code| code == "crates/mot\u{f6}r/deploy/install.sh"),
        "the moved script is code again: {:?}",
        changed.code()
    );
}

#[test]
fn an_uncommitted_kit_path_in_the_working_tree_stops_the_kit_move() {
    let uncommitted: [Change; 5] = [
        Change {
            why: "an untracked file named as the kit",
            make: |repository| repository.write("deploy", "x\n"),
        },
        Change {
            why: "a staged file named as the kit in other letters",
            make: |repository| repository.stage("100644", "DEPLOY", "x\n"),
        },
        Change {
            why: "an untracked file below the old place",
            make: |repository| repository.write("deploy/install.sh", "x\n"),
        },
        Change {
            why: "an untracked file below the new place",
            make: |repository| repository.write("crates/app/deploy/notes.txt", "x\n"),
        },
        Change {
            why: "a staged file below the new place in other letters",
            make: |repository| repository.stage("100644", "crates/app/DEPLOY/notes.txt", "x\n"),
        },
    ];
    let mut missed = Vec::new();
    for Change { why, make } in uncommitted {
        let repository = repository_with_a_kit();
        move_the_kit(&repository);
        repository.commit("move the kit into the crate");
        make(&repository);
        let changed = repository.changed_files_to(&Head::WorkingTree);
        if !the_kit_counts(&changed) {
            missed.push(format!("{why}: {changed:?}"));
        }
    }
    assert_no_kit_move_missed(&missed);
}
