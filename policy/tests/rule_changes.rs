//! `ChangedFiles` on real git histories: which branches mix rule changes with code changes.

use std::fs;
use std::path::Path;
use std::process::Command;

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
        self.git(&["commit", "--quiet", "--no-verify", "--message", message]);
    }

    /// Runs git in the repository, with a fixed identity and no signing, so the user's git
    /// configuration doesn't matter.
    fn git(&self, arguments: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(self.path())
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

    /// The files the commits on `HEAD` change since `base`.
    fn changed_files(&self) -> ChangedFiles {
        self.changed_files_to(&Head::Commit("HEAD".to_owned()))
    }

    /// The files changed from the merge base of `base` and `head` to `head`.
    fn changed_files_to(&self, head: &Head) -> ChangedFiles {
        ChangedFiles::between(self.path(), "base", head).expect("git should list the changed files")
    }
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
fn an_unknown_base_fails() {
    let repository = Repository::new();
    let error =
        ChangedFiles::between(repository.path(), "no-such-branch", &Head::WorkingTree).unwrap_err();
    assert!(
        error.to_string().contains("merge-base"),
        "the error should name the failing git command: {error}"
    );
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
            ".gitignore (untracked; commit it in a rules pull request, or ignore it in \
             .git/info/exclude)",
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

/// A repository whose `base` adds the nested package manifest at `manifest`.
fn repository_with_nested_manifest(manifest: &str) -> Repository {
    let repository = Repository::new();
    repository.write(manifest, NESTED_MANIFEST);
    repository.commit("add a package inside a crate");
    repository.git(&["branch", "--force", "base"]);
    repository
}

/// A file system that ignores case stores the build script under the spelling its directory
/// already has, so the path is compared in any case.
#[test]
fn a_committed_build_rs_whose_package_directory_differs_in_case_is_a_rule() {
    for (manifest, build_script) in CASE_SPLIT_LAYOUTS {
        let repository = repository_with_nested_manifest(manifest);
        repository.write(build_script, LOOSE_BUILD_SCRIPT);
        repository.write("crates/app/src/lib.rs", "//! The app, changed.\n");
        repository.commit("add the build script and change the code");

        let changed = repository.changed_files();
        let rules = changed.rules();
        assert!(
            matches!(rules, [rule] if rule.eq_ignore_ascii_case(build_script)),
            "a case-insensitive file system puts {build_script} beside {manifest}, but the rules \
             are {rules:?}"
        );
        assert!(changed.mixes_rules_and_code(), "beside code, it fails");
    }
}

/// A file system that ignores case stores the build script under the spelling its directory
/// already has, so the path is compared in any case.
#[test]
fn a_working_tree_build_rs_whose_package_directory_differs_in_case_is_a_rule() {
    for (manifest, build_script) in CASE_SPLIT_LAYOUTS {
        let repository = repository_with_nested_manifest(manifest);
        repository.write(build_script, LOOSE_BUILD_SCRIPT);
        repository.write("crates/app/src/lib.rs", "//! The app, changed.\n");

        let changed = repository.changed_files_to(&Head::WorkingTree);
        let rules = changed.rules();
        assert!(
            matches!(rules, [rule] if rule.eq_ignore_ascii_case(build_script)),
            "a case-insensitive file system puts {build_script} beside {manifest}, but the rules \
             are {rules:?}"
        );
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
            "crates/app/Cargo.toml ([build-dependencies] and the build key)".to_owned(),
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
