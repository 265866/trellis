//! `trellis-policy check` on a small workspace shaped like a project generated from the trellis
//! template, which passes, and on copies of it that each break one rule.

use std::fs;
#[cfg(unix)]
use std::os::unix;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

/// The root `Cargo.toml` of the project, with the `[workspace]` line `exclude` and the dylint
/// `libraries`.
fn root_manifest(exclude: &str, libraries: &str) -> String {
    format!(
        r#"[workspace]
resolver = "3"
members = ["crates/*"]
{exclude}
[workspace.package]
version = "0.1.0"
edition = "2024"
rust-version = "1.85"
publish = false

[workspace.dependencies]
demo_core = {{ path = "crates/demo_core" }}

[workspace.lints.rust]
missing_docs = "warn"
unsafe_code = "deny"
unexpected_cfgs = {{ level = "warn", check-cfg = ["cfg(dylint_lib, values(any()))"] }}

[workspace.lints.clippy]
pedantic = {{ level = "warn", priority = -1 }}
unwrap_used = "warn"

[workspace.metadata.dylint]
libraries = {libraries}

[workspace.metadata.policy.internal_dependencies]
demo = ["demo_core"]
demo_core = []

[profile.release]
overflow-checks = true
panic = "abort"
"#
    )
}

/// The dylint libraries of the passing project: the fixture lints in `lints/` beside the project,
/// outside its workspace, as a trellis checkout is.
const PATH_LIBRARY: &str = r#"[{ path = "../lints" }]"#;

/// The manifest of the member `name`, with the `[dependencies]` entries `dependencies`.
fn member_manifest(name: &str, dependencies: &str) -> String {
    format!(
        "[package]\nname = \"{name}\"\nversion.workspace = true\nedition.workspace = true\n\
         rust-version.workspace = true\npublish.workspace = true\n\n\
         [dependencies]\n{dependencies}\n[lints]\nworkspace = true\n"
    )
}

/// The tool configuration files a generated project keeps at its root, with their contents.
const CONFIG_FILES: [(&str, &str); 14] = [
    ("rustfmt.toml", "style_edition = \"2024\"\n"),
    ("clippy.toml", "too-many-arguments-threshold = 5\n"),
    ("deny.toml", "[advisories]\n"),
    ("_typos.toml", "[default]\n"),
    (".taplo.toml", "[formatting]\n"),
    ("dylint.toml", "[trellis_lints]\n"),
    (
        "rust-toolchain.toml",
        "[toolchain]\nchannel = \"1.100.0\"\n",
    ),
    (".config/nextest.toml", "[profile.default]\n"),
    (
        "release-plz.toml",
        "[workspace]\n\n[[package]]\nname = \"demo\"\nchangelog_update = true\n\
         changelog_include = [\"demo_core\"]\n",
    ),
    ("renovate.json", "{}\n"),
    ("lychee.toml", "max_retries = 2\n"),
    (
        ".cargo/config.toml",
        "[target.x86_64-pc-windows-msvc]\nrustflags = [\"-C\", \"target-feature=+crt-static\"]\n",
    ),
    (".editorconfig", "root = true\n"),
    (".gitattributes", "* text=auto\n"),
];

/// A generated project in `project/` of a temporary directory, deleted on drop.
struct Project {
    /// The temporary directory, which holds the project and the fixture lints beside it.
    directory: TempDir,
}

