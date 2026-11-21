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

[workspace.metadata.policy]
docs_files = ["README.md", "AGENTS.md", "docs/install.md"]
readme_sections = ["Install", "Documentation"]

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

/// The project's `README.md`, with the sections `readme_sections` lists and an index that links
/// its one page.
const README: &str = "# Demo\n\nThe demo.\n\n## Install\n\n\
                      See [the install guide](docs/install.md).\n\n## Documentation\n\n\
                      - [Install](docs/install.md#linux): install the demo.\n";

/// A generated project in `project/` of a temporary directory, deleted on drop.
struct Project {
    /// The temporary directory, which holds the project and the fixture lints beside it.
    directory: TempDir,
}

impl Project {
    /// A project that keeps every rule: the members `demo` and `demo_core`, the kept tool
    /// configuration files, a `README.md` whose index links `docs/install.md`, and the fixture
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
        project.write("README.md", README);
        project.write(
            "docs/install.md",
            "# Install\n\nFor people who run the demo: how to install it.\n",
        );
        project.lock();
        project
    }

    /// Writes the root `Cargo.toml` with the `[workspace]` line `exclude` and the dylint
    /// `libraries`.
    fn root_manifest(&self, exclude: &str, libraries: &str) {
        self.write("Cargo.toml", &root_manifest(exclude, libraries));
    }

    /// Adds the line `setting` to the `[workspace.metadata.policy]` table of the root
    /// `Cargo.toml`.
    fn policy_setting(&self, setting: &str) {
        let root = fs::read_to_string(self.path().join("Cargo.toml"))
            .expect("the root manifest should be read");
        let table = "[workspace.metadata.policy]\n";
        assert!(root.contains(table), "the root manifest has the table");
        self.write(
            "Cargo.toml",
            &root.replacen(table, &format!("{table}{setting}\n"), 1),
        );
    }

    /// Replaces the first `old` in the root `Cargo.toml` with `new`.
    fn edit_root_manifest(&self, old: &str, new: &str) {
        let root = fs::read_to_string(self.path().join("Cargo.toml"))
            .expect("the root manifest should be read");
        assert!(root.contains(old), "the root manifest has {old:?}");
        self.write("Cargo.toml", &root.replacen(old, new, 1));
    }

    /// Removes the line that starts with `key` from the root `Cargo.toml`.
    fn remove_policy_setting(&self, key: &str) {
        let root = fs::read_to_string(self.path().join("Cargo.toml"))
            .expect("the root manifest should be read");
        let kept: Vec<&str> = root
            .lines()
            .filter(|line| !line.starts_with(&format!("{key} = ")))
            .collect();
        self.write("Cargo.toml", &format!("{}\n", kept.join("\n")));
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
    /// Asserts that the run exited with 0 and printed only the one-line summary.
    fn assert_passes(&self) {
        assert_eq!(self.code, Some(0), "the exit code should be 0: {self:#?}");
        assert_eq!(
            self.stdout.lines().count(),
            1,
            "a clean run prints only the summary: {self:#?}"
        );
    }

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
    run.assert_passes();
    run.assert_reports(0, "All 12 structure rules hold");
}

