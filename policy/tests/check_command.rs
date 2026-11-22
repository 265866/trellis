//! `trellis-policy check` on a small workspace shaped like a project generated from the trellis
//! template, which passes, and on copies of it that each break one rule.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::{env, fs};

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
        "[workspace]\nchangelog_update = false\n\n[[package]]\nname = \"demo\"\n\
         changelog_path = \"./CHANGELOG.md\"\nchangelog_update = true\n\
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

    /// Writes the lockfile, fetching git dependencies into the Cargo home `cargo_home`, so the
    /// fetch doesn't touch the user's.
    fn lock_fetching(&self, cargo_home: &Path) {
        let output = Command::new(env!("CARGO"))
            .arg("generate-lockfile")
            .current_dir(self.path())
            .env("CARGO_HOME", cargo_home)
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

    /// Makes `path` a git repository of its own with one commit holding the package `sub`, which
    /// `git add` in the project adds as a submodule.
    fn nested_repository(&self, path: &str) {
        self.write(&format!("{path}/Cargo.toml"), &member_manifest("sub", ""));
        self.write(
            &format!("{path}/src/lib.rs"),
            "//! A crate in a submodule.\n",
        );
        self.git(&["-C", path, "init", "--quiet"]);
        self.git(&["-C", path, "add", "--all"]);
        self.git(&[
            "-C",
            path,
            "commit",
            "--quiet",
            "--no-verify",
            "--message",
            "nested",
        ]);
    }

    /// Runs `trellis-policy` with `arguments` in the project.
    fn run(&self, arguments: &[&str]) -> Run {
        run(&self.path(), arguments, Ci::Local)
    }

    /// Runs `trellis-policy check --manifest-path <root>/Cargo.toml`.
    fn check(&self) -> Run {
        output(self.check_command())
    }

    /// Runs `trellis-policy check` as `check` does, with the Cargo home `cargo_home`, which holds
    /// the project's git dependencies.
    fn check_with_cargo_home(&self, cargo_home: &Path) -> Run {
        let mut command = self.check_command();
        command.env("CARGO_HOME", cargo_home);
        output(command)
    }

    /// The command `check` runs.
    fn check_command(&self) -> Command {
        let manifest = self.path().join("Cargo.toml");
        let manifest = manifest
            .to_str()
            .expect("the temporary path should be UTF-8");
        command(
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
    output(command(directory, arguments, ci))
}

/// The command with `arguments` in `directory`, under `ci`.
fn command(directory: &Path, arguments: &[&str], ci: Ci) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_trellis-policy"));
    command
        .args(arguments)
        .current_dir(directory)
        .env("CARGO", env!("CARGO"))
        .env_remove("GITHUB_ACTIONS");
    if ci == Ci::GitHubActions {
        command.env("GITHUB_ACTIONS", "true");
    }
    command
}

/// Runs `command`, and returns what it did.
fn output(mut command: Command) -> Run {
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

/// Makes `link` a symbolic link to `target`, which is absolute or relative to the link's
/// directory. `ln` and `mklink` make it, because the standard library has a different function
/// for it on each system, and a `cfg` here would keep one of them from the lints.
fn symbolic_link(target: impl AsRef<Path>, link: &Path) {
    let target: PathBuf = target.as_ref().components().collect();
    let link: PathBuf = link.components().collect();
    let directory = link.parent().expect("a link has a directory");
    let mut command = if env::consts::OS == "windows" {
        let mut command = Command::new("cmd");
        command.args(["/C", "mklink"]);
        if directory.join(&target).is_dir() {
            command.arg("/D");
        }
        command.arg(&link).arg(&target);
        command
    } else {
        let mut command = Command::new("ln");
        command.arg("-s").arg(&target).arg(&link);
        command
    };
    let output = command.output().expect("the link command should run");
    let is_a_link = fs::symlink_metadata(&link).is_ok_and(|metadata| metadata.is_symlink());
    let windows_needs = if env::consts::OS == "windows" {
        " Windows lets a user make one only with Developer Mode on (Settings > System > For \
         developers) or the \"Create symbolic links\" privilege; without either, this test can't \
         run here, so it fails rather than pass untested."
    } else {
        ""
    };
    assert!(
        output.status.success() && is_a_link,
        "{} should be a symbolic link.{windows_needs} {output:?}",
        link.display()
    );
}

#[test]
fn a_generated_project_passes() {
    let run = Project::new().check();
    run.assert_passes();
    run.assert_reports(0, "All 15 structure rules hold");
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

#[test]
fn a_lints_library_linked_into_the_workspace_fails() {
    let project = Project::new();
    project.lints_manifest("vendored", "trellis_lints");
    let link = project.directory.path().join("link");
    symbolic_link(project.path().join("vendored"), &link);
    project.root_manifest("", r#"[{ path = "../link" }]"#);
    project.check().assert_reports(
        1,
        "is inside the workspace, so a change to its sources counts as code",
    );
}

#[test]
fn a_symbolic_link_in_a_crate_fails() {
    let project = Project::new();
    symbolic_link(
        "../../../README.md",
        &project.path().join("crates/demo_core/src/extra.rs"),
    );
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
fn a_version_on_an_unpublished_path_dependency_fails() {
    let project = Project::new();
    project.write(
        "crates/demo/Cargo.toml",
        &member_manifest(
            "demo",
            "demo_core = { version = \"0.1.0\", path = \"../demo_core\" }\n",
        ),
    );
    project.lock();
    project.check().assert_reports(
        1,
        "error: [internal_dependency_versions] crates/demo/Cargo.toml: `[dependencies] demo_core` \
         gives the workspace crate `demo_core` a `version` beside its `path`.",
    );
    project.write(
        "crates/demo/Cargo.toml",
        &member_manifest("demo", "demo_core = { path = \"../demo_core\" }\n"),
    );
    project.lock();
    project.check().assert_passes();
}

#[test]
fn a_member_version_of_its_own_fails() {
    let project = Project::new();
    let own_version = member_manifest("demo_core", "")
        .replace("\nversion.workspace = true", "\nversion = \"0.1.0\"");
    project.write("crates/demo_core/Cargo.toml", &own_version);
    project.lock();
    project.check().assert_reports(
        1,
        "error: [member_manifests] crates/demo_core/Cargo.toml: `[package] version` is \"0.1.0\", \
         but it must be `version.workspace = true`, because the project shares one version",
    );
    project.write(
        "crates/demo_core/Cargo.toml",
        &member_manifest("demo_core", ""),
    );
    project.lock();
    project.check().assert_passes();
}

#[test]
fn a_crate_changelog_release_plz_would_write_fails() {
    let project = Project::new();
    project.write(
        "release-plz.toml",
        "[workspace]\n\n[[package]]\nname = \"demo\"\nchangelog_path = \"./CHANGELOG.md\"\n\
         changelog_update = true\nchangelog_include = [\"demo_core\"]\n",
    );
    project.check().assert_reports(
        1,
        "error: [release_changelog] release-plz.toml: release-plz would write the changelog of \
         `demo_core` to `crates/demo_core/CHANGELOG.md`, which is code",
    );
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

/// The `tsconfig.json` of a web UI in the crate `demo`, with the `extends` and `references`
/// lines `reaches`, and a comment and trailing commas, which TypeScript allows.
fn web_tsconfig(reaches: &str) -> String {
    format!(
        "{{\n    // The web UI.\n    {reaches}\n    \"compilerOptions\": {{ \"strict\": true, }},\n    \
         \"include\": [\"src\",],\n}}\n"
    )
}

#[test]
fn a_tsconfig_that_reaches_only_other_tsconfig_files_passes() {
    let project = Project::new();
    project.write(
        "crates/demo/web/tsconfig.json",
        &web_tsconfig(
            "\"extends\": [\"./strict/tsconfig.json\", \"../../demo_core/web/tsconfig.json\", \
             \"./tsconfig.base.json\"],\n    \
             \"references\": [{ \"path\": \"./tools/tsconfig.json\" }, { \"path\": \"./tools\" }, \
             { \"path\": \"../../demo_core/web/\" }],",
        ),
    );
    project.write("crates/demo/web/strict/tsconfig.json", &web_tsconfig(""));
    project.write("crates/demo/web/tools/tsconfig.json", &web_tsconfig(""));
    project.write("crates/demo/web/tsconfig.base.json", &web_tsconfig(""));
    project.write(
        "crates/demo_core/web/tsconfig.json",
        "\u{feff}{\r\n  // Windows line endings.\r\n  \"compilerOptions\": { \"strict\": true }\r\n}\r\n",
    );
    project.check().assert_passes();
}

/// Vite's starter splits the settings among `tsconfig.json`, `tsconfig.app.json` and
/// `tsconfig.node.json`.
#[test]
fn a_tsconfig_that_references_other_tsconfig_files_by_any_name_passes() {
    let project = Project::new();
    project.write(
        "crates/demo/web/tsconfig.json",
        "{\n  \"files\": [],\n  \"references\": [\n    { \"path\": \"./tsconfig.app.json\" },\n    \
         { \"path\": \"./TSConfig.Node.json\" }\n  ]\n}\n",
    );
    project.write(
        "crates/demo/web/tsconfig.app.json",
        &web_tsconfig("\"extends\": \"./tsconfig.base.json\","),
    );
    project.write("crates/demo/web/TSConfig.Node.json", &web_tsconfig(""));
    project.write("crates/demo/web/tsconfig.base.json", &web_tsconfig(""));
    project.check().assert_passes();
}

#[test]
fn a_tsconfig_that_reaches_another_file_fails() {
    for (reaches, problem) in [
        (
            "\"extends\": \"./loose.json\",",
            "`extends` names `./loose.json`",
        ),
        (
            "\"extends\": [\"./strict/tsconfig.json\", \"./jsconfig.json\"],",
            "`extends` names `./jsconfig.json`",
        ),
        (
            "\"extends\": \"@tsconfig/strictest/tsconfig.json\",",
            "`extends` names `@tsconfig/strictest/tsconfig.json`",
        ),
        (
            "\"extends\": \"tsconfig.base.json\",",
            "`extends` names `tsconfig.base.json`",
        ),
        (
            "\"extends\": \"./node_modules/strictest/tsconfig.json\",",
            "`extends` names `./node_modules/strictest/tsconfig.json`",
        ),
        (
            "\"extends\": \"./strict/../node_modules/strictest/tsconfig.json\",",
            "`extends` names `./strict/../node_modules/strictest/tsconfig.json`",
        ),
        (
            "\"extends\": \"./missing/tsconfig.json\",",
            "`extends` names `./missing/tsconfig.json`",
        ),
        (
            "\"extends\": \"../../../../tsconfig.json\",",
            "`extends` names `../../../../tsconfig.json`",
        ),
        ("\"extends\": \"./strict\",", "`extends` names `./strict`"),
        (
            "\"extends\": \"./strict\\\\tsconfig.json\",",
            "`extends` names `./strict\\tsconfig.json`",
        ),
        ("\"extends\": 1,", "`extends` is `1`"),
        (
            "\"references\": [{ \"path\": \"./src\" }],",
            "`references` names `./src`",
        ),
        (
            "\"references\": [{ \"path\": \"./loose.json\" }],",
            "`references` names `./loose.json`",
        ),
        (
            "\"references\": [{ \"path\": \"strict\" }],",
            "`references` names `strict`",
        ),
        (
            "\"references\": [\"./strict\"],",
            "`references` is `[\"./strict\"]`",
        ),
        (
            "\"extends\": ./strict/tsconfig.json,",
            "TypeScript could read this file otherwise than as JSON with comments",
        ),
    ] {
        let project = Project::new();
        project.write("crates/demo/web/tsconfig.json", &web_tsconfig(reaches));
        project.write("crates/demo/web/strict/tsconfig.json", &web_tsconfig(""));
        project.write("crates/demo/web/src/main.ts", "export {};\n");
        for loose in ["loose.json", "jsconfig.json"] {
            project.write(
                &format!("crates/demo/web/{loose}"),
                "{ \"compilerOptions\": { \"strict\": false } }\n",
            );
        }
        project.write(
            "crates/demo/web/node_modules/strictest/tsconfig.json",
            &web_tsconfig(""),
        );
        project.check().assert_reports(
            1,
            &format!("error: [config_files] crates/demo/web/tsconfig.json: {problem}"),
        );
    }
}

#[test]
fn a_tsconfig_file_of_any_name_that_reaches_another_file_fails() {
    let project = Project::new();
    project.write(
        "crates/demo/web/tsconfig.json",
        &web_tsconfig("\"extends\": \"./tsconfig.base.json\","),
    );
    project.write(
        "crates/demo/web/tsconfig.base.json",
        &web_tsconfig("\"extends\": \"./loose.json\","),
    );
    project.write(
        "crates/demo/web/loose.json",
        "{ \"compilerOptions\": { \"strict\": false } }\n",
    );
    project.check().assert_reports(
        1,
        "error: [config_files] crates/demo/web/tsconfig.base.json: `extends` names `./loose.json`",
    );
}

#[test]
fn a_tsconfig_file_that_is_a_symbolic_link_fails() {
    let project = Project::new();
    project.write(
        "web/tsconfig.json",
        &web_tsconfig("\"extends\": \"./tsconfig.base.json\","),
    );
    project.write(
        "crates/demo/web/loose.json",
        "{ \"compilerOptions\": { \"strict\": false } }\n",
    );
    symbolic_link(
        "../crates/demo/web/loose.json",
        &project.path().join("web/tsconfig.base.json"),
    );
    project.check().assert_reports(
        1,
        "error: [config_files] web/tsconfig.base.json: This is a symbolic link",
    );
}

/// TypeScript ends a `//` comment at a carriage return, a line separator (U+2028) or a paragraph
/// separator (U+2029) as well as at a line feed, and reads the rest of the line. It also reads a
/// file whose syntax it reports as wrong, so whatever could read two ways fails.
#[test]
fn a_tsconfig_typescript_could_read_otherwise_fails() {
    for (text, why) in [
        (
            "{\n    \"include\": [\"src\"] // see tsconfig.base.json\r, \"extends\": \"./loose.json\"\n}\n",
            "a comment that ends at a carriage return",
        ),
        (
            "{\n    \"include\": [\"src\"] // see\u{2028}, \"extends\": \"./loose.json\"\n}\n",
            "a comment that ends at a line separator",
        ),
        (
            "{\n    \"include\": [\"src\"] // see\u{2029}, \"extends\": \"./loose.json\"\n}\n",
            "a comment that ends at a paragraph separator",
        ),
        (
            "{\n    \"include\": [\"src\u{2028}\"]\n}\n",
            "a line separator in a string, which TypeScript ends there",
        ),
        (
            "{\n    \"extends\": \"./loose.json\",\n    \"extends\": \"./strict/tsconfig.json\"\n}\n",
            "a key given twice",
        ),
        (
            "{\n    \"references\": [{ \"path\": \"./loose.json\", \"path\": \"./strict\" }]\n}\n",
            "a key given twice in a reference",
        ),
        (
            "{\n    \"include\": [\"src\"]\n}\n/* never closed\n",
            "a block comment never closed",
        ),
        ("{ , }\n", "a comma with nothing before it"),
        (
            "{ \"include\": [ , ] }\n",
            "a comma with nothing before it in a list",
        ),
    ] {
        let project = Project::new();
        project.write("crates/demo/web/tsconfig.json", text);
        project.write("crates/demo/web/strict/tsconfig.json", &web_tsconfig(""));
        project.write(
            "crates/demo/web/loose.json",
            "{ \"compilerOptions\": { \"strict\": false } }\n",
        );
        let run = project.check();
        assert!(
            run.code == Some(1)
                && run.stdout.lines().any(|line| {
                    line.starts_with("error: [config_files] crates/demo/web/tsconfig.json: ")
                }),
            "{why} should fail: {run:#?}"
        );
    }
}

/// The `.gitattributes` every generated project gets.
const TEMPLATE_ATTRIBUTES: &str = include_str!("../../template/.gitattributes");

/// The start of the violation for line `line` of the root `.gitattributes` setting `attribute`.
fn sets(line: usize, attribute: &str) -> String {
    format!("error: [config_files] .gitattributes: Line {line} sets `{attribute}`, but only")
}

#[test]
fn the_template_gitattributes_passes_in_the_working_tree_and_in_the_index() {
    let project = Project::new();
    project.write(".gitattributes", TEMPLATE_ATTRIBUTES);
    project.check().assert_passes();
    project.git(&["init", "--quiet"]);
    project.commit("generate");
    project.check().assert_passes();
}

/// Each of these makes a checkout write other bytes than the blob, hides the diff, or changes what
/// a merge or an archive writes, except an attribute name in other letters, which git reads as
/// another attribute and which fails because only the listed spellings pass.
/// `working-tree-encoding=ISO-2022-JP-3` turns 13 characters in a comment of a `tsconfig.json`
/// into `*/,"noImplicitAny":false/*`.
#[test]
fn a_gitattributes_line_that_can_change_checked_out_bytes_fails() {
    for attribute in [
        "working-tree-encoding=ISO-2022-JP-3",
        "ident",
        "filter=lfs",
        "eol=crlf",
        "!eol",
        "crlf",
        "IDENT",
        "Text",
        "-diff",
        "binary",
        "linguist-generated",
        "export-subst",
        "merge=union",
    ] {
        let project = Project::new();
        project.write(
            ".gitattributes",
            &format!("* text=auto eol=lf\ncrates/demo/web/tsconfig.json {attribute}\n"),
        );
        project.check().assert_reports(1, &sets(2, attribute));
    }
}

#[test]
fn a_gitattributes_macro_fails_where_it_is_defined_and_where_it_is_set() {
    let project = Project::new();
    project.write(".gitattributes", "[attr]lf ident\n* text=auto lf\n");
    let run = project.check();
    run.assert_reports(
        1,
        "error: [config_files] .gitattributes: Line 1 defines the attribute macro `lf`",
    );
    run.assert_reports(1, &sets(2, "lf"));
}

/// Git reads a pattern in double quotes as a C string, with or without a blank after it, splits
/// the rest at blanks with no escapes, and reads `#` as a comment only at the start of a line.
#[test]
fn gitattributes_lines_are_split_as_git_splits_them() {
    for (text, attribute) in [
        ("\"docs/my notes.md\"ident\n", "ident"),
        ("\"docs/my notes.md\" text ident\n", "ident"),
        ("docs/my\\ notes.md text\n", "notes.md"),
        ("*.rs text # line endings\n", "#"),
    ] {
        let project = Project::new();
        project.write(".gitattributes", text);
        project.check().assert_reports(1, &sets(1, attribute));
    }
    let project = Project::new();
    project.write(
        ".gitattributes",
        "# A comment that names ident.\n  \"docs/my notes.md\"\ttext\n",
    );
    project.check().assert_passes();
}

#[test]
fn a_gitattributes_with_crlf_endings_or_a_byte_order_mark_reads_as_git_reads_it() {
    let project = Project::new();
    project.write(
        ".gitattributes",
        "\u{feff}# Line endings.\r\n* text=auto eol=lf\r\n",
    );
    project.check().assert_passes();
    project.write(".gitattributes", "\u{feff}* text=auto\r\n*.rs ident\r\n");
    project.check().assert_reports(1, &sets(2, "ident"));
}

/// A checkout reads the attributes from git's index, so a copy there that the working tree no
/// longer shows still fails.
#[test]
fn a_gitattributes_in_the_index_counts_as_well_as_the_working_tree() {
    let project = Project::new();
    project.git(&["init", "--quiet"]);
    project.write(".gitattributes", "* text=auto eol=lf\n*.rs ident\n");
    project.git(&["add", "--all"]);
    project.write(".gitattributes", TEMPLATE_ATTRIBUTES);
    let run = project.check();
    run.assert_reports(
        1,
        "error: [config_files] .gitattributes: In the copy in git's index, which a checkout \
         reads and `git add` replaces, line 2 sets `ident`",
    );
    assert_eq!(
        run.stdout.matches("sets `ident`").count(),
        1,
        "the line is reported once: {run:#?}"
    );
    project.write(".gitattributes", "* text=auto eol=lf\n*.rs ident\n");
    let run = project.check();
    run.assert_reports(1, &sets(2, "ident"));
    assert_eq!(
        run.stdout.matches("sets `ident`").count(),
        1,
        "a line the working tree and the index share is reported once: {run:#?}"
    );
}

#[test]
fn a_gitattributes_that_is_a_symbolic_link_fails() {
    let project = Project::new();
    project.write("attributes", TEMPLATE_ATTRIBUTES);
    fs::remove_file(project.path().join(".gitattributes"))
        .expect("the attributes file should be removed");
    symbolic_link("attributes", &project.path().join(".gitattributes"));
    project.check().assert_reports(
        1,
        "error: [config_files] .gitattributes: This is a symbolic link.",
    );
}

#[test]
fn a_stale_build_script_entry_fails() {
    let project = Project::new();
    project.policy_setting("build_script_crates = [\"demo\"]");
    project.check().assert_reports(
        1,
        "error: [build_scripts] Cargo.toml: `build_script_crates` in \
         `[workspace.metadata.policy]` lists `demo`, which has no build script any more.",
    );
}

#[test]
fn a_build_script_entry_that_names_no_member_passes() {
    let project = Project::new();
    project.policy_setting("build_script_crates = [\"demo_engine\"]");
    project.check().assert_passes();
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
        "[workspace]\nchangelog_update = false\n\n[[package]]\nname = \"demo\"\n\
         changelog_path = \"./CHANGELOG.md\"\nchangelog_update = true\n\
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
fn a_member_directory_outside_ascii_fails() {
    let project = Project::new();
    project.write(
        "crates/\u{e9}t\u{e9}/Cargo.toml",
        &member_manifest("ete", ""),
    );
    project.write("crates/\u{e9}t\u{e9}/src/lib.rs", "//! Summer.\n");
    project.lock();
    project.check().assert_reports(
        1,
        "error: [workspace_shape] crates/\u{e9}t\u{e9}: The workspace member `ete` lives in \
         `crates/\u{e9}t\u{e9}`, whose name holds a character outside ASCII.",
    );
}

/// `check` finds the members with `cargo metadata`, which reads the working tree, and Windows
/// can't hold these directories, so the test runs on Linux and macOS only.
#[cfg(unix)]
#[test]
fn a_member_directory_windows_reads_as_another_fails() {
    for name in ["extra.", "extra ", "ex:tra", "ex\\tra"] {
        let project = Project::new();
        project.write(
            &format!("crates/{name}/Cargo.toml"),
            &member_manifest("extra", ""),
        );
        project.write(&format!("crates/{name}/src/lib.rs"), "//! Extra.\n");
        project.lock();
        project.check().assert_reports(
            1,
            &format!(
                "error: [workspace_shape] crates/{name}: The workspace member `extra` lives in \
                 `crates/{name}`, whose name Windows reads as another name."
            ),
        );
    }
}

/// A crate's directory of the form of a Windows short name fails once, as any path of the form
/// does. Every system can hold the directory, since no longer name beside it has that short name.
#[test]
fn a_member_directory_of_the_windows_short_name_form_fails_once() {
    let project = Project::new();
    project.write("crates/extra~1/Cargo.toml", &member_manifest("extra", ""));
    project.write("crates/extra~1/src/lib.rs", "//! Extra.\n");
    project.lock();
    let run = project.check();
    run.assert_reports(
        1,
        "error: [workspace_shape] crates/extra~1: This name has the form of a Windows short name",
    );
    assert_eq!(
        run.stdout.matches("] crates/extra~1: ").count(),
        1,
        "the directory is reported once: {run:#?}"
    );
}

/// Git lists no file of a crate that `.gitignore` excludes, so the directory's name is read from
/// the entries of `crates/`.
#[test]
fn an_ignored_member_directory_of_the_windows_short_name_form_fails() {
    let project = Project::new();
    project.write("crates/extra~1/Cargo.toml", &member_manifest("extra", ""));
    project.write("crates/extra~1/src/lib.rs", "//! Extra.\n");
    project.lock();
    project.write(".gitignore", "/crates/extra~1/\n");
    project.git(&["init", "--quiet"]);
    project.git(&["add", "--all"]);
    project.check().assert_reports(
        1,
        "error: [workspace_shape] crates/extra~1: This name has the form of a Windows short name",
    );
}

/// Has git track `respelled`, another spelling of the file `tracked` in `project`, as it does
/// after a clone on any system. A file system that ignores case can hold one file for both, so
/// `respelled` is written with `tracked`'s contents and added with `core.ignorecase` off, which
/// makes git keep both names, and with `core.precomposeunicode` off, which keeps git on macOS from
/// rewriting a decomposed name.
fn track_respelled(project: &Project, tracked: &str, respelled: &str) {
    let contents =
        fs::read_to_string(project.path().join(tracked)).expect("the tracked file should be read");
    project.write(respelled, &contents);
    project.git(&[
        "-c",
        "core.ignorecase=false",
        "-c",
        "core.precomposeunicode=false",
        "update-index",
        "--add",
        "--",
        respelled,
    ]);
}

#[test]
fn paths_a_case_insensitive_file_system_reads_as_one_fail() {
    let project = Project::new();
    project.git(&["init", "--quiet"]);
    project.git(&["add", "--all"]);
    track_respelled(
        &project,
        "crates/demo_core/src/lib.rs",
        "crate\u{17f}/demo_core/src/lib.rs",
    );
    track_respelled(&project, "docs/install.md", "docs/INSTALL.md");
    let run = project.check();
    run.assert_reports(
        1,
        "error: [workspace_shape] crate\u{17f}: `crates` and `crate\u{17f}` are one path to a \
         file system that ignores case,",
    );
    run.assert_reports(
        1,
        "error: [workspace_shape] docs/install.md: `docs/INSTALL.md` and `docs/install.md` are \
         one path",
    );
    assert_eq!(
        run.stdout.matches("are one path").count(),
        2,
        "each collision is reported once, at its shortest path: {run:#?}"
    );
}

/// A file system whose case table maps the ypogegrammeni to the letter iota, and that doesn't
/// reorder marks, reads `a`, the ypogegrammeni and an acute accent as the same name as `a`, a
/// small iota and the accent. Canonical caseless folding keeps them apart, so the character fails
/// by itself. APFS and the one NTFS volume measured keep the two apart, so the test's file system
/// holds both.
#[test]
fn a_path_with_a_combining_ypogegrammeni_fails() {
    let project = Project::new();
    project.git(&["init", "--quiet"]);
    project.git(&["add", "--all"]);
    track_respelled(&project, "docs/install.md", "docs/a\u{345}\u{301}.md");
    track_respelled(&project, "docs/install.md", "docs/a\u{3b9}\u{301}.md");
    project.check().assert_reports(
        1,
        "error: [workspace_shape] docs/a\u{345}\u{301}.md: This path holds the combining Greek \
         ypogegrammeni (U+0345)",
    );
}

/// Windows gives `Cargo.toml` the short name `CARGO~1.TOM` and opens the file under either name,
/// in any case, so a tracked `cargo~1.tom` can replace the manifest's contents in a checkout
/// there. Where the test's directory has short names, the second spelling is the manifest
/// itself, which is why it is written with the manifest's contents. A directory of the form is
/// reported once, whatever it holds.
#[test]
fn a_path_of_the_windows_short_name_form_fails() {
    let project = Project::new();
    project.git(&["init", "--quiet"]);
    project.git(&["add", "--all"]);
    track_respelled(&project, "Cargo.toml", "cargo~1.tom");
    track_respelled(&project, "AGENTS.md", "notes~12/first.txt");
    track_respelled(&project, "AGENTS.md", "notes~12/second.txt");
    track_respelled(&project, "AGENTS.md", "notes~/third.txt");
    let run = project.check();
    run.assert_reports(
        1,
        "error: [workspace_shape] cargo~1.tom: This name has the form of a Windows short name",
    );
    run.assert_reports(
        1,
        "error: [workspace_shape] notes~12: This name has the form of a Windows short name",
    );
    assert_eq!(
        run.stdout.matches("Windows short name").count(),
        2,
        "each name of the form is reported once, and `notes~` isn't one: {run:#?}"
    );
}

/// Letters outside ASCII merge in either case, as `É` and `é` do on macOS and Windows, and an
/// accented letter written as one character merges with the letter and a combining accent on
/// macOS. A letter and the same letter with an accent stay apart.
#[test]
fn paths_that_differ_in_a_letter_outside_ascii_fail() {
    let project = Project::new();
    project.write("crates/demo_core/data/\u{e9}.txt", "accented\n");
    project.write("crates/demo_core/data/e.txt", "plain\n");
    project.write("crates/demo_core/src/\u{c9}.rs", "//! Seen by the lints.\n");
    project.write("\u{414}/x.txt", "x\n");
    project.write("docs/\u{3a3}.md", "# Sigma\n\nA page.\n");
    project.git(&["init", "--quiet"]);
    project.git(&["add", "--all"]);
    let pairs = [
        (
            "crates/demo_core/src/\u{c9}.rs",
            "crates/demo_core/src/\u{e9}.rs",
        ),
        ("docs/\u{3a3}.md", "docs/\u{3c3}.md"),
        ("\u{414}/x.txt", "\u{434}/x.txt"),
        (
            "crates/demo_core/data/\u{e9}.txt",
            "crates/demo_core/data/e\u{301}.txt",
        ),
    ];
    for (tracked, respelled) in pairs {
        track_respelled(&project, tracked, respelled);
    }
    let run = project.check();
    for reported in [
        "`crates/demo_core/src/\u{c9}.rs` and `crates/demo_core/src/\u{e9}.rs`",
        "`docs/\u{3a3}.md` and `docs/\u{3c3}.md`",
        "`\u{414}` and `\u{434}`",
        "`crates/demo_core/data/e\u{301}.txt` and `crates/demo_core/data/\u{e9}.txt`",
    ] {
        run.assert_reports(1, &format!("{reported} are one path"));
    }
    assert_eq!(
        run.stdout.matches("are one path").count(),
        4,
        "`e.txt` stays apart from `\u{e9}.txt`: {run:#?}"
    );
    run.assert_reports(
        1,
        "as its code point: `crates/demo_core/data/e\\u{301}.txt` and \
         `crates/demo_core/data/\\u{e9}.txt`.",
    );
    assert_eq!(
        run.stdout.matches("look the same").count(),
        1,
        "only the pair that looks the same is spelled out: {run:#?}"
    );
}

/// Where the file system keeps case, `crates/DEMO_CORE` is a second member beside
/// `crates/demo_core`. Where it ignores case, as on macOS and Windows, it can't hold both, so git
/// tracks the second spelling of the manifest, as after a clone of a repository that holds both.
#[test]
fn member_directories_that_differ_only_in_case_fail() {
    let project = Project::new();
    let ignores_case = fs::exists(project.path().join("crates/DEMO_CORE"))
        .expect("the file system should say whether the crate is there");
    if ignores_case {
        project.git(&["init", "--quiet"]);
        project.git(&["add", "--all"]);
        track_respelled(
            &project,
            "crates/demo_core/Cargo.toml",
            "crates/DEMO_CORE/Cargo.toml",
        );
    } else {
        project.write(
            "crates/DEMO_CORE/Cargo.toml",
            &member_manifest("shouting", ""),
        );
        project.write("crates/DEMO_CORE/src/lib.rs", "//! The core, louder.\n");
        project.lock();
    }
    project.check().assert_reports(
        1,
        "error: [workspace_shape] crates/demo_core: `crates/DEMO_CORE` and `crates/demo_core` \
         are one path to a file system that ignores case",
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

#[test]
fn a_member_whose_crate_directory_is_a_symbolic_link_fails() {
    let project = Project::new();
    fs::create_dir_all(project.path().join("tools")).expect("tools/ should be created");
    fs::rename(
        project.path().join("crates/demo_core"),
        project.path().join("tools/demo_core"),
    )
    .expect("the crate should move");
    symbolic_link(
        "../tools/demo_core",
        &project.path().join("crates/demo_core"),
    );
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

#[test]
fn a_member_under_a_linked_crates_directory_fails() {
    let project = Project::new();
    fs::rename(project.path().join("crates"), project.path().join("tools"))
        .expect("crates/ should move");
    symbolic_link("tools", &project.path().join("crates"));
    project.check().assert_reports(
        1,
        "error: [workspace_shape] crates: This is a symbolic link, so the crates' files sit \
         elsewhere",
    );
}

/// A repository outside `crates/` fails too. A checkout that initializes a submodule at
/// `.config/sub` or `tools/sub` holds files no check read, though a diff shows only its commit.
#[test]
fn a_git_repository_anywhere_fails() {
    for path in [
        "crates/demo_core/vendor/sub",
        "crates/sub",
        "tools/sub",
        ".config/sub",
        "docs/sub",
    ] {
        let project = Project::new();
        project.git(&["init", "--quiet"]);
        project.nested_repository(path);
        project.lock();
        let violation = format!(
            "error: [member_sources] {path}: This is another git repository, such as a \
             submodule, so the checks can't see its files"
        );
        project.check().assert_reports(1, &violation);
        project.commit("add a submodule");
        project.check().assert_reports(1, &violation);
    }
}

/// Git doesn't list a repository that `.gitignore` excludes, and a plain `git add` doesn't add
/// it, so no pull request carries it. Added by force it is a submodule like any other.
#[test]
fn a_git_repository_that_is_ignored_passes_until_it_is_tracked() {
    let project = Project::new();
    project.write(".gitignore", "/scratch/\n");
    project.git(&["init", "--quiet"]);
    project.nested_repository("scratch/sub");
    project.check().assert_passes();
    project.git(&["add", "--force", "scratch/sub"]);
    project.check().assert_reports(
        1,
        "error: [member_sources] scratch/sub: This is another git repository, such as a \
         submodule, so the checks can't see its files",
    );
}

/// A deploy kit's scripts that shellcheck passes, a unit file and a script known only by its `#!`
/// line pass.
#[test]
fn a_deploy_kit_that_shellcheck_passes_passes() {
    let project = Project::new();
    project.write(
        "deploy/common.sh",
        "#!/usr/bin/env bash\n# Shared settings.\nexport APP_DIR=/srv/app\n",
    );
    project.write(
        "deploy/install.sh",
        "#!/usr/bin/env bash\nset -euo pipefail\n# shellcheck source=common.sh\n\
         . \"$(dirname \"$0\")/common.sh\"\nmkdir -p \"$APP_DIR\"\n",
    );
    project.write("deploy/update", "#!/bin/sh\nset -eu\necho \"updating\"\n");
    project.write(
        "deploy/app.container",
        "[Container]\nImage=ghcr.io/owner/app:1\nExec=sh -c $unquoted\n",
    );
    project.check().assert_passes();
}

/// Each script under `deploy/` that shellcheck faults is reported at its file, whether it is
/// known by its name or by its `#!` line, and only there.
#[test]
fn a_deploy_script_that_shellcheck_faults_fails() {
    let project = Project::new();
    project.write(
        "deploy/backup.sh",
        "#!/usr/bin/env bash\nrm -rf $DATA_DIR/old\n",
    );
    project.write("deploy/restore", "#!/bin/sh\ncd $1\n");
    project.write("tools/loose.sh", "#!/bin/sh\ncd $1\n");
    let run = project.check();
    run.assert_reports(
        1,
        "error: [deploy_scripts] deploy/backup.sh: shellcheck reports",
    );
    run.assert_reports(1, "[SC2086]");
    run.assert_reports(
        1,
        "error: [deploy_scripts] deploy/restore: shellcheck reports",
    );
    assert!(
        !run.stdout.contains("tools/loose.sh"),
        "only deploy/ is checked: {run:#?}"
    );
}

/// A `.shellcheckrc`, in the kit or at the root, can't turn a check off; a directive beside the
/// line can, where review sees it.
#[test]
fn a_shellcheckrc_cant_silence_a_deploy_script() {
    let project = Project::new();
    project.write(".shellcheckrc", "disable=all\n");
    project.write("deploy/.shellcheckrc", "disable=all\n");
    project.write("deploy/backup.sh", "#!/bin/sh\nrm -rf $1/old\n");
    project.check().assert_reports(
        1,
        "error: [deploy_scripts] deploy/backup.sh: shellcheck reports",
    );
    project.write(
        "deploy/backup.sh",
        "#!/bin/sh\nset -eu\n# shellcheck disable=SC2086 # The argument is one word by contract.\n\
         rm -rf $1/old\n",
    );
    project.check().assert_passes();
}

/// A directive that turns every check off, or one before the first command, which covers the
/// whole script, fails, and so does `SHELLCHECK_OPTS`'s attempt to exclude a check.
#[test]
fn a_deploy_script_cant_turn_shellcheck_off() {
    let project = Project::new();
    project.write(
        "deploy/backup.sh",
        "#!/bin/sh\n# shellcheck disable=all\nrm -rf $1/old\n",
    );
    project.check().assert_reports(
        1,
        "error: [deploy_scripts] deploy/backup.sh: Line 2 disables `all`",
    );
    project.write(
        "deploy/backup.sh",
        "#!/bin/sh\n# shellcheck disable=SC2086 # Covers the script.\nrm -rf $1/old\nrm -rf $2/new\n",
    );
    project.check().assert_reports(
        1,
        "error: [deploy_scripts] deploy/backup.sh: A shellcheck directive hides SC2086 at line 4, \
         but one right above line 4 would silence it there alone",
    );
    project.write("deploy/backup.sh", "#!/bin/sh\nset -eu\nrm -rf $1/old\n");
    let output = Command::new(env!("CARGO_BIN_EXE_trellis-policy"))
        .arg("check")
        .current_dir(project.path())
        .env("CARGO", env!("CARGO"))
        .env("SHELLCHECK_OPTS", "--exclude=SC2086 --severity=error")
        .env_remove("GITHUB_ACTIONS")
        .output()
        .expect("trellis-policy should run");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(1), "{stdout}");
    assert!(stdout.contains("[SC2086]"), "{stdout}");
}

/// A symbolic link in a deploy kit fails, since review read the file it points at nowhere.
#[test]
fn a_symbolic_link_in_a_deploy_kit_fails() {
    let project = Project::new();
    project.write("scripts/install.sh", "#!/bin/sh\necho installed\n");
    project.write("deploy/README.md", "The kit.\n");
    symbolic_link(
        "../scripts/install.sh",
        &project.path().join("deploy/install.sh"),
    );
    project.check().assert_reports(
        1,
        "error: [deploy_scripts] deploy/install.sh: This is a symbolic link",
    );
}

/// Without shellcheck the check can't run, so the run fails rather than pass the kit unchecked,
/// and a project with no deploy kit doesn't need it.
#[test]
fn a_deploy_kit_without_shellcheck_fails() {
    let project = Project::new();
    let empty = TempDir::new().expect("the temporary directory should be created");
    let sysroot = Command::new("rustc")
        .args(["--print", "sysroot"])
        .output()
        .expect("rustc should run");
    let sysroot = String::from_utf8_lossy(&sysroot.stdout);
    let rustc = Path::new(sysroot.trim())
        .join("bin")
        .join(format!("rustc{}", env::consts::EXE_SUFFIX));
    let check = |project: &Project| {
        let output = Command::new(env!("CARGO_BIN_EXE_trellis-policy"))
            .arg("check")
            .current_dir(project.path())
            .env("CARGO", env!("CARGO"))
            .env("PATH", empty.path())
            .env("RUSTC", &rustc)
            .env_remove("GITHUB_ACTIONS")
            .output()
            .expect("trellis-policy should run");
        (
            output.status.code(),
            String::from_utf8_lossy(&output.stdout).into_owned(),
        )
    };
    let (code, stdout) = check(&project);
    assert_eq!(code, Some(0), "no kit needs no shellcheck: {stdout}");
    project.write("deploy/install.sh", "#!/bin/sh\necho installed\n");
    let (code, stdout) = check(&project);
    assert_eq!(code, Some(2), "{stdout}");
    assert!(
        stdout.contains("[deploy_scripts] the check couldn't run")
            && stdout.contains("shellcheck isn't installed"),
        "{stdout}"
    );
}

/// A file a deploy script sources is shellchecked too, though no name or `#!` line marks it as a
/// script, and its findings and directives are reported at it.
#[test]
fn a_file_a_deploy_script_sources_is_checked() {
    let project = Project::new();
    project.write(
        "deploy/install.sh",
        "#!/bin/sh\nset -eu\n# shellcheck source=lib/common\n. \"$(dirname \"$0\")/lib/common\"\n\
         clean old\n",
    );
    project.write("deploy/lib/common", "clean() {\n  rm -rf $1\n}\n");
    project.check().assert_reports(
        1,
        "error: [deploy_scripts] deploy/lib/common: shellcheck reports, at line:column 2:10",
    );
    project.write(
        "deploy/lib/common",
        "# shellcheck disable=all\nclean() {\n  rm -rf $1\n}\n",
    );
    project.check().assert_reports(
        1,
        "error: [deploy_scripts] deploy/lib/common: Line 1 disables `all`",
    );
}

/// A directive that covers more than the line below it, the way shellcheck applies one to the
/// whole next command, fails at the finding it hides: across a lone `\` line, over a block that
/// opens mid-line, and over a sourced file.
#[test]
fn a_directive_that_hides_more_than_its_line_fails() {
    for (script, hidden_in, line) in [
        (
            "#!/bin/sh\n\\\n# shellcheck disable=SC2086 # r\nset -eu\nrm $1\n",
            "deploy/run.sh",
            5,
        ),
        (
            "#!/bin/sh\nset -eu\n# shellcheck disable=SC2086 # r\ntrue && {\n  rm $1\n}\n",
            "deploy/run.sh",
            5,
        ),
        (
            "#!/bin/sh\nset -eu\n# shellcheck disable=SC2086 # r\nls | while read -r f; do\n  rm $f\ndone\n",
            "deploy/run.sh",
            5,
        ),
        (
            "#!/bin/sh\nset -eu\n# shellcheck disable=SC2086 # r\n. ./app.env\n",
            "deploy/app.env",
            1,
        ),
    ] {
        let project = Project::new();
        project.write("deploy/run.sh", script);
        project.write("deploy/app.env", "rm -rf $1\n");
        project.check().assert_reports(
            1,
            &format!(
                "error: [deploy_scripts] {hidden_in}: A shellcheck directive hides SC2086 at line \
                 {line}"
            ),
        );
    }
}

/// A file outside the kit that a script sources isn't read, so the script must say so.
#[test]
fn a_deploy_script_cant_source_a_file_outside_the_kit_unseen() {
    let project = Project::new();
    project.write("tools/lib.sh", "# shellcheck disable=all\nrm -rf $1\n");
    project.write(
        "deploy/run.sh",
        "#!/bin/sh\nset -eu\n. \"$(dirname \"$0\")/../tools/lib.sh\"\n",
    );
    project.check().assert_reports(
        1,
        "error: [deploy_scripts] deploy/run.sh: shellcheck reports",
    );
}

/// Files in a deploy kit that are no shell scripts pass, though they have neither an extension
/// nor a `#!` line, and a library known only by its `shell=` directive is checked on its own.
#[test]
fn a_deploy_kit_s_other_files_pass_and_a_library_with_a_shell_directive_is_checked() {
    let project = Project::new();
    project.write(
        "deploy/Containerfile",
        "FROM scratch\nRUN echo $unquoted && [ -d x ]\n",
    );
    project.write(
        "deploy/Caddyfile",
        "example.com {\n\treverse_proxy :8080\n}\n",
    );
    project.write("deploy/Makefile", "install:\n\tcp $(BIN) /usr/local/bin\n");
    project.write("deploy/justfile", "install:\n    cp {{bin}} ~/.local/bin\n");
    project.write(
        "deploy/sudoers.d/app",
        "app ALL=(root) NOPASSWD: /bin/systemctl restart app\n",
    );
    project.write(
        "deploy/app.service",
        "[Service]\n# shellcheck is run in CI on the scripts beside this file.\nExecStart=/srv/app\n",
    );
    project.check().assert_passes();
    project.write(
        "deploy/functions.inc",
        "# shellcheck shell=sh\nclean() {\n  rm -rf $1\n}\n",
    );
    project.check().assert_reports(
        1,
        "error: [deploy_scripts] deploy/functions.inc: shellcheck reports, at line:column 3:10",
    );
}

/// A directive right above a command covers the lines that command continues onto: after a
/// pipe, `||` or `&&`, inside a quoted string, and a `case` statement's pattern lines, where
/// shellcheck reads no directive. A `<<` in a comment or a shift opens no heredoc.
#[test]
fn a_directive_above_a_command_covers_the_lines_it_continues_onto() {
    let project = Project::new();
    project.write(
        "deploy/run.sh",
        "#!/bin/sh\nset -eu\n# shellcheck disable=SC2086 # r\ncurl $1 |\n  tar -C $2 -x\n\
         # shellcheck disable=SC2086 # r\nfalse ||\n  rm $3\n\
         # shellcheck disable=SC2086 # r\necho \"x\n$(rm $4)\"\n\
         # shellcheck disable=SC2254 # r\ncase $1 in\n  $2) echo \"$3\" ;;\n  *) : ;;\nesac\n\
         x=$((1<<4)) # <<EOF\n# shellcheck disable=SC2086 # r\necho $x\n",
    );
    project.check().assert_passes();
}

/// A finding in a host file a deploy script sources by its absolute path isn't the kit's, so it
/// isn't reported, and no directive in the kit could silence it.
#[cfg(unix)]
#[test]
fn a_host_file_a_deploy_script_sources_isnt_reported() {
    let host = TempDir::new().expect("the temporary directory should be created");
    let library = host.path().join("os-release");
    fs::write(&library, "NAME=demo\nrm -rf $1\n").expect("the host file should be written");
    let project = Project::new();
    project.write(
        "deploy/run.sh",
        &format!(
            "#!/bin/sh\nset -eu\n. {}\necho \"$NAME\"\n",
            library.display()
        ),
    );
    project.check().assert_passes();
}

/// A directive passes when shellcheck takes none closer to the finding it hides: above a loop
/// whose closing line holds the finding, above a multi-line array, above a `case` whose pattern
/// line holds it, and above a pipeline that continues after `done`.
#[test]
fn a_directive_where_shellcheck_takes_none_closer_passes() {
    let project = Project::new();
    project.write(
        "deploy/run.sh",
        "#!/bin/bash\nset -eu\n# shellcheck disable=SC2086 # r\nwhile read -r line; do\n  echo \"$line\"\ndone < $1\n\
         # shellcheck disable=SC2206 # r\nargs=(\n  --foo $2\n)\necho \"${args[@]}\"\n\
         # shellcheck disable=SC2254 # r\ncase $1 in\n  $2) echo \"$3\" ;;\n  *) : ;;\nesac\n\
         # shellcheck disable=SC2086 # r\nfor f in a b; do\n  echo \"$f\"\ndone |\n  sort $4\n",
    );
    project.check().assert_passes();
}

/// A directive fails when a closer one would silence the finding alone: above a function whose
/// body holds it, and before the first command, which covers the whole file, above a command
/// whose quoted text holds an apostrophe, where a lexer could lose track of the quotes.
#[test]
fn a_directive_where_shellcheck_takes_a_closer_one_fails() {
    for (script, line) in [
        (
            "#!/bin/sh\nset -eu\n# shellcheck disable=SC2086 # r\nclean() {\n  echo done\n  rm $1\n}\n",
            6,
        ),
        (
            "#!/bin/bash\n# shellcheck disable=SC2086,SC2116 # r\nmsg=\"$(echo \"can't connect\" $1)\"\n\
             echo \"$msg\"\nrm $2\n",
            5,
        ),
    ] {
        let project = Project::new();
        project.write("deploy/run.sh", script);
        project.check().assert_reports(
            1,
            &format!(
                "error: [deploy_scripts] deploy/run.sh: A shellcheck directive hides SC2086 at line \
                 {line}"
            ),
        );
    }
}

/// A kit file a script sources by its absolute path in the checkout fails, since shellcheck then
/// reads the checkout, with the file's directives as they are, instead of the kit's copy.
#[cfg(unix)]
#[test]
fn a_kit_file_sourced_by_its_absolute_path_fails() {
    let project = Project::new();
    project.write(
        "deploy/lib/common",
        "# shellcheck disable=SC2086 # r\nclean() {\n  rm -rf $1\n}\n",
    );
    let library = project.path().join("deploy/lib/common");
    project.write(
        "deploy/run.sh",
        &format!("#!/bin/sh\nset -eu\n. {}\nclean old\n", library.display()),
    );
    project.check().assert_reports(
        1,
        "error: [deploy_scripts] deploy/run.sh: This file sources a kit file by its absolute path \
         in the checkout",
    );
    project.write(
        "deploy/run.sh",
        &format!(
            "#!/bin/sh\nset -eu\n. {}\nclean old\n",
            library.display().to_string().replacen('/', "//", 2)
        ),
    );
    project.check().assert_reports(
        1,
        "error: [deploy_scripts] deploy/run.sh: This file sources a kit file by its absolute path",
    );
}

/// A unit file, a README or a script that only runs a kit file by its path in the checkout passes,
/// since shellcheck reads none of those paths.
#[cfg(unix)]
#[test]
fn naming_a_kit_file_in_the_checkout_without_sourcing_it_passes() {
    let project = Project::new();
    let helper = project.path().join("deploy/helper.sh");
    project.write("deploy/helper.sh", "#!/bin/sh\necho helped\n");
    project.write(
        "deploy/app.service",
        &format!("[Service]\nExecStart={}\n", helper.display()),
    );
    project.write(
        "deploy/README.md",
        &format!("Run `{}` once.\n", helper.display()),
    );
    project.write(
        "deploy/run.sh",
        &format!("#!/bin/sh\nset -eu\n{} --once\n", helper.display()),
    );
    project.check().assert_passes();
}

/// A directive that hides a finding where a closer one would silence it fails though another
/// directive for its code, or a heredoc line that reads like one, sits between them.
#[test]
fn a_directive_fails_past_another_for_its_code() {
    for inner in [
        "  # shellcheck disable=SC2086 # r\n  echo $line\n",
        "  cat <<EOF\n# shellcheck disable=SC2086 # in the generated file\nEOF\n",
    ] {
        let project = Project::new();
        project.write(
            "deploy/run.sh",
            &format!(
                "#!/bin/bash\n# shellcheck disable=SC2086 # r\nset -eu\nwhile read -r line; do\n\
                 {inner}done < $1\n"
            ),
        );
        project
            .check()
            .assert_reports(1, "but one right above line 4 would silence it there alone");
    }
}

/// A probe that breaks the parse doesn't count as a closer place, though the broken parse also
/// drops other findings: a directive above an `if` whose `[[ ]]` spans lines passes beside
/// sourced files shellcheck can't find.
#[test]
fn a_probe_that_breaks_the_parse_is_no_closer_place() {
    let project = Project::new();
    project.write(
        "deploy/run.sh",
        "#!/bin/bash\nset -eu\n\
         # shellcheck disable=SC1091 # r\n. /nonexistent/a.conf\n\
         # shellcheck disable=SC1091 # r\n. /nonexistent/b.conf\n\
         # shellcheck disable=SC1091 # r\n. /nonexistent/c.conf\n\
         a=${1:-}\nb=${2:-}\n\
         # shellcheck disable=SC2053 # r\nif [[ -n $a &&\n      $a == $b ]]; then\n  echo same\nfi\n",
    );
    project.check().assert_passes();
}

/// A directive above a command that a `\` continues covers its continued line, while a trailing
/// comment that ends in `\` continues nothing, so a closer directive there still counts.
#[test]
fn a_backslash_continues_a_command_but_not_a_comment() {
    let project = Project::new();
    project.write(
        "deploy/run.sh",
        "#!/bin/bash\nset -eu\n# shellcheck disable=SC2086 # r\ncp a \\\n  $1\n",
    );
    project.check().assert_passes();
    project.write(
        "deploy/run.sh",
        "#!/bin/bash\n# shellcheck disable=SC2086 # whole function\nf() {\n  y=1\t# note \\\n  rm $1\n}\n",
    );
    project
        .check()
        .assert_reports(1, "but one right above line 5 would silence it there alone");
}

/// A kit file sourced by its path in the checkout fails in every place a command starts, and a
/// README that shows such a line passes, since shellcheck doesn't read it.
#[cfg(unix)]
#[test]
fn a_checkout_path_sourced_where_a_command_starts_fails() {
    let project = Project::new();
    project.write("deploy/common", "clean() {\n  rm -rf \"$1\"\n}\n");
    let library = project.path().join("deploy/common");
    let library = library.display();
    project.write("deploy/README.md", &format!("Run:\n\n    . {library}\n"));
    project.check().assert_passes();
    for line in [
        format!(". {library};"),
        format!("if . \"{library}\"; then clean old; fi"),
        format!("{{ . {library}; }}"),
    ] {
        project.write("deploy/run.sh", &format!("#!/bin/sh\nset -eu\n{line}\n"));
        project.check().assert_reports(
            1,
            "error: [deploy_scripts] deploy/run.sh: This file sources a kit file by its absolute \
             path in the checkout",
        );
    }
}

/// Writes the members `demo_ui`, a UI toolkit stand-in, and `demo_widgets`, which depends on it,
/// and lets every member depend on the others as `internal_dependencies` asks, with the line
/// `restricted` added to the policy table. `demo_core` gets the `[dependencies]` entries
/// `core_dependencies`.
fn restricted_project(core_dependencies: &str, restricted: &str) -> Project {
    restricted_workspace(
        "demo_ui = { path = \"../demo_ui\" }\n",
        core_dependencies,
        restricted,
    )
}

/// The project of `restricted_project`, with `demo_widgets` given the `[dependencies]` entries
/// `widgets_dependencies` instead of its normal dependency on `demo_ui`.
fn restricted_workspace(
    widgets_dependencies: &str,
    core_dependencies: &str,
    restricted: &str,
) -> Project {
    let project = Project::new();
    project.write("crates/demo_ui/Cargo.toml", &member_manifest("demo_ui", ""));
    project.write("crates/demo_ui/src/lib.rs", "//! The UI toolkit.\n");
    project.write(
        "crates/demo_widgets/Cargo.toml",
        &member_manifest("demo_widgets", widgets_dependencies),
    );
    project.write("crates/demo_widgets/src/lib.rs", "//! Widgets.\n");
    project.write(
        "crates/demo_core/Cargo.toml",
        &member_manifest("demo_core", core_dependencies),
    );
    project.edit_root_manifest(
        "demo = [\"demo_core\"]\ndemo_core = []\n",
        "demo = [\"demo_core\", \"demo_widgets\"]\n\
         demo_core = [\"demo_widgets\"]\ndemo_ui = []\ndemo_widgets = [\"demo_ui\"]\n",
    );
    project.write(
        "crates/demo/Cargo.toml",
        &member_manifest(
            "demo",
            "demo_core.workspace = true\ndemo_widgets = { path = \"../demo_widgets\" }\n",
        ),
    );
    project.write(
        "release-plz.toml",
        "[workspace]\nchangelog_update = false\n\n[[package]]\nname = \"demo\"\n\
         changelog_path = \"./CHANGELOG.md\"\nchangelog_update = true\n\
         changelog_include = [\"demo_core\", \"demo_ui\", \"demo_widgets\"]\n",
    );
    project.policy_setting(restricted);
    project.lock();
    project
}

/// `demo_core`'s dependency on `demo_widgets`, which brings `demo_ui`.
const CORE_USES_WIDGETS: &str = "demo_widgets = { path = \"../demo_widgets\" }\n";

/// The `restricted_crates` violations of `run`, one line each.
fn restricted_violations(run: &Run) -> Vec<&str> {
    run.stdout
        .lines()
        .filter(|line| line.starts_with("error: [restricted_crates] "))
        .collect()
}

/// A member that reaches a restricted package through another member fails, at its manifest,
/// with the path; the members its entry lists, and the package itself, pass.
#[test]
fn a_member_that_reaches_a_restricted_crate_fails() {
    let restricted = "restricted_crates = { \"demo_ui\" = [\"demo\", \"demo_widgets\"] }";
    restricted_project("", restricted).check().assert_passes();
    restricted_project(CORE_USES_WIDGETS, restricted)
        .check()
        .assert_reports(
            1,
            "error: [restricted_crates] crates/demo_core/Cargo.toml: `demo_core` reaches \
             `demo_ui` (demo_core -> demo_widgets -> demo_ui), which `restricted_crates` lets \
             only `demo`, `demo_widgets` reach, through its key `demo_ui`. Move the code that \
             needs it into one of those crates, or drop the dependency that brings it.",
        );
}

/// Without the setting, a member may reach any package, and the check says nothing.
#[test]
fn without_restricted_crates_nothing_is_restricted() {
    let run = restricted_project(CORE_USES_WIDGETS, "").check();
    run.assert_passes();
    assert!(
        !run.stdout.contains("restricted_crates") && !run.stderr.contains("restricted_crates"),
        "a project without the setting should get no `restricted_crates` output: {run:#?}"
    );
}

/// A name or prefix key matches whatever the case of the name or key, with `-` and `_` read as
/// one, as crates.io and Cargo read them.
#[test]
fn a_name_key_matches_in_any_case_with_dash_and_underscore_as_one() {
    for key in ["demo-ui", "DEMO_UI", "Demo-Ui", "demo-u*", "DEMO_U*"] {
        let run = restricted_project(
            CORE_USES_WIDGETS,
            &format!("restricted_crates = {{ \"{key}\" = [\"demo\", \"demo_widgets\"] }}"),
        )
        .check();
        let violations = restricted_violations(&run);
        assert!(
            run.code == Some(1)
                && violations.iter().any(|line| line.contains(
                    "`demo_core` reaches `demo_ui` (demo_core -> demo_widgets -> demo_ui)"
                )),
            "the key `{key}` should match `demo_ui`: {run:#?}"
        );
    }
}

/// A member's own dev-dependencies count, but its dependencies' dev-dependencies aren't built for
/// it, so they don't.
#[test]
fn only_a_member_s_own_dev_dependencies_count() {
    let run = restricted_workspace(
        "\n[dev-dependencies]\ndemo_ui = { path = \"../demo_ui\" }\n",
        CORE_USES_WIDGETS,
        "restricted_crates = { \"demo_ui\" = [\"demo\"] }",
    )
    .check();
    assert_eq!(
        restricted_violations(&run),
        [
            "error: [restricted_crates] crates/demo_widgets/Cargo.toml: `demo_widgets` reaches \
             `demo_ui` (demo_widgets -> demo_ui), which `restricted_crates` lets only `demo` \
             reach, through its key `demo_ui`. Move the code that needs it into one of those \
             crates, or drop the dependency that brings it."
        ],
        "only `demo_widgets`, whose own dev-dependency brings `demo_ui`, should fail: {run:#?}"
    );
}

/// A build dependency, and a dependency for another platform than the one the check runs on,
/// count.
#[test]
fn build_and_other_platform_dependencies_count() {
    for (dependencies, kind) in [
        (
            "\n[build-dependencies]\ndemo_widgets = { path = \"../demo_widgets\" }\n",
            "a build dependency",
        ),
        (
            "\n[target.'cfg(windows)'.dependencies]\ndemo_widgets = { path = \"../demo_widgets\" }\n",
            "a Windows dependency",
        ),
        (
            "\n[target.'cfg(not(windows))'.dependencies]\ndemo_widgets = { path = \"../demo_widgets\" }\n",
            "a dependency for every system but Windows",
        ),
    ] {
        let run = restricted_project(
            dependencies,
            "restricted_crates = { \"demo_ui\" = [\"demo\", \"demo_widgets\"] }",
        )
        .check();
        let violations = restricted_violations(&run);
        assert!(
            run.code == Some(1)
                && violations.len() == 1
                && violations[0].contains("`demo_core` reaches `demo_ui`"),
            "{kind} should count: {run:#?}"
        );
    }
}

/// An empty list lets no member reach the package, so moving the code elsewhere in the workspace
/// can't help.
#[test]
fn an_empty_list_lets_no_member_reach_the_package() {
    let run = restricted_project("", "restricted_crates = { \"demo_ui\" = [] }").check();
    assert_eq!(
        restricted_violations(&run),
        [
            "error: [restricted_crates] crates/demo/Cargo.toml: `demo` reaches `demo_ui` \
             (demo -> demo_widgets -> demo_ui), which `restricted_crates` lets no member reach, \
             through its key `demo_ui`. Drop the dependency that brings it.",
            "error: [restricted_crates] crates/demo_widgets/Cargo.toml: `demo_widgets` reaches \
             `demo_ui` (demo_widgets -> demo_ui), which `restricted_crates` lets no member reach, \
             through its key `demo_ui`. Drop the dependency that brings it.",
        ],
        "every member that reaches `demo_ui` should fail: {run:#?}"
    );
}

/// A member that a key restricts is checked like any other, so it fails when it reaches another
/// restricted package its list leaves it out of.
#[test]
fn a_restricted_member_that_reaches_another_restricted_package_fails() {
    let run = restricted_project(
        "",
        "restricted_crates = { \"demo_widgets\" = [\"demo\"], \"demo_ui\" = [\"demo\"] }",
    )
    .check();
    assert_eq!(
        restricted_violations(&run),
        [
            "error: [restricted_crates] crates/demo_widgets/Cargo.toml: `demo_widgets` reaches \
             `demo_ui` (demo_widgets -> demo_ui), which `restricted_crates` lets only `demo` \
             reach, through its key `demo_ui`. Move the code that needs it into one of those \
             crates, or drop the dependency that brings it."
        ],
        "`demo_widgets` should fail for `demo_ui`: {run:#?}"
    );
}

/// Keys that match the same package give one violation per member, which names each key that
/// leaves the member out and only the members every matching key lists.
#[test]
fn overlapping_keys_report_one_violation_per_package() {
    let run = restricted_project(
        CORE_USES_WIDGETS,
        "restricted_crates = { \"demo_ui\" = [\"demo\", \"demo_widgets\"], \
         \"demo-u*\" = [\"demo_widgets\"], \"DEMO-UI\" = [\"demo\", \"demo_widgets\"] }",
    )
    .check();
    assert_eq!(
        restricted_violations(&run),
        [
            "error: [restricted_crates] crates/demo/Cargo.toml: `demo` reaches `demo_ui` \
             (demo -> demo_widgets -> demo_ui), which `restricted_crates` lets only \
             `demo_widgets` reach, through its key `demo-u*`. Move the code that needs it into \
             one of those crates, or drop the dependency that brings it.",
            "error: [restricted_crates] crates/demo_core/Cargo.toml: `demo_core` reaches \
             `demo_ui` (demo_core -> demo_widgets -> demo_ui), which `restricted_crates` lets \
             only `demo_widgets` reach, through its keys `DEMO-UI`, `demo-u*`, `demo_ui`. Move \
             the code that needs it into one of those crates, or drop the dependency that brings \
             it.",
        ],
        "each member should fail once: {run:#?}"
    );
}

/// A key that matches no package in the graph, on any platform and with every feature, is
/// misspelled or stale, so it fails at the root manifest rather than restricting nothing.
#[test]
fn a_key_that_matches_no_package_fails() {
    for key in ["demo_uii", "zed-*", "git+https://github.com/example/demo"] {
        let run = restricted_project(
            "",
            &format!(
                "restricted_crates = {{ \"{key}\" = [\"demo\"], \"demo_ui\" = [\"demo\", \
                 \"demo_widgets\"] }}"
            ),
        )
        .check();
        assert_eq!(
            restricted_violations(&run),
            [format!(
                "error: [restricted_crates] Cargo.toml: `restricted_crates` has a key `{key}` \
                 that matches no package in the dependency graph, on any platform with every \
                 workspace feature on, though it lists crates that may reach one. Fix the \
                 spelling, or remove the key."
            )],
            "the key `{key}` should fail: {run:#?}"
        );
    }
}

/// A key with an empty list bans its package from every crate, so it passes while no crate
/// depends on the package yet, and fails the first crate that does.
#[test]
fn a_ban_on_a_package_no_crate_depends_on_yet_passes() {
    let restricted =
        "restricted_crates = { \"absent\" = [], \"demo_ui\" = [\"demo\", \"demo_widgets\"] }";
    restricted_project("", restricted).check().assert_passes();
    let run = restricted_project("", "restricted_crates = { \"demo_ui\" = [] }").check();
    run.assert_reports(1, "which `restricted_crates` lets no member reach");
}

/// A key that names no package, a list that names no member, and a value of the wrong shape fail
/// as malformed settings.
#[test]
fn a_restricted_crates_setting_that_names_nothing_fails() {
    restricted_project("", "restricted_crates = { \"demo*ui\" = [\"demo\"] }")
        .check()
        .assert_reports(2, "has a key `demo*ui` that names no package");
    restricted_project(
        "",
        "restricted_crates = { \"demo_ui\" = [\"demo_widget\"] }",
    )
    .check()
    .assert_reports(
        2,
        "names `demo_widget`, which isn't a key of `internal_dependencies`",
    );
    restricted_project("", "restricted_crates = [\"demo_ui\"]")
        .check()
        .assert_reports(
            2,
            "`restricted_crates` must be a table from package names, name prefixes ending in \
             `*` or sources to arrays of member names",
        );
    restricted_project("", "restricted_crates = { \"demo_ui\" = \"demo\" }")
        .check()
        .assert_reports(
            2,
            "`restricted_crates.\"demo_ui\"` must be an array of member names",
        );
}

/// The `file:` URL of `path`, an absolute path, with `/` separators on every system.
fn file_url(path: &Path) -> String {
    let path = path.to_str().expect("the temporary path should be UTF-8");
    format!(
        "file:///{}",
        path.replace('\\', "/").trim_start_matches('/')
    )
}

/// A source key matches a package from the same repository however the key writes its URL: in
/// other letters, with `.git`, a trailing `/`, a query or a fragment. A key for another
/// repository whose URL starts with this one's, or for another kind of source, matches nothing.
#[test]
fn a_source_key_matches_the_repository_however_it_is_written() {
    let project = restricted_workspace("", CORE_USES_WIDGETS, "");
    project.write(
        "../upstream/Cargo.toml",
        "[package]\nname = \"demo_remote\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n\
         [workspace]\n",
    );
    project.write("../upstream/src/lib.rs", "//! A crate from git.\n");
    project.git(&["-C", "../upstream", "init", "--quiet"]);
    project.git(&["-C", "../upstream", "add", "--all"]);
    project.git(&[
        "-C",
        "../upstream",
        "commit",
        "--quiet",
        "--no-verify",
        "--message",
        "upstream",
    ]);
    let upstream = file_url(&project.directory.path().join("upstream"));
    project.write(
        "crates/demo_widgets/Cargo.toml",
        &member_manifest(
            "demo_widgets",
            &format!("demo_remote = {{ git = \"{upstream}\" }}\n"),
        ),
    );
    let cargo_home = project.directory.path().join("cargo-home");
    project.lock_fetching(&cargo_home);
    let shouted = upstream.to_uppercase();
    for key in [
        format!("git+{upstream}"),
        format!("git+{shouted}"),
        format!("git+{upstream}.git/"),
        format!("git+{upstream}/?branch=main"),
        format!("git+{upstream}#0123abc"),
    ] {
        project.remove_policy_setting("restricted_crates");
        project.policy_setting(&format!(
            "restricted_crates = {{ \"{key}\" = [\"demo\", \"demo_widgets\"] }}"
        ));
        let run = project.check_with_cargo_home(&cargo_home);
        assert_eq!(
            restricted_violations(&run).len(),
            1,
            "the key `{key}` should match `demo_remote` once: {run:#?}"
        );
        run.assert_reports(
            1,
            "`demo_core` reaches `demo_remote` (demo_core -> demo_widgets -> demo_remote)",
        );
    }
    for key in [
        format!("git+{upstream}-fork"),
        format!("git+{upstream}2"),
        format!("registry+{upstream}"),
        format!("sparse+{upstream}"),
    ] {
        project.remove_policy_setting("restricted_crates");
        project.policy_setting(&format!(
            "restricted_crates = {{ \"{key}\" = [\"demo\", \"demo_widgets\"] }}"
        ));
        project
            .check_with_cargo_home(&cargo_home)
            .assert_reports(1, &format!("has a key `{key}` that matches no package"));
    }
}

/// A crate's deploy kit, under `crates/<crate>/deploy/`, is checked as the root `deploy/` is: a
/// kit that shellcheck passes passes, and each script it faults is reported at its path in the
/// crate, whether it is known by its name or by its `#!` line.
#[test]
fn a_crate_s_deploy_kit_is_checked_like_the_root_one() {
    let project = Project::new();
    project.write(
        "crates/demo/deploy/common.sh",
        "#!/usr/bin/env bash\n# Shared settings.\nexport APP_DIR=/srv/app\n",
    );
    project.write(
        "crates/demo/deploy/install.sh",
        "#!/usr/bin/env bash\nset -euo pipefail\n# shellcheck source=common.sh\n\
         . \"$(dirname \"$0\")/common.sh\"\nmkdir -p \"$APP_DIR\"\n",
    );
    project.write(
        "crates/demo/deploy/demo.service",
        "[Service]\nExecStart=/srv/app/demo\n",
    );
    project.check().assert_passes();
    project.write(
        "crates/demo/deploy/backup.sh",
        "#!/usr/bin/env bash\nrm -rf $DATA_DIR/old\n",
    );
    project.write("crates/demo/deploy/restore", "#!/bin/sh\ncd $1\n");
    project.write("crates/demo/scripts/loose.sh", "#!/bin/sh\ncd $1\n");
    let run = project.check();
    run.assert_reports(
        1,
        "error: [deploy_scripts] crates/demo/deploy/backup.sh: shellcheck reports, at \
         line:column 2:8",
    );
    run.assert_reports(
        1,
        "error: [deploy_scripts] crates/demo/deploy/restore: shellcheck reports",
    );
    assert!(
        !run.stdout.contains("loose.sh"),
        "only the crate's deploy/ is checked: {run:#?}"
    );
}

/// A crate kit's directives follow the root kit's rules, and a directive that hides more than its
/// line fails there too.
#[test]
fn a_crate_kit_s_directives_are_checked() {
    let project = Project::new();
    project.write(
        "crates/demo/deploy/backup.sh",
        "#!/bin/sh\n# shellcheck disable=all\nrm -rf $1/old\n",
    );
    project.check().assert_reports(
        1,
        "error: [deploy_scripts] crates/demo/deploy/backup.sh: Line 2 disables `all`",
    );
    project.write(
        "crates/demo/deploy/backup.sh",
        "#!/bin/sh\n# shellcheck disable=SC2086 # Covers the script.\nrm -rf $1/old\nrm -rf $2/new\n",
    );
    project.check().assert_reports(
        1,
        "error: [deploy_scripts] crates/demo/deploy/backup.sh: A shellcheck directive hides \
         SC2086 at line 4, but one right above line 4 would silence it there alone",
    );
    project.write(
        "crates/demo/deploy/backup.sh",
        "#!/bin/sh\nset -eu\n# shellcheck disable=SC2086 # One word by contract.\nrm -rf $1/old\n",
    );
    project.check().assert_passes();
}

/// A crate kit holds only its own files: a script that sources the crate's other files or the root
/// kit's is faulted, since shellcheck can't read them, and a `source=` that points there fails.
#[test]
fn a_crate_kit_can_t_source_outside_itself() {
    let project = Project::new();
    project.write("crates/demo/scripts/setup.sh", "rm -rf $1\n");
    project.write("deploy/common.sh", "#!/bin/sh\nexport APP_DIR=/srv/app\n");
    for (target, directive) in [
        ("../scripts/setup.sh", "source=../scripts/setup.sh"),
        (
            "../../../deploy/common.sh",
            "source=../../../deploy/common.sh",
        ),
    ] {
        project.write(
            "crates/demo/deploy/run.sh",
            &format!("#!/bin/sh\nset -eu\n. \"$(dirname \"$0\")/{target}\"\n"),
        );
        let run = project.check();
        run.assert_reports(
            1,
            "error: [deploy_scripts] crates/demo/deploy/run.sh: shellcheck reports",
        );
        run.assert_reports(1, "[SC1091]");
        project.write(
            "crates/demo/deploy/run.sh",
            &format!(
                "#!/bin/sh\nset -eu\n# shellcheck {directive}\n. \"$(dirname \"$0\")/{target}\"\n"
            ),
        );
        project.check().assert_reports(
            1,
            &format!(
                "error: [deploy_scripts] crates/demo/deploy/run.sh: Line 3 points shellcheck at \
                 `{}`, which isn't a file in `crates/demo/deploy/`",
                directive.trim_start_matches("source=")
            ),
        );
    }
}

/// A kit file a crate kit's script sources by its absolute path in the checkout fails, as in the
/// root kit, and so does a symbolic link in a crate kit.
#[cfg(unix)]
#[test]
fn a_crate_kit_s_checkout_paths_and_links_fail() {
    let project = Project::new();
    project.write(
        "crates/demo/deploy/lib/common",
        "clean() {\n  rm -rf \"$1\"\n}\n",
    );
    let library = project.path().join("crates/demo/deploy/lib/common");
    project.write(
        "crates/demo/deploy/run.sh",
        &format!("#!/bin/sh\nset -eu\n. {}\nclean old\n", library.display()),
    );
    project.check().assert_reports(
        1,
        "error: [deploy_scripts] crates/demo/deploy/run.sh: This file sources a kit file by its \
         absolute path in the checkout",
    );
    project.write("crates/demo/deploy/run.sh", "#!/bin/sh\necho run\n");
    project.write("scripts/install.sh", "#!/bin/sh\necho installed\n");
    symbolic_link(
        "../../../scripts/install.sh",
        &project.path().join("crates/demo/deploy/install.sh"),
    );
    project.check().assert_reports(
        1,
        "error: [deploy_scripts] crates/demo/deploy/install.sh: This is a symbolic link",
    );
}

/// A project with a kit in both places checks each on its own: each one's findings are reported at
/// its own files, a file of the same name in each is its own kit's, and each passes once fixed.
#[test]
fn a_project_with_both_kits_checks_each() {
    let project = Project::new();
    project.write("deploy/install.sh", "#!/bin/sh\ncd $1\n");
    project.write("deploy/lib.sh", "#!/bin/sh\nexport APP_DIR=/srv/app\n");
    project.write(
        "crates/demo/deploy/install.sh",
        "#!/bin/sh\nset -eu\n. \"$(dirname \"$0\")/lib.sh\"\necho \"$APP_DIR\"\n",
    );
    let run = project.check();
    run.assert_reports(
        1,
        "error: [deploy_scripts] deploy/install.sh: shellcheck reports, at line:column 2:4",
    );
    run.assert_reports(
        1,
        "error: [deploy_scripts] crates/demo/deploy/install.sh: shellcheck reports, at \
         line:column 3:3",
    );
    assert!(
        !run.stdout
            .contains("crates/demo/deploy/install.sh: shellcheck reports, at line:column 2:4"),
        "the root kit's finding isn't the crate kit's: {run:#?}"
    );
    project.write("deploy/install.sh", "#!/bin/sh\ncd \"$1\" || exit 1\n");
    project.write(
        "crates/demo/deploy/lib.sh",
        "#!/bin/sh\nexport APP_DIR=/srv/demo\n",
    );
    project.check().assert_passes();
}