impl Project {
    /// A project that keeps every rule: the members `demo` and `demo_core`, the kept tool
    /// configuration files, a `README.md` that links `docs/install.md`, and the fixture
    /// `trellis_lints` in `../lints/`, which the dylint libraries name by path.
    fn new() -> Self {
        let project = Self {
            directory: TempDir::new().expect("the temporary directory should be created"),
        };
        project.root_manifest("", PATH_LIBRARY);
        project.write(
            "crates/demo/Cargo.toml",
            &member_manifest("demo", "demo_core.workspace = true\n"),
        );
        project.write("crates/demo/src/main.rs", "//! The demo.\n\nfn main() {}\n");
        project.write(
            "crates/demo_core/Cargo.toml",
            &member_manifest("demo_core", ""),
        );
        project.write("crates/demo_core/src/lib.rs", "//! The demo's core.\n");
        project.lints_manifest("../lints", "trellis_lints");
        for (file, contents) in CONFIG_FILES {
            project.write(file, contents);
        }
        project.write("AGENTS.md", "# Working on the demo\n");
        project.write(
            "README.md",
            "# Demo\n\nSee [the install guide](docs/install.md#linux).\n",
        );
        project.write("docs/install.md", "# Install\n");
        project.write("docs/guides/setup.md", "# Setup\n");
        project.lock();
        project
    }

    /// Writes the root `Cargo.toml` with the `[workspace]` line `exclude` and the dylint
    /// `libraries`.
    fn root_manifest(&self, exclude: &str, libraries: &str) {
        self.write("Cargo.toml", &root_manifest(exclude, libraries));
    }

    /// Writes a lints manifest under `directory`, relative to the project, for the package
    /// `name`, a workspace of its own.
    fn lints_manifest(&self, directory: &str, name: &str) {
        self.write(
            &format!("{directory}/Cargo.toml"),
            &format!(
                "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n\
                 [lib]\ncrate-type = [\"cdylib\"]\n\n[workspace]\n"
            ),
        );
        self.write(&format!("{directory}/src/lib.rs"), "//! The lints.\n");
    }