#[test]
fn the_workspace_is_found_from_the_current_directory() {
    let project = Project::new();
    run(
        &project.path().join("crates/demo/src"),
        &["check"],
        Ci::Local,
    )
    .assert_passes();
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
    project.check().assert_passes();
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

#[cfg(unix)]
#[test]
fn a_symbolic_link_in_a_crate_fails() {
    let project = Project::new();
    unix::fs::symlink(
        "../../../README.md",
        project.path().join("crates/demo_core/src/extra.rs"),
    )
    .expect("the link should be created");
    project.check().assert_reports(
        1,
        "error: [member_sources] crates/demo_core/src/extra.rs: This is a symbolic link in or \
         above the crate `demo_core`, so the crate could read a file from anywhere.",
    );
}

#[test]
fn a_page_the_index_doesnt_link_fails() {
    let project = Project::new();
    project.write("docs/extra.md", "# Extra\n\nAn extra page.\n");
    project.write("docs/Upper.MD", "# Upper\n\nAn upper page.\n");
    let run = project.check();
    run.assert_reports(
        1,
        "error: [docs_index] README.md: the `## Documentation` section of `README.md` doesn't \
         link the page `docs/extra.md`, so readers don't find it. Add an entry such as \
         `- [extra](docs/extra.md): what the reader does with it.` under `## Documentation` in \
         `README.md`, or delete the page if nothing needs it.",
    );
    run.assert_reports(1, "doesn't link the page `docs/Upper.MD`");
    assert!(
        !run.stdout.contains("docs/install.md"),
        "the linked page passes: {run:#?}"
    );
    run.assert_reports(1, "2 structure rule violation(s)");
}

#[test]
fn a_link_outside_the_index_doesnt_count() {
    let project = Project::new();
    project.write(
        "README.md",
        "# Demo\n\nThe demo.\n\n## Install\n\nSee [the install guide](docs/install.md).\n",
    );
    let run = project.check();
    run.assert_reports(
        1,
        "[docs_index] README.md: `README.md` has no `## Documentation` section to link the page \
         `docs/install.md`",
    );
    run.assert_reports(
        1,
        "[docs_layout] README.md: The readme's `##` sections are `Install`, but `readme_sections` \
         in `[workspace.metadata.policy]` lists `Install`, `Documentation`.",
    );
    run.assert_reports(1, "2 structure rule violation(s)");
}

#[test]
fn a_missing_readme_fails_for_each_page() {
    let project = Project::new();
    fs::remove_file(project.path().join("README.md")).expect("the readme should be removed");
    let run = project.check();
    run.assert_reports(
        1,
        "there is no root `README.md` to link the page `docs/install.md`",
    );
    run.assert_reports(
        1,
        "[docs_layout] README.md: `docs_files` in `[workspace.metadata.policy]` lists this file, \
         but it doesn't exist.",
    );
    run.assert_reports(1, "2 structure rule violation(s)");
}

#[test]
fn missing_docs_settings_fail() {
    let project = Project::new();
    project.remove_policy_setting("docs_files");
    project.remove_policy_setting("readme_sections");
    let run = project.check();
    run.assert_reports(
        1,
        "error: [docs_layout] Cargo.toml: `docs_files` isn't set in \
         `[workspace.metadata.policy]`, so nothing says which docs files the project keeps. Set \
         it to the files that must exist, such as \
         `docs_files = [\"README.md\", \"CONTRIBUTING.md\", \"docs/design.md\"]`.",
    );
    run.assert_reports(
        1,
        "error: [docs_layout] Cargo.toml: `readme_sections` isn't set in \
         `[workspace.metadata.policy]`, so nothing says which sections the root `README.md` has.",
    );
    run.assert_reports(1, "2 structure rule violation(s)");
}

#[test]
fn an_unknown_docs_setting_is_a_load_error() {
    let project = Project::new();
    project.policy_setting("docs_file = []");
    project.check().assert_reports(
        2,
        "it has a key `docs_file` the checks don't know; the keys are `build_script_crates`, \
         `proc_macro_crates`, `internal_dependencies`, `dev_only_crates`, `docs_files`, \
         `readme_sections`",
    );
}

#[test]
fn a_missing_docs_file_fails() {
    let project = Project::new();
    fs::remove_file(project.path().join("AGENTS.md")).expect("AGENTS.md should be removed");
    let run = project.check();
    run.assert_reports(
        1,
        "error: [docs_layout] AGENTS.md: `docs_files` in `[workspace.metadata.policy]` lists this \
         file, but it doesn't exist. Restore it, or remove it from `docs_files` in a rule change \
         if the project no longer keeps it.",
    );
    run.assert_reports(1, "1 structure rule violation(s)");
}

#[test]
fn readme_sections_must_match_in_order() {
    let project = Project::new();
    let index = "## Documentation\n\n- [Install](docs/install.md): install the demo.\n";
    for (readme, found) in [
        (
            format!("# Demo\n\n{index}\n## Install\n\nRun it.\n"),
            "`Documentation`, `Install`",
        ),
        (
            format!("# Demo\n\n## Install\n\nRun it.\n\n{index}\n## Usage\n\nMore.\n"),
            "`Install`, `Documentation`, `Usage`",
        ),
        (
            format!("# Demo\n\n## Setup\n\nRun it.\n\n{index}"),
            "`Setup`, `Documentation`",
        ),
    ] {
        project.write("README.md", &readme);
        let run = project.check();
        run.assert_reports(
            1,
            &format!(
                "[docs_layout] README.md: The readme's `##` sections are {found}, but \
                 `readme_sections` in `[workspace.metadata.policy]` lists `Install`, \
                 `Documentation`. Give the readme exactly those sections, in that order, and \
                 move anything longer than a few commands to a page in `docs/`, or change \
                 `readme_sections` in a rule change if the project's sections change."
            ),
        );
        run.assert_reports(1, "1 structure rule violation(s)");
    }
    project.write(
        "README.md",
        &format!(
            "# Demo\n\n```md\n## Not a section\n```\n\n## Install\n\nRun it.\n\n\
             ### Linux\n\n{index}"
        ),
    );
    project.check().assert_passes();
}

#[test]
fn a_setext_heading_in_the_readme_fails() {
    let project = Project::new();
    project.write(
        "README.md",
        &README.replace("## Install\n", "Usage\n-----\n\n## Install\n"),
    );
    let run = project.check();
    run.assert_reports(
        1,
        "[docs_layout] README.md: Line 6 underlines the line above it, which makes that line a \
         heading. Write every heading as a `#` line",
    );
    run.assert_reports(1, "The readme's `##` sections are `Usage`, `Install`");
    run.assert_reports(1, "2 structure rule violation(s)");
}

#[test]
fn an_index_entry_that_isnt_a_link_and_a_description_fails() {
    let project = Project::new();
    project.write(
        "README.md",
        &format!(
            "{README}- [Contributing](https://example.com/CONTRIBUTING.md): how to help.\n\
             - [Agents](AGENTS.md)\n\nMore pages are coming.\n"
        ),
    );
    let run = project.check();
    run.assert_reports(
        1,
        "[docs_layout] README.md: The `## Documentation` entry \
         `[Contributing](https://example.com/CONTRIBUTING.md): how to help.` isn't an index \
         entry: its link isn't a relative path to a file in the repository, written without \
         entities or escapes.",
    );
    run.assert_reports(
        1,
        "The `## Documentation` entry `[Agents](AGENTS.md)` isn't an index entry: the link isn't \
         followed by `: ` and what the reader does with the page.",
    );
    run.assert_reports(
        1,
        "[docs_layout] README.md: The `## Documentation` section holds `More pages are coming.`, \
         but it may hold only its index",
    );
    run.assert_reports(1, "3 structure rule violation(s)");
}

#[test]
fn a_page_without_a_title_and_a_paragraph_fails() {
    let project = Project::new();
    for (page, problem) in [
        (
            "Install the demo.\n",
            "It doesn't start with a `# Title` line.",
        ),
        (
            "# Install\n\n## Linux\n",
            "Its title isn't followed by a paragraph",
        ),
        (
            "# Install\n\n```sh\njust install\n```\n",
            "Its title isn't followed by a paragraph",
        ),
    ] {
        project.write("docs/install.md", page);
        let run = project.check();
        run.assert_reports(1, &format!("[docs_layout] docs/install.md: {problem}"));
        run.assert_reports(
            1,
            "A page starts with its title as a `# Title` line, then a blank line and a paragraph \
             that says who the page is for and what it gets them, before any other heading, \
             list, table or code block.",
        );
        run.assert_reports(1, "1 structure rule violation(s)");
    }
    project.write(
        "docs/install.md",
        "# Install\n\nFor people who run the demo.\n",
    );
    project.check().assert_passes();
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
fn rule_changes_has_nothing_to_compare_until_a_remote_branch_exists() {
    let project = Project::new();
    project.git(&["init", "--quiet"]);
    project.commit("a generated project");
    let nothing_to_compare = "Nothing to compare yet: `origin/main` isn't a commit";
    project
        .run(&["rule-changes", "origin/main"])
        .assert_reports(0, nothing_to_compare);
    project
        .run(&["rule-changes", "origin/main", "--head", "no-such-commit"])
        .assert_reports(
            2,
            "error: `git rev-parse --verify no-such-commit^{commit}` failed",
        );

    project.git(&[
        "remote",
        "add",
        "origin",
        "https://github.com/example/demo.git",
    ]);
    project
        .run(&["rule-changes", "origin/main"])
        .assert_reports(0, nothing_to_compare);

    project.git(&["update-ref", "refs/remotes/origin/feature", "HEAD"]);
    project
        .run(&["rule-changes", "origin/main"])
        .assert_reports(2, "error: `git merge-base origin/main HEAD` failed");
}

#[test]
fn adding_or_removing_a_build_script_is_one_rule_change_that_passes() {
    let project = Project::new();
    project.git(&["init", "--quiet"]);
    project.commit("a generated project");
    project.git(&["branch", "without"]);
    project.policy_setting("build_script_crates = [\"demo\"]");
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
    project.check().assert_passes();
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
    project.check().assert_passes();
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
    project.policy_setting("proc_macro_crates = [\"serde_derive\"]");
    project.commit("list a proc macro");
    project.git(&["branch", "listed"]);
    project.check().assert_passes();
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
    project.check().assert_passes();
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
    project.policy_setting("build_script_crates = [\"demo\", \"gone\"]");
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
    project.check().assert_passes();
}

/// The project's `README.md` with `extra` at the end of its `Install` section.
fn readme_with_install(extra: &str) -> String {
    README.replace(
        "## Documentation\n",
        &format!("{extra}\n\n## Documentation\n"),
    )
}

/// Asserts that `readme` fails `docs_layout` on `README.md` with a message that contains `text`.
fn assert_readme_fails(project: &Project, readme: &str, text: &str) {
    project.write("README.md", readme);
    let run = project.check();
    assert!(
        run.code == Some(1)
            && run
                .stdout
                .lines()
                .any(|line| line.contains("[docs_layout] README.md: ") && line.contains(text)),
        "the readme should fail with {text:?}:\n{readme}\n{run:#?}"
    );
}

/// The message for `##` sections other than `readme_sections`.
const SECTIONS: &str = "The readme's `##` sections are";

/// The message for a `##` heading inside a list item, quote or footnote.
const NESTED: &str = "inside a list item, quote or footnote. A section heading must be a top-level";

/// The message for a setext heading.
const SETEXT: &str = "underlines the line above it";

/// The message for an HTML heading tag.
const HTML_HEADING: &str = "has an HTML heading tag";

/// The message for HTML that doesn't close.
const UNCLOSED: &str = "opens HTML that doesn't close";

/// The message for a `##` heading with markup.
const MARKUP: &str = "Write the heading as plain text";

/// README snippets that GitHub renders as a heading, or that hide what follows, each placed at
/// the end of the `Install` section, with the message each must fail with.
const README_REGRESSIONS: [(&str, &str, &str); 60] = [
    ("setext in a list item", "- Extra\n  ---", NESTED),
    (
        "level-1 setext in a list item",
        "- Extra\n  ===",
        "has 2 `#` headings",
    ),
    ("setext in a quote", "> Extra\n> ---", NESTED),
    ("list in a list", "- - ## Extra", NESTED),
    ("list in a quote", "> - ## Extra", NESTED),
    ("quote in a list", "- > ## Extra", NESTED),
    (
        "nested item on the next line",
        "- item\n  - ## Extra",
        NESTED,
    ),
    ("second ordered item", "1. first\n2. ## Extra", NESTED),
    ("quote, a space and a tab", "> \t## Extra", NESTED),
    ("tab after a list marker", "-\ta\n\n    ## Extra", NESTED),
    (
        "tab after a list marker, HTML",
        "-\ta\n\n    <h2>Extra</h2>",
        HTML_HEADING,
    ),
    (
        "URL autolink",
        "<https://example.com/demo>\n## Extra",
        SECTIONS,
    ),
    ("email autolink", "<me@example.com>\n## Extra", SECTIONS),
    ("text that isn't a tag", "<x =>\n## Extra", SECTIONS),
    ("empty processing instruction", "<?>\n## Extra", SECTIONS),
    (
        "definition-like setext",
        "[Extra section]: more text here\n---",
        SETEXT,
    ),
    ("table-like setext", "Extra | section\n-|-|-\n---", SETEXT),
    (
        "level-1 table-like setext",
        "Extra | section\n-|-|-\n===",
        SETEXT,
    ),
    (
        "definition-like line, then HTML",
        "[x]: y z\n    <h2>Extra</h2>",
        HTML_HEADING,
    ),
    ("escaped code span", "\\`<h2>Extra</h2>\\`", HTML_HEADING),
    (
        "code span from the line before",
        "Run `a\nx` <h2>Extra</h2> `y`",
        HTML_HEADING,
    ),
    (
        "backticks in a div",
        "<div>`<h2>Extra</h2>`</div>",
        HTML_HEADING,
    ),
    ("a lone carriage return", "Some text.\r## Extra", SECTIONS),
    (
        "lowercase declaration",
        "<!a\n## Extra",
        "`Install`, `Extra`, `Documentation`",
    ),
    (
        "lowercase declaration in an item",
        "- <!x\n\n  ## Extra",
        NESTED,
    ),
    (
        "lowercase declaration, then a quote",
        "<!x\n\n## Extra\n\n> quote",
        SECTIONS,
    ),
    (
        "lowercase doctype",
        "<!doctype html\n\n## Extra\n\nRun `a -> b`.",
        SECTIONS,
    ),
    ("search tag", "<search>x\n## Extra", SECTIONS),
    (
        "search tag and a form feed",
        "<search\x0cx\n## Extra",
        SECTIONS,
    ),
    (
        "source tag, a form feed and no end",
        "<source\x0cx\n## Hidden",
        UNCLOSED,
    ),
    ("search tag in a quote", "> <search>x\n> ## Extra", NESTED),
    (
        "search tag in a footnote",
        "Note.[^1]\n\n[^1]: <search>x\n    ## Extra",
        NESTED,
    ),
    (
        "referenced footnote body",
        "See this.[^1]\n\n[^1]: note\n\n    ## Extra",
        NESTED,
    ),
    ("no-break space", "\u{a0}\n---", SETEXT),
    (
        "empty item, then a heading",
        "-\n\n  ## Extra",
        "`Install`, `Extra`, `Documentation`",
    ),
    (
        "comment ended by --!>",
        "<div><!-- a --!> <h2>Extra</h2> --></div>",
        HTML_HEADING,
    ),
    (
        "comment opener in a value",
        "<div><a b=\"<!--\"><h2>Extra</h2><i c=\"-->\"></i></div>",
        HTML_HEADING,
    ),
    (
        "form feed after a tag name",
        "<div><h2\x0cid=x>Extra</div>",
        HTML_HEADING,
    ),
    (
        "quote after a tag name",
        "<div><h2\"id=x>Extra</div>",
        HTML_HEADING,
    ),
    ("unclosed comment in an item", "- <!--\n\nText.", UNCLOSED),
    ("unclosed comment in a div", "<div><!-- a\n</div>", UNCLOSED),
    (
        "unclosed attribute value",
        "<div title=\"\n</div>",
        UNCLOSED,
    ),
    ("start tag with no end", "<div\n## Extra", UNCLOSED),
    (
        "heading with a link and an image",
        "## [Extra](x.md) ![img](a.png)",
        MARKUP,
    ),
    ("inline heading tags", "Text <h2>x</h2>.", HTML_HEADING),
    (
        "details left open with a code fence",
        "<details>\n<summary>More</summary>\n\n```sh\nrun\n```",
        "opens `<details>`, which doesn't close before the next section",
    ),
    (
        "misspelled closing tag",
        "<details>\n<summary>More</summary>\n\nText.\n\n</detail>",
        "opens `<details>`, which doesn't close before the next section",
    ),
    (
        "centered div left open",
        "<div align=\"center\">\n\n[![CI](https://example.com/ci.svg)](https://example.com/ci)",
        "opens `<div>`, which doesn't close before the next section",
    ),
    (
        "select",
        "<select>",
        "opens `<select>`, which doesn't close",
    ),
    (
        "here-document in a pre",
        "<pre>\ncat <<EOF\nhi\nEOF\n</pre>",
        UNCLOSED,
    ),
    (
        "usage line in a pre",
        "<pre>\nUsage: demo <file\n</pre>",
        UNCLOSED,
    ),
    (
        "quote in an attribute name",
        "<div it's title=\"it's>\nx\n</div>",
        UNCLOSED,
    ),
    (
        "lowercase CDATA in a div",
        "<div><![cdata[ x\n</div>",
        UNCLOSED,
    ),
    (
        "stray closing tag",
        "Text.\n\n</div>",
        "closes `</div>`, which no `<div>` open in its section",
    ),
    ("empty section heading", "##", "has an empty `##` heading"),
    ("inline element left open", "<span>a", "opens `<span>`"),
    (
        "slash in an unquoted value",
        "<details open=x/>\n<summary>More</summary>\n\nText.",
        "opens `<details>`, which doesn't close before the next section",
    ),
    (
        "element name in prose",
        "Run `demo` in a <dir> of your own.",
        "opens `<dir>`, which doesn't close before the next section, so GitHub renders what \
         follows inside it. Close it in the same section, or write a literal `<` as `&lt;`.",
    ),
    (
        "list item without its end tag",
        "<ul>\n<li>One\n</ul>",
        "opens `<li>`, which doesn't close before the next section. Close it; trellis requires \
         every element closed in its section. Or write a literal `<` as `&lt;`.",
    ),
    (
        "generic type in a pre",
        "<pre>\nHashMap<String, Vec<u8>>\n</pre>",
        "or write a literal `<` as `&lt;`",
    ),
];

/// Changes to the README's own lines, each from one text to another, with the message each must
/// fail with.
const README_EDIT_REGRESSIONS: [(&str, &str, &str, &str); 10] = [
    (
        "comment hiding a section",
        "## Install\n",
        "<!--\n\n## Install\n\n-->\n",
        SECTIONS,
    ),
    (
        "section in a quote",
        "## Install\n",
        "> ## Install\n",
        NESTED,
    ),
    (
        "section in a list",
        "## Install\n",
        "- ## Install\n",
        NESTED,
    ),
    (
        "emphasized section",
        "## Install\n",
        "## *Install*\n",
        MARKUP,
    ),
    (
        "linked section",
        "## Install\n",
        "## [Install](x.md)\n",
        MARKUP,
    ),
    (
        "section in a tag",
        "## Install\n",
        "## <kbd>Install</kbd>\n",
        MARKUP,
    ),
    (
        "section as an image",
        "## Install\n",
        "## ![Install](x.png)\n",
        MARKUP,
    ),
    (
        "empty title",
        "# Demo\n",
        "#\n",
        "Its `#` title has no text",
    ),
    (
        "image title",
        "# Demo\n",
        "# ![Demo](demo.png)\n",
        "Its `#` title has no text",
    ),
    (
        "section inside details",
        "## Install\n",
        "<details>\n\n## Install\n\n</details>\n",
        "opens `<details>`, which doesn't close before the next section",
    ),
];

/// Index entries for `docs/install.md`, with the message each must fail with.
const INDEX_REGRESSIONS: [(&str, &str, &str); 21] = [
    (
        "space and angle brackets",
        "- [Install]( <docs/install.md>): install the demo.",
        "has a title, angle brackets or spaces",
    ),
    (
        "empty title",
        "- [Install](docs/install.md \"\"): install the demo.",
        "has a title, angle brackets or spaces",
    ),
    (
        "image as the link text",
        "- [![Install](a.png)](docs/install.md): install the demo.",
        "its link has no text",
    ),
    (
        "task list item",
        "- [ ] [Install](docs/install.md): install the demo.",
        "task list item",
    ),
    (
        "empty path part",
        "- [Install](docs//install.md): install the demo.",
        "isn't a relative path",
    ),
    (
        "trailing slash",
        "- [Install](docs/install.md/): install the demo.",
        "isn't a relative path",
    ),
    (
        "words only in a comment",
        "- [Install](docs/install.md): <!-- install the demo -->.",
        "its description has no words",
    ),
    (
        "escaped bracket",
        "- [Install\\](docs/install.md): install the demo.",
        "doesn't start with a link",
    ),
    (
        "link with no closing parenthesis",
        "- [Install](docs/install.md: install the demo.",
        "doesn't start with a link",
    ),
    (
        "badge after the period",
        "- [Install](docs/install.md): install the demo. ![badge](b.svg)",
        "with nothing after it",
    ),
    (
        "footnote after the period",
        "- [Install](docs/install.md): install the demo.[^n]\n\n[^n]: A note.",
        "with nothing after it",
    ),
    (
        "HTML image after the period",
        "- [Install](docs/install.md): install the demo. <img src=\"b.svg\">",
        "with nothing after it",
    ),
    (
        "linked image after the period",
        "- [Install](docs/install.md): install the demo. [![b](b.svg)](x)",
        "with nothing after it",
    ),
    (
        "badges after the period",
        "- [Install](docs/install.md): install the demo. ![a](a.svg) ![b](b.svg)",
        "with nothing after it",
    ),
    (
        "encoded mail scheme",
        "- [Install](mailto&colon;a@b.co): install the demo.",
        "without entities or escapes",
    ),
    (
        "encoded web scheme",
        "- [Install](https&colon;&sol;&sol;example.com): install the demo.",
        "without entities or escapes",
    ),
    (
        "encoded parent",
        "- [Install](&period;&period;/x.md): install the demo.",
        "without entities or escapes",
    ),
    (
        "escaped parent",
        "- [Install](\\.\\./x.md): install the demo.",
        "without entities or escapes",
    ),
    (
        "escaped anchor",
        "- [Install](\\#install): install the demo.",
        "without entities or escapes",
    ),
    (
        "escaped root",
        "- [Install](\\/docs/install.md): install the demo.",
        "without entities or escapes",
    ),
    (
        "dot part",
        "- [Install](docs/install.md/.): install the demo.",
        "isn't a relative path",
    ),
];

/// README snippets that GitHub renders without an extra heading, each placed at the end of the
/// `Install` section. Each must pass.
const README_FALSE_POSITIVES: [(&str, &str); 22] = [
    ("comment over paragraphs", "<!--\n\n## Old section\n\n-->"),
    ("pre block", "<pre>\n\n# a comment\n</pre>"),
    ("source tag", "<source>x\n## Hidden"),
    ("unreferenced footnote", "[^1]: note\n\n    ## Unrendered"),
    (
        "footnote before the last section",
        "Built with Rust 1.100.[^msrv]\n\n[^msrv]: The oldest Rust it builds with.",
    ),
    ("fenced comments", "```sh\n# install it\njust install\n```"),
    (
        "ordered steps with code",
        "1. Install it:\n\n   ```sh\n   just install\n   ```\n\n2. Run it.",
    ),
    ("alert", "> [!NOTE]\n> Read this first."),
    (
        "table",
        "| Tool | Use |\n| --- | --- |\n| just | Runs the checks. |",
    ),
    (
        "details",
        "<details>\n<summary>More</summary>\n\nText.\n\n</details>",
    ),
    ("nested bullets", "- a\n  - b\n- c"),
    ("level-3 heading", "### Linux\n\nRun it."),
    (
        "badges in a section",
        "[![CI](https://example.com/ci.svg)](https://example.com/ci)",
    ),
    (
        "spawn signature in a pre",
        "<pre>\nfn spawn<F: 'static>(f: F)\n</pre>",
    ),
    (
        "apostrophe in an unquoted value",
        "<div title=it's>\nx\n</div>",
    ),
    (
        "details with a code fence",
        "<details>\n<summary>More</summary>\n\n```sh\nrun\n```\n\n</details>",
    ),
    (
        "picture, image and break",
        "<picture>\n<source srcset=\"a.png\">\n<img src=\"b.png\" alt=\"Logo\">\n</picture>\n\nLine one<br>line two.",
    ),
    (
        "centered badges",
        "<div align=\"center\">\n\n[![CI](https://example.com/ci.svg)](https://example.com/ci)\n\n</div>",
    ),
    ("inline element", "Press <kbd>Ctrl</kbd> and <kbd>C</kbd>."),
    (
        "picture whose source names a header image",
        "<picture>\n  <source media=\"(prefers-color-scheme: dark)\" \
         srcset=\"docs/header-dark.png\">\n  <img src=\"docs/header.png\" alt=\"Demo\" \
         width=\"400\">\n</picture>",
    ),
    (
        "link to a URL ending in a slash",
        "<a href=https://example.com/>the site</a>",
    ),
    (
        "search and form on one line",
        "<search><form>x</form></search>",
    ),
];

/// The line of the fixture's index entry for `docs/install.md`.
const INSTALL_ENTRY: &str = "- [Install](docs/install.md#linux): install the demo.\n";

/// Asserts that the project's check fails with a line that contains `expected`, for the case
/// `name`.
fn assert_case_fails(project: &Project, name: &str, expected: &str) {
    let run = project.check();
    assert!(
        run.code == Some(1) && run.stdout.lines().any(|line| line.contains(expected)),
        "{name} should fail with {expected:?}: {run:#?}"
    );
}

#[test]
fn readme_shapes_github_renders_as_headings_fail() {
    let project = Project::new();
    for (name, snippet, expected) in README_REGRESSIONS {
        project.write("README.md", &readme_with_install(snippet));
        assert_case_fails(&project, name, expected);
    }
    for (name, from, to, expected) in README_EDIT_REGRESSIONS {
        project.write("README.md", &README.replacen(from, to, 1));
        assert_case_fails(&project, name, expected);
    }
    for (name, entry, expected) in INDEX_REGRESSIONS {
        project.write(
            "README.md",
            &README.replace(INSTALL_ENTRY, &format!("{entry}\n")),
        );
        assert_case_fails(&project, name, expected);
    }
}

#[test]
fn readme_shapes_github_renders_without_a_heading_pass() {
    let project = Project::new();
    for (name, snippet) in README_FALSE_POSITIVES {
        project.write("README.md", &readme_with_install(snippet));
        let run = project.check();
        assert!(
            run.code == Some(0),
            "{name}: {snippet:?} should pass: {run:#?}"
        );
    }
    for (name, readme) in [
        (
            "reference badges before the title",
            format!(
                "[![CI][ci-badge]][ci]\n\n{README}\n[ci]: https://example.com/ci\n\
                     [ci-badge]: https://example.com/ci.svg\n"
            ),
        ),
        (
            "badges after the title",
            README.replacen(
                "# Demo\n",
                "# Demo\n\n[![CI](https://example.com/ci.svg)](https://example.com/ci)\n",
                1,
            ),
        ),
        (
            "closing `#` sequence",
            README.replacen("## Install\n", "##  Install  ##\n", 1),
        ),
        (
            "an SPDX comment before the title",
            format!("<!-- SPDX-License-Identifier: MIT -->\n\n{README}"),
        ),
        (
            "a comment in the index",
            README.replace(
                INSTALL_ENTRY,
                &format!("<!-- markdownlint-disable MD013 -->\n\n{INSTALL_ENTRY}"),
            ),
        ),
        (
            "an index entry with a leading ./",
            README.replace("](docs/install.md#linux)", "](./docs/install.md#linux)"),
        ),
        ("CRLF line breaks", README.replace('\n', "\r\n")),
        (
            "a tight ordered index",
            README.replace(
                INSTALL_ENTRY,
                "1. [Install](docs/install.md#linux): install the demo.\n\
                 2. [Agents](AGENTS.md): what agents follow.\n",
            ),
        ),
    ] {
        project.write("README.md", &readme);
        let run = project.check();
        assert!(run.code == Some(0), "{name} should pass: {run:#?}");
    }
}

#[test]
fn a_link_that_github_doesnt_render_doesnt_index_a_page() {
    let project = Project::new();
    for entry in [
        "- [Install\\](docs/install.md): install the demo.\n",
        "- [Install](docs/install.md: install the demo.\n",
        "- <!-- [Install](docs/install.md) --> install the demo.\n",
    ] {
        project.write(
            "README.md",
            &README.replace(
                "- [Install](docs/install.md#linux): install the demo.\n",
                entry,
            ),
        );
        project.check().assert_reports(
            1,
            "[docs_index] README.md: the `## Documentation` section of `README.md` doesn't \
             link the page `docs/install.md`",
        );
    }
}

#[test]
fn pages_github_renders_as_described_pass() {
    let project = Project::new();
    project.write(
        "docs/install.md",
        "# Install\n\n<https://example.com/demo> has the releases. [Set it up][setup] first.\n\n\
         ## Steps\n\n1. Download it.\n\n   ```sh\n   # the archive\n\
         \x20  tar -xf demo.tar.gz\n   ```\n\n\
         2. Run it.\n\n| Flag | Does |\n| --- | --- |\n| `-v` | Prints more. |\n\n\
         > [!TIP]\n> Read the guide.\n\n[setup]: guides/setup.md\n",
    );
    project.write(
        "docs/guides/setup.md",
        "# Setup\r\n\r\nFor people who set up the demo. Go [deeper](deep/more.md).\r\n",
    );
    project.write(
        "docs/guides/deep/more.md",
        "# More\n\nFor people who want more.\n",
    );
    project.check().assert_passes();
    project.write(
        "docs/guides/setup.md",
        "# Setup\n\nFor people who set up the demo.\n<!-- [deeper](deep/more.md) -->\n",
    );
    project.check().assert_reports(
        1,
        "[docs_index] docs/guides/deep/more.md: No page in `docs/guides/` links this page",
    );
}

#[test]
fn the_readme_has_one_title_first() {
    let project = Project::new();
    for (readme, text) in [
        (
            readme_with_install("# Another title\n"),
            "has 2 `#` headings",
        ),
        (
            README.replacen("# Demo\n", "Demo\n", 1),
            "has no `#` heading. It needs exactly one",
        ),
        (format!("Intro.\n\n{README}"), "Its `#` title comes after"),
    ] {
        assert_readme_fails(&project, &readme, text);
    }
    project.write(
        "README.md",
        &format!("[![CI](https://example.com/ci.svg)](https://example.com/ci)\n\n{README}"),
    );
    project.check().assert_passes();
}

#[test]
fn an_index_entry_needs_one_line_and_words() {
    let project = Project::new();
    for (index, text) in [
        (
            "- [Install](docs/install.md): .\n",
            "its description has no words",
        ),
        (
            "- [Install](docs/install.md): 1.\n",
            "its description has no words",
        ),
        (
            "- [Install](docs/install.md): install the\n  demo.\n",
            "one single-line list item per page or file",
        ),
        (
            "- [Install](docs/install.md): install the\ndemo.\n",
            "one single-line list item per page or file",
        ),
        (
            "- [Install](docs/install.md \"Install\"): install it.\n",
            "its link has a title, angle brackets or spaces",
        ),
        (
            "- [Install](<docs/install.md>): install it.\n",
            "its link has a title, angle brackets or spaces",
        ),
    ] {
        let readme = README.replace(
            "- [Install](docs/install.md#linux): install the demo.\n",
            index,
        );
        assert_readme_fails(&project, &readme, text);
    }
}

#[test]
fn readme_sections_can_change_in_a_rule_change() {
    let project = Project::new();
    assert_readme_fails(
        &project,
        &readme_with_install("## Usage\n"),
        "or change `readme_sections` in a rule change if the project's sections change.",
    );
}

#[test]
fn a_page_opening_that_isnt_a_paragraph_fails() {
    let project = Project::new();
    for opening in [
        "\tjust install\n",
        "<!-- For people who run the demo. -->\n",
        "![The demo](demo.png)\n",
        "[install]: install.md\n",
        "For people\nwho run it\n-----\n",
        "For people who run it.\n=====\n",
    ] {
        project.write("docs/install.md", &format!("# Install\n\n{opening}"));
        let run = project.check();
        run.assert_reports(
            1,
            "[docs_layout] docs/install.md: Its title isn't followed by a paragraph",
        );
    }
}

#[test]
fn a_page_with_a_byte_order_mark_fails() {
    let project = Project::new();
    project.write(
        "docs/install.md",
        "\u{feff}# Install\n\nFor people who run the demo.\n",
    );
    let run = project.check();
    run.assert_reports(
        1,
        "[docs_layout] docs/install.md: It starts with a byte order mark.",
    );
    run.assert_reports(1, "1 structure rule violation(s)");
}

#[test]
fn pages_in_subdirectories_are_linked_and_shaped() {
    let project = Project::new();
    project.write("docs/guides/setup.md", "No title.\n");
    let run = project.check();
    run.assert_reports(
        1,
        "[docs_index] docs/guides/setup.md: No page in `docs/` links this page",
    );
    run.assert_reports(
        1,
        "[docs_layout] docs/guides/setup.md: It doesn't start with a `# Title` line.",
    );
    run.assert_reports(1, "2 structure rule violation(s)");
    project.write(
        "docs/guides/setup.md",
        "# Setup\n\nFor people who set up the demo.\n",
    );
    project.write(
        "docs/install.md",
        "# Install\n\nFor people who run the demo. [Set it up](guides/setup.md#first) first.\n",
    );
    project.check().assert_passes();
}

#[test]
fn a_markdown_file_under_docs_without_the_md_extension_fails() {
    let project = Project::new();
    project.write("docs/extra.markdown", "Anything.\n");
    project.write("docs/guides/more.MKD", "Anything.\n");
    let run = project.check();
    run.assert_reports(
        1,
        "[docs_layout] docs/extra.markdown: Pages in `docs/` use the `.md` extension",
    );
    run.assert_reports(1, "[docs_layout] docs/guides/more.MKD: Pages in `docs/`");
    run.assert_reports(1, "2 structure rule violation(s)");
}

#[test]
fn a_link_in_an_html_comment_doesnt_count() {
    let project = Project::new();
    project.write(
        "docs/guides/setup.md",
        "# Setup\n\nFor people who set up the demo.\n",
    );
    project.write(
        "docs/install.md",
        "# Install\n\nFor people who run the demo.\n\n<!--\n\n[Set it up](guides/setup.md).\n\n\
         -->\n",
    );
    project.check().assert_reports(
        1,
        "[docs_index] docs/guides/setup.md: No page in `docs/` links this page",
    );
}

/// Pages for `docs/install.md`, with the message each must fail with.
const PAGE_REGRESSIONS: [(&str, &str, &str); 3] = [
    (
        "image title",
        "# ![Install](logo.png)\n\nFor people who run the demo.\n",
        "It doesn't start with a `# Title` line",
    ),
    (
        "unclosed attribute value",
        "# Install\n\nFor people who run the demo.\n\n<div title=\"\n</div>\n",
        "[docs_layout] docs/install.md: Line 5 opens HTML that doesn't close",
    ),
    (
        "unclosed comment",
        "# Install\n\nFor people who run the demo.\n\n<!--\n",
        "[docs_layout] docs/install.md: Line 5 opens HTML that doesn't close",
    ),
];

#[test]
fn page_shapes_that_hide_text_or_lack_a_title_fail() {
    let project = Project::new();
    for (name, page, expected) in PAGE_REGRESSIONS {
        project.write("docs/install.md", page);
        assert_case_fails(&project, name, expected);
    }
}

#[test]
fn a_link_with_an_empty_path_part_doesnt_link_a_page() {
    let project = Project::new();
    project.write(
        "docs/guides/setup.md",
        "# Setup\n\nFor people who set up the demo.\n",
    );
    for link in ["guides//setup.md", "guides/setup.md/", "guides/setup.md/."] {
        project.write(
            "docs/install.md",
            &format!("# Install\n\nFor people who run the demo. [Set it up]({link}).\n"),
        );
        assert_case_fails(
            &project,
            link,
            "[docs_index] docs/guides/setup.md: No page in `docs/` links this page",
        );
    }
}

#[test]
fn a_closing_tag_after_unclosed_html_isnt_reported_again() {
    let project = Project::new();
    project.write(
        "README.md",
        &readme_with_install("<div title=\"\n\n</details>"),
    );
    let run = project.check();
    run.assert_reports(1, UNCLOSED);
    assert!(
        !run.stdout.contains("closes `</details>`"),
        "the closing tag after unclosed HTML isn't reported: {run:#?}"
    );
    run.assert_reports(1, "1 structure rule violation(s)");
}

#[test]
fn a_page_may_start_with_comments_before_its_title() {
    let project = Project::new();
    project.write(
        "docs/install.md",
        "<!-- SPDX-License-Identifier: MIT -->\n\n# Install\n\nFor people who run the demo.\n",
    );
    project.check().assert_passes();
    project.write(
        "docs/install.md",
        "<!-- SPDX-License-Identifier: MIT -->\n\nText.\n\n# Install\n\nFor people.\n",
    );
    project.check().assert_reports(
        1,
        "[docs_layout] docs/install.md: It doesn't start with a `# Title` line.",
    );
}

#[test]
fn a_member_that_doesnt_inherit_the_workspace_license_fails() {
    let project = Project::new();
    project.edit_root_manifest("publish = false\n", "publish = false\nlicense = \"MIT\"\n");
    project.check().assert_reports(
        1,
        "error: [member_manifests] crates/demo_core/Cargo.toml: `[package] license` is missing, \
         but it must be `license.workspace = true`",
    );

    project.write(
        "crates/demo_core/Cargo.toml",
        &member_manifest("demo_core", "").replace(
            "publish.workspace = true\n",
            "publish.workspace = true\nlicense = \"MIT OR Apache-2.0\"\n",
        ),
    );
    project.check().assert_reports(
        1,
        "`[package] license` is \"MIT OR Apache-2.0\", but it must be `license.workspace = true`",
    );

    for crate_name in ["demo", "demo_core"] {
        let dependencies = if crate_name == "demo" {
            "demo_core.workspace = true\n"
        } else {
            ""
        };
        project.write(
            &format!("crates/{crate_name}/Cargo.toml"),
            &member_manifest(crate_name, dependencies).replace(
                "publish.workspace = true\n",
                "publish.workspace = true\nlicense.workspace = true\n",
            ),
        );
    }
    project.check().assert_passes();
}

#[test]
fn a_member_needs_no_license_when_the_workspace_has_none() {
    Project::new().check().assert_passes();
}

#[test]
fn a_member_outside_crates_fails() {
    let project = Project::new();
    project.write("tools/gen/Cargo.toml", &member_manifest("gen", ""));
    project.write("tools/gen/src/lib.rs", "//! Generates.\n");
    project.edit_root_manifest(
        "members = [\"crates/*\"]",
        "members = [\"crates/*\", \"tools/gen\"]",
    );
    project.lock();
    project.check().assert_reports(
        1,
        "error: [workspace_shape] tools/gen/Cargo.toml: The workspace member `gen` lives in \
         `tools/gen`",
    );
}

#[test]
fn a_nested_member_fails() {
    let project = Project::new();
    project.write("crates/demo/gen/Cargo.toml", &member_manifest("gen", ""));
    project.write("crates/demo/gen/src/lib.rs", "//! Generates.\n");
    project.edit_root_manifest(
        "members = [\"crates/*\"]",
        "members = [\"crates/*\", \"crates/demo/gen\"]",
    );
    project.lock();
    project.check().assert_reports(
        1,
        "error: [workspace_shape] crates/demo/gen/Cargo.toml: The workspace member `gen` lives \
         in `crates/demo/gen`",
    );
}

#[test]
fn a_path_dependency_in_the_workspace_is_a_member_that_must_live_under_crates() {
    let project = Project::new();
    project.write("tools/gen/Cargo.toml", &member_manifest("gen", ""));
    project.write("tools/gen/src/lib.rs", "//! Generates.\n");
    project.write(
        "crates/demo_core/Cargo.toml",
        &member_manifest("demo_core", "gen = { path = \"../../tools/gen\" }\n"),
    );
    project.lock();
    project.check().assert_reports(
        1,
        "error: [workspace_shape] tools/gen/Cargo.toml: The workspace member `gen` lives in \
         `tools/gen`",
    );
}

#[test]
fn a_package_under_crates_that_isnt_a_member_fails() {
    let project = Project::new();
    project.write("crates/extra/Cargo.toml", &member_manifest("extra", ""));
    project.write("crates/extra/src/lib.rs", "//! Extra.\n");
    project.edit_root_manifest(
        "members = [\"crates/*\"]",
        "members = [\"crates/demo\", \"crates/demo_core\"]",
    );
    project.lock();
    project.check().assert_reports(
        1,
        "error: [workspace_shape] crates/extra/Cargo.toml: This package isn't a workspace member",
    );
}

#[cfg(unix)]
#[test]
fn a_member_whose_crate_directory_is_a_symbolic_link_fails() {
    let project = Project::new();
    fs::create_dir_all(project.path().join("tools")).expect("tools/ should be created");
    fs::rename(
        project.path().join("crates/demo_core"),
        project.path().join("tools/demo_core"),
    )
    .expect("the crate should move");
    unix::fs::symlink(
        "../tools/demo_core",
        project.path().join("crates/demo_core"),
    )
    .expect("the link should be created");
    let run = project.check();
    run.assert_reports(
        1,
        "error: [workspace_shape] crates/demo_core: The workspace member `demo_core` lives in \
         `crates/demo_core`, which is a symbolic link",
    );
    run.assert_reports(
        1,
        "error: [member_sources] crates/demo_core: This is a symbolic link in or above the crate \
         `demo_core`",
    );
}

#[cfg(unix)]
#[test]
fn a_member_under_a_linked_crates_directory_fails() {
    let project = Project::new();
    fs::rename(project.path().join("crates"), project.path().join("tools"))
        .expect("crates/ should move");
    unix::fs::symlink("tools", project.path().join("crates")).expect("the link should be created");
    project.check().assert_reports(
        1,
        "error: [workspace_shape] crates: This is a symbolic link, so the crates' files sit \
         elsewhere",
    );
}