    /// Writes the lockfile, so `cargo metadata --locked` reads the workspace.
    fn lock(&self) {
        let output = Command::new(env!("CARGO"))
            .args(["generate-lockfile", "--offline"])
            .current_dir(self.path())
            .output()
            .expect("cargo should run");
        assert!(
            output.status.success(),
            "cargo generate-lockfile failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// The project's directory.
    fn path(&self) -> PathBuf {
        self.directory.path().join("project")
    }

    /// Writes `contents` to `path` in the project.
    fn write(&self, path: &str, contents: &str) {
        let path = self.path().join(path);
        let directory = path
            .parent()
            .expect("a file in the project has a directory");
        fs::create_dir_all(directory).expect("the directory should be created");
        fs::write(path, contents).expect("the file should be written");
    }

    /// Runs git in the project, with a fixed identity and no signing, so the user's git
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

    /// Commits every change in the project.
    fn commit(&self, message: &str) {
        self.git(&["add", "--all"]);
        self.git(&["commit", "--quiet", "--no-verify", "--message", message]);
    }

    /// Runs `trellis-policy` with `arguments` in the project.
    fn run(&self, arguments: &[&str]) -> Run {
        run(&self.path(), arguments, Ci::Local)
    }

    /// Runs `trellis-policy check --manifest-path <root>/Cargo.toml`.
    fn check(&self) -> Run {
        let manifest = self.path().join("Cargo.toml");
        let manifest = manifest
            .to_str()
            .expect("the temporary path should be UTF-8");
        run(
            &self.path(),
            &["check", "--manifest-path", manifest],
            Ci::Local,
        )
    }
}

/// What a run of the command did.
#[derive(Debug)]
struct Run {
    /// The exit code.
    code: Option<i32>,
    /// What it wrote to standard output.
    stdout: String,
    /// What it wrote to standard error.
    stderr: String,
}

impl Run {
    /// Asserts that the run exited with `code` and printed a line that contains `text`.
    fn assert_reports(&self, code: i32, text: &str) {
        assert_eq!(
            self.code,
            Some(code),
            "the exit code should be {code}: {self:#?}"
        );
        assert!(
            self.stdout.lines().any(|line| line.contains(text)),
            "a line should contain {text:?}: {self:#?}"
        );
    }
}

/// Where the command runs, which decides how it writes errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ci {
    /// Outside CI.
    Local,
    /// Under GitHub Actions.
    GitHubActions,
}

/// Runs the command with `arguments` in `directory`, under `ci`.
fn run(directory: &Path, arguments: &[&str], ci: Ci) -> Run {
    let mut command = Command::new(env!("CARGO_BIN_EXE_trellis-policy"));
    command
        .args(arguments)
        .current_dir(directory)
        .env("CARGO", env!("CARGO"))
        .env_remove("GITHUB_ACTIONS");
    if ci == Ci::GitHubActions {
        command.env("GITHUB_ACTIONS", "true");
    }
    let Output {
        status,
        stdout,
        stderr,
    } = command.output().expect("trellis-policy should run");
    Run {
        code: status.code(),
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
    }
}

#[test]
fn a_generated_project_passes() {
    let run = Project::new().check();
    run.assert_reports(0, "All 11 structure rules hold");
    assert_eq!(
        run.stdout.lines().count(),
        1,
        "a clean run prints only the summary: {run:#?}"
    );
}

#[test]
fn the_workspace_is_found_from_the_current_directory() {
    let project = Project::new();
    run(
        &project.path().join("crates/demo/src"),
        &["check"],
        Ci::Local,
    )
    .assert_reports(0, "All 11 structure rules hold");
}

#[test]
fn a_lints_library_at_an_absolute_path_passes() {
    let project = Project::new();
    let lints = TempDir::new().expect("the temporary directory should be created");
    fs::write(
        lints.path().join("Cargo.toml"),
        "[package]\nname = \"trellis_lints\"\n",
    )
    .expect("the lints manifest should be written");
    let path = lints
        .path()
        .to_str()
        .expect("the temporary path should be UTF-8");
    project.root_manifest("", &format!("[{{ path = {path:?} }}]"));
    project
        .check()
        .assert_reports(0, "All 11 structure rules hold");
}

#[test]
fn a_lints_library_inside_the_workspace_fails() {
    let project = Project::new();
    project.lints_manifest("vendored", "trellis_lints");
    let absolute = project.path().join("vendored");
    let absolute = absolute
        .to_str()
        .expect("the temporary path should be UTF-8");
    for libraries in [
        r#"[{ path = "vendored" }]"#.to_owned(),
        r#"[{ path = "../project/vendored" }]"#.to_owned(),
        r#"[{ path = "." }]"#.to_owned(),
        format!("[{{ path = {absolute:?} }}]"),
    ] {
        project.root_manifest("", &libraries);
        project.check().assert_reports(
            1,
            "is inside the workspace, so a change to its sources counts as code",
        );
    }
}

#[cfg(unix)]
#[test]
fn a_lints_library_linked_into_the_workspace_fails() {
    let project = Project::new();
    project.lints_manifest("vendored", "trellis_lints");
    let link = project.directory.path().join("link");
    unix::fs::symlink(project.path().join("vendored"), &link).expect("the link should be created");
    project.root_manifest("", r#"[{ path = "../link" }]"#);
    project.check().assert_reports(
        1,
        "is inside the workspace, so a change to its sources counts as code",
    );
}

#[test]
fn a_page_the_readme_doesnt_link_fails() {
    let project = Project::new();
    project.write("docs/extra.md", "# Extra\n");
    project.write("docs/Upper.MD", "# Upper\n");
    let run = project.check();
    run.assert_reports(
        1,
        "error: [docs_index] README.md: `README.md` doesn't link the page `docs/extra.md`, so \
         readers don't find it. Add a relative link such as `[extra](docs/extra.md)`",
    );
    run.assert_reports(1, "doesn't link the page `docs/Upper.MD`");
    assert!(
        !run.stdout.contains("docs/install.md") && !run.stdout.contains("docs/guides"),
        "the linked page and the page in a subdirectory pass: {run:#?}"
    );
    run.assert_reports(1, "2 structure rule violation(s)");
}

#[test]
fn a_missing_readme_fails_for_each_page() {
    let project = Project::new();
    fs::remove_file(project.path().join("README.md")).expect("the readme should be removed");
    project.check().assert_reports(
        1,
        "there is no root `README.md` to link the page `docs/install.md`",
    );
}

#[test]
fn a_dylint_library_of_another_package_fails() {
    let project = Project::new();
    project.lints_manifest("../lints", "other_lints");
    project.check().assert_reports(
        1,
        "[workspace_shape] Cargo.toml: `workspace.metadata.dylint.libraries`: the entry \
         { path = \"../lints\" } names",
    );
    project.check().assert_reports(
        1,
        "whose `package.name` is `other_lints`, not `trellis_lints`",
    );
}

#[test]
fn an_unpinned_or_extra_dylint_library_fails() {
    let project = Project::new();
    project.root_manifest(
        "",
        r#"[{ git = "https://github.com/265866/trellis", rev = "abc123", pattern = "lints" }]"#,
    );
    project
        .check()
        .assert_reports(1, "doesn't pin a full lowercase commit hash in `rev`");
    project.root_manifest(
        "",
        r#"[{ path = "lints" }, { git = "https://example.com/lints", tag = "v1" }]"#,
    );
    project
        .check()
        .assert_reports(1, "but it must hold exactly one entry");
}

#[test]
fn an_excluded_crate_fails() {
    let project = Project::new();
    project.root_manifest("exclude = [\"lints\"]\n", PATH_LIBRARY);
    project.check().assert_reports(
        1,
        "[workspace_shape] Cargo.toml: `workspace.exclude` must be missing or empty but is \
         [\"lints\"]",
    );
}

#[test]
fn violations_are_annotations_under_github_actions() {
    let project = Project::new();
    project.write("docs/extra.md", "# Extra\n");
    let manifest = project.path().join("Cargo.toml");
    let manifest = manifest
        .to_str()
        .expect("the temporary path should be UTF-8");
    run(
        &project.path(),
        &["check", "--manifest-path", manifest],
        Ci::GitHubActions,
    )
    .assert_reports(1, "::error::[docs_index] README.md: ");
}

#[test]
fn a_usage_or_load_error_exits_with_2() {
    let project = Project::new();
    let run_in_project = |arguments: &[&str]| run(&project.path(), arguments, Ci::Local);
    for arguments in [
        &["check", "--manifest-path"][..],
        &["check", "--bogus"],
        &["unknown-command"],
        &["rule-changes", "--help"],
        &[],
    ] {
        let run = run_in_project(arguments);
        assert_eq!(
            run.code,
            Some(2),
            "{arguments:?} is a usage error: {run:#?}"
        );
        assert!(
            run.stderr.starts_with("usage: trellis-policy check"),
            "{arguments:?} prints the usage: {run:#?}"
        );
    }
    let missing = project.path().join("missing/Cargo.toml");
    let missing = missing
        .to_str()
        .expect("the temporary path should be UTF-8");
    run_in_project(&["check", "--manifest-path", missing]).assert_reports(
        2,
        "error: couldn't load the workspace: `cargo metadata` failed",
    );
}

#[test]
fn adding_or_removing_a_build_script_is_one_rule_change_that_passes() {
    let project = Project::new();
    project.git(&["init", "--quiet"]);
    project.commit("a generated project");
    project.git(&["branch", "without"]);
    let root = fs::read_to_string(project.path().join("Cargo.toml"))
        .expect("the root manifest should be read");
    project.write(
        "Cargo.toml",
        &format!("{root}\n[workspace.metadata.policy]\nbuild_script_crates = [\"demo\"]\n"),
    );
    project.write("crates/demo/build.rs", "fn main() {}\n");
    project.write(
        "crates/demo/Cargo.toml",
        &format!(
            "{}\n[build-dependencies]\ndemo_core.workspace = true\n",
            member_manifest("demo", "demo_core.workspace = true\n")
        ),
    );
    project.commit("add a build script");
    project.git(&["branch", "with"]);
    project
        .check()
        .assert_reports(0, "All 11 structure rules hold");
    project
        .run(&["rule-changes", "without", "--head", "HEAD"])
        .assert_reports(
            0,
            "Rule changes stand alone: 3 rule files and 0 code files changed",
        );

    project.root_manifest("", PATH_LIBRARY);
    fs::remove_file(project.path().join("crates/demo/build.rs"))
        .expect("the build script should be removed");
    project.write(
        "crates/demo/Cargo.toml",
        &member_manifest("demo", "demo_core.workspace = true\n"),
    );
    project.commit("remove the build script");
    project
        .check()
        .assert_reports(0, "All 11 structure rules hold");
    project
        .run(&["rule-changes", "with", "--head", "HEAD"])
        .assert_reports(
            0,
            "Rule changes stand alone: 3 rule files and 0 code files changed",
        );
}

#[test]
fn a_proc_macro_lands_as_a_rule_change_then_a_code_change_that_pass() {
    let project = Project::new();
    project.git(&["init", "--quiet"]);
    project.commit("a generated project");
    project.git(&["branch", "generated"]);
    let root = fs::read_to_string(project.path().join("Cargo.toml"))
        .expect("the root manifest should be read");
    project.write(
        "Cargo.toml",
        &format!("{root}\n[workspace.metadata.policy]\nproc_macro_crates = [\"serde_derive\"]\n"),
    );
    project.commit("list a proc macro");
    project.git(&["branch", "listed"]);
    project
        .check()
        .assert_reports(0, "All 11 structure rules hold");
    project
        .run(&["rule-changes", "generated", "--head", "HEAD"])
        .assert_reports(
            0,
            "Rule changes stand alone: 1 rule files and 0 code files changed",
        );

    project.write(
        "crates/demo_core/Cargo.toml",
        &member_manifest("demo_core", "serde_derive = \"1\"\n"),
    );
    project.write(
        "crates/demo_core/src/lib.rs",
        "//! The demo's core.\n\n/// A point.\n#[derive(serde_derive::Serialize)]\npub struct Point;\n",
    );
    project.lock();
    project.commit("derive with the proc macro");
    project
        .check()
        .assert_reports(0, "All 11 structure rules hold");
    project
        .run(&["rule-changes", "listed", "--head", "HEAD"])
        .assert_reports(
            0,
            "Rule changes stand alone: 0 rule files and 2 code files changed",
        );
}

#[test]
fn a_stale_build_script_entry_fails() {
    let project = Project::new();
    let root = fs::read_to_string(project.path().join("Cargo.toml"))
        .expect("the root manifest should be read");
    project.write(
        "Cargo.toml",
        &format!(
            "{root}\n[workspace.metadata.policy]\nbuild_script_crates = [\"demo\", \"gone\"]\n"
        ),
    );
    let run = project.check();
    run.assert_reports(
        1,
        "error: [build_scripts] Cargo.toml: `build_script_crates` in \
         `[workspace.metadata.policy]` lists `demo`, which has no build script any more.",
    );
    run.assert_reports(1, "lists `gone`, which isn't a workspace member.");
}

#[test]
fn a_member_the_release_changelog_leaves_out_fails() {
    let project = Project::new();
    project.write(
        "release-plz.toml",
        "[workspace]\nchangelog_update = false\n\n[[package]]\nname = \"demo\"\n\
         changelog_path = \"./CHANGELOG.md\"\nchangelog_update = true\nchangelog_include = []\n",
    );
    let run = project.check();
    run.assert_reports(
        1,
        "error: [release_changelog] release-plz.toml: `demo_core` is a workspace member that \
         `changelog_include` of the `demo` entry in `release-plz.toml` doesn't list",
    );
    run.assert_reports(1, "1 structure rule violation(s)");

    project.write(
        "release-plz.toml",
        "[[package]]\nname = \"demo\"\nchangelog_update = true\n\
         changelog_include = [\"demo_core\", \"demo_server\"]\n",
    );
    project
        .check()
        .assert_reports(0, "All 11 structure rules hold");
}
