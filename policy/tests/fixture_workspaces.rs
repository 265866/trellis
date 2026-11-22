//! The structure checks on small workspaces written to temporary directories, where a test can
//! break each rule on purpose.

use std::ffi::OsString;
use std::path::Path;
use std::process::Command;
use std::{env, fs};

use tempfile::TempDir;
use trellis_policy::{
    Violation, Workspace, agent_files, build_scripts, config_files, docs_layout,
    internal_dependencies, internal_dependency_versions, license_files, proc_macros,
    release_changelog,
};

/// The manifest of the member `app`, which depends on the member `derive` only for its tests.
const APP_MANIFEST: &str = "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n\
                            [dev-dependencies]\nderive = { path = \"../derive\" }\n";

/// The manifest of the member `derive`, a proc-macro crate.
const DERIVE_MANIFEST: &str = "[package]\nname = \"derive\"\nversion = \"0.1.0\"\n\
                               edition = \"2024\"\n\n[lib]\nproc-macro = true\n";

/// A workspace in a temporary directory, deleted on drop, with the members `app` and `derive`.
struct Fixture {
    /// The workspace's directory.
    directory: TempDir,
}

impl Fixture {
    /// A workspace whose `[workspace.metadata.policy]` table is `policy`, with a lockfile, so
    /// `Workspace::load` can run `cargo metadata --locked` on it.
    fn new(policy: &str) -> Self {
        let fixture = Self {
            directory: TempDir::new().expect("the temporary directory should be created"),
        };
        fixture.write(
            "Cargo.toml",
            &format!(
                "[workspace]\nresolver = \"3\"\nmembers = [\"crates/*\"]\n\n\
                 [workspace.metadata.policy]\n{policy}"
            ),
        );
        fixture.write("crates/app/Cargo.toml", APP_MANIFEST);
        fixture.write("crates/app/src/lib.rs", "//! The app.\n");
        fixture.write("crates/derive/Cargo.toml", DERIVE_MANIFEST);
        fixture.write("crates/derive/src/lib.rs", "//! A proc macro.\n");
        fixture.lock();
        fixture
    }

    /// Adds the member `name` with the `[package]` name `name` and the manifest sections
    /// `sections` after it, then updates the lockfile.
    fn member(&self, name: &str, sections: &str) {
        self.write(
            &format!("crates/{name}/Cargo.toml"),
            &format!(
                "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n\
                 {sections}"
            ),
        );
        self.write(&format!("crates/{name}/src/lib.rs"), "//! A member.\n");
        self.lock();
    }

    /// Writes the lockfile, so `Workspace::load` can run `cargo metadata --locked`.
    fn lock(&self) {
        let cargo = env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"));
        let output = Command::new(cargo)
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

    /// The workspace's directory.
    fn path(&self) -> &Path {
        self.directory.path()
    }

    /// Writes `contents` to `path` in the workspace.
    fn write(&self, path: &str, contents: &str) {
        let path = self.path().join(path);
        let directory = path
            .parent()
            .expect("a file in the workspace has a directory");
        fs::create_dir_all(directory).expect("the directory should be created");
        fs::write(path, contents).expect("the file should be written");
    }

    /// The workspace, as the checks read it.
    fn workspace(&self) -> Workspace {
        Workspace::load(Some(&self.path().join("Cargo.toml")))
            .unwrap_or_else(|error| panic!("the checks should read the workspace, but {error}"))
    }
}

/// The files that `violations` name.
fn files(violations: &[Violation]) -> Vec<&str> {
    violations.iter().map(Violation::file).collect()
}

/// Whether a message in `violations` contains `text`.
fn mentions(violations: &[Violation], text: &str) -> bool {
    violations
        .iter()
        .any(|violation| violation.message().contains(text))
}

#[test]
fn an_unlisted_proc_macro_dependency_fails() {
    let fixture = Fixture::new("proc_macro_crates = []\n");
    let violations = proc_macros::check(&fixture.workspace());
    assert_eq!(
        files(&violations),
        ["crates/app/Cargo.toml"],
        "a dev dependency on a proc macro counts"
    );
    assert!(
        mentions(&violations, "the proc-macro crate `derive`"),
        "the message should name the proc macro: {violations:?}"
    );
}

#[test]
fn a_listed_proc_macro_dependency_passes_and_an_unused_entry_is_allowed() {
    let fixture = Fixture::new("proc_macro_crates = [\"derive\", \"not_yet_used\"]\n");
    let violations = proc_macros::check(&fixture.workspace());
    assert!(
        violations.is_empty(),
        "a listed crate passes, and an entry no member uses yet lets the rule change land before \
         the code: {violations:?}"
    );
}

#[test]
fn a_build_script_crate_entry_without_a_build_script_fails() {
    let fixture = Fixture::new("build_script_crates = [\"app\", \"not_a_member\"]\n");
    let violations =
        build_scripts::check(&fixture.workspace()).expect("the members should be readable");
    assert_eq!(
        files(&violations),
        ["Cargo.toml"],
        "only the member's stale entry fails: {violations:?}"
    );
    assert!(
        mentions(
            &violations,
            "lists `app`, which has no build script any more"
        ),
        "a listed member without a build script fails: {violations:?}"
    );
    assert!(
        !mentions(&violations, "not_a_member"),
        "a listed name that isn't a member lets a package rename land in three pull requests: \
         {violations:?}"
    );
}

#[test]
fn a_listed_build_script_passes_and_an_unlisted_one_fails() {
    let fixture = Fixture::new("build_script_crates = [\"app\"]\n");
    fixture.write("crates/app/build.rs", "fn main() {}\n");
    fixture.write("crates/derive/build.rs", "fn main() {}\n");
    let violations =
        build_scripts::check(&fixture.workspace()).expect("the members should be readable");
    assert_eq!(
        files(&violations),
        ["crates/derive/build.rs"],
        "the listed crate may have a build script, and an unlisted one still fails"
    );
}

#[test]
fn a_build_key_that_leaves_the_crate_fails() {
    let fixture = Fixture::new("build_script_crates = [\"app\"]\n");
    for (path, leaves) in [
        ("make.rs", false),
        ("scripts/../make.rs", false),
        ("../../tools/make.rs", true),
        ("scripts/../../make.rs", true),
        ("/tmp/make.rs", true),
    ] {
        fixture.write(
            "crates/app/Cargo.toml",
            &APP_MANIFEST.replace(
                "edition = \"2024\"\n",
                &format!("edition = \"2024\"\nbuild = \"{path}\"\n"),
            ),
        );
        let violations =
            build_scripts::check(&fixture.workspace()).expect("the members should be readable");
        assert_eq!(
            mentions(&violations, "outside the crate's directory"),
            leaves,
            "`build = {path:?}` on a listed crate: {violations:?}"
        );
    }
}

#[test]
fn a_build_key_outside_ascii_fails() {
    let fixture = Fixture::new("build_script_crates = [\"app\"]\n");
    for (path, is_ascii) in [
        ("scripts/make.rs", true),
        ("scripts/\u{c9}t\u{e9}.rs", false),
    ] {
        fixture.write(
            "crates/app/Cargo.toml",
            &APP_MANIFEST.replace(
                "edition = \"2024\"\n",
                &format!("edition = \"2024\"\nbuild = \"{path}\"\n"),
            ),
        );
        let violations =
            build_scripts::check(&fixture.workspace()).expect("the members should be readable");
        assert_eq!(
            mentions(&violations, "holds a character outside ASCII"),
            !is_ascii,
            "`build = \"{path}\"` on a listed crate: {violations:?}"
        );
    }
}

#[test]
fn a_build_key_windows_reads_as_another_path_fails() {
    let fixture = Fixture::new("build_script_crates = [\"app\"]\n");
    for (path, is_another_path) in [
        ("scripts/make.rs", false),
        ("scripts/.hidden/make.rs", false),
        ("scripts\\\\x/make.rs", true),
        ("scripts./make.rs", true),
        ("scripts /make.rs", true),
        ("CUSTOM~1.RS", true),
        ("script~1/make.rs", true),
        ("scripts/make~.rs", false),
    ] {
        fixture.write(
            "crates/app/Cargo.toml",
            &APP_MANIFEST.replace(
                "edition = \"2024\"\n",
                &format!("edition = \"2024\"\nbuild = \"{path}\"\n"),
            ),
        );
        let violations =
            build_scripts::check(&fixture.workspace()).expect("the members should be readable");
        assert_eq!(
            mentions(&violations, "names different files on different systems"),
            is_another_path,
            "`build = \"{path}\"` on a listed crate: {violations:?}"
        );
    }
}

#[test]
fn a_build_key_alone_counts_as_a_build_script() {
    let fixture = Fixture::new("build_script_crates = [\"app\"]\n");
    fixture.write(
        "crates/app/Cargo.toml",
        &APP_MANIFEST.replace(
            "edition = \"2024\"\n",
            "edition = \"2024\"\nbuild = \"make.rs\"\n",
        ),
    );
    let violations =
        build_scripts::check(&fixture.workspace()).expect("the members should be readable");
    assert!(
        violations.is_empty(),
        "a listed crate whose `build` key names its build script keeps its entry: {violations:?}"
    );
    fixture.write(
        "crates/app/Cargo.toml",
        &APP_MANIFEST.replace(
            "edition = \"2024\"\n",
            "edition = \"2024\"\nbuild = false\n",
        ),
    );
    let violations =
        build_scripts::check(&fixture.workspace()).expect("the members should be readable");
    assert!(
        mentions(
            &violations,
            "lists `app`, which has no build script any more"
        ),
        "`build = false` is no build script, so the entry is stale: {violations:?}"
    );
}

#[test]
fn rule_files_in_another_case_fail() {
    let fixture = Fixture::new("");
    fixture.write("crates/app/Clippy.toml", "disallowed-methods = []\n");
    fixture.write("crates/app/Build.rs", "fn main() {}\n");
    let workspace = fixture.workspace();

    let violations = config_files::check(&workspace).expect("the files should be listed");
    assert_eq!(
        files(&violations),
        ["crates/app/Clippy.toml"],
        "clippy reads `Clippy.toml` on a case-insensitive file system"
    );
    let violations = build_scripts::check(&workspace).expect("the members should be readable");
    assert_eq!(
        files(&violations),
        ["crates/app/Build.rs"],
        "Cargo runs `Build.rs` on a case-insensitive file system"
    );
}

/// The tool configuration files a trellis project keeps at its root.
const KEPT_CONFIG_FILES: [&str; 14] = [
    "rustfmt.toml",
    "clippy.toml",
    "deny.toml",
    "_typos.toml",
    ".taplo.toml",
    "dylint.toml",
    "rust-toolchain.toml",
    ".config/nextest.toml",
    "release-plz.toml",
    "renovate.json",
    "lychee.toml",
    ".cargo/config.toml",
    ".editorconfig",
    ".gitattributes",
];

#[test]
fn the_kept_config_files_pass_at_the_root_and_fail_anywhere_else() {
    let fixture = Fixture::new("");
    for file in KEPT_CONFIG_FILES {
        fixture.write(file, "");
    }
    let violations = config_files::check(&fixture.workspace()).expect("the files should be listed");
    assert!(
        violations.is_empty(),
        "no root config file is reported: {violations:?}"
    );

    for file in KEPT_CONFIG_FILES {
        fixture.write(&format!("crates/app/{file}"), "");
    }
    let violations = config_files::check(&fixture.workspace()).expect("the files should be listed");
    let mut expected: Vec<String> = KEPT_CONFIG_FILES
        .iter()
        .map(|file| format!("crates/app/{file}"))
        .collect();
    expected.sort();
    assert_eq!(
        files(&violations),
        expected,
        "a second copy of each kept file in a crate is reported, and the root ones still aren't"
    );
}

#[test]
fn agent_files_other_than_the_root_agents_md_fail() {
    let fixture = Fixture::new("");
    for path in [
        "AGENTS.md",
        "CLAUDE.md",
        "crates/app/AGENTS.md",
        "crates/app/.cursorrules",
        ".claude/settings.json",
        ".github/copilot-instructions.md",
    ] {
        fixture.write(path, "Instructions.\n");
    }
    let violations = agent_files::check(&fixture.workspace()).expect("the files should be listed");
    assert_eq!(
        files(&violations),
        [
            ".claude/settings.json",
            ".github/copilot-instructions.md",
            "CLAUDE.md",
            "crates/app/.cursorrules",
            "crates/app/AGENTS.md",
        ],
        "every agent file but the root `AGENTS.md` fails"
    );
    assert!(
        mentions(&violations, "the root `AGENTS.md`"),
        "the message should point at the root `AGENTS.md`: {violations:?}"
    );
}

/// The violations of `internal_dependencies::check`, sorted by file.
fn internal_violations(fixture: &Fixture) -> Vec<Violation> {
    let mut violations = internal_dependencies::check(&fixture.workspace());
    violations.sort_by(|left, right| left.file().cmp(right.file()));
    violations
}

#[test]
fn internal_dependencies_the_map_lists_pass_and_absent_crates_are_ignored() {
    let fixture = Fixture::new(
        "\n[workspace.metadata.policy.internal_dependencies]\napp = [\"derive\"]\nderive = []\n\
         planned = [\"derive\"]\n",
    );
    let violations = internal_violations(&fixture);
    assert!(
        violations.is_empty(),
        "an allowed edge passes, and a mapped crate that isn't a member yet doesn't fail: \
         {violations:?}"
    );
}

#[test]
fn an_internal_dependency_the_map_doesnt_list_fails() {
    let fixture = Fixture::new(
        "\n[workspace.metadata.policy.internal_dependencies]\napp = []\nderive = []\n",
    );
    let violations = internal_violations(&fixture);
    assert_eq!(
        files(&violations),
        ["crates/app/Cargo.toml"],
        "a dev-dependency on another member counts as an edge"
    );
    assert!(
        mentions(&violations, "depends on the workspace crate `derive`"),
        "the message should name the forbidden edge: {violations:?}"
    );
}

#[test]
fn a_member_the_map_doesnt_list_fails() {
    let fixture =
        Fixture::new("\n[workspace.metadata.policy.internal_dependencies]\nderive = []\n");
    let violations = internal_violations(&fixture);
    assert_eq!(
        files(&violations),
        ["crates/app/Cargo.toml"],
        "a new crate needs an entry, which is a rule change"
    );
    assert!(
        mentions(
            &violations,
            "`app` is a workspace member that `internal_dependencies`"
        ),
        "the message should name the unmapped member: {violations:?}"
    );
}

/// A violation a test expects.
#[derive(Debug)]
struct Expected {
    /// The file the violation names.
    file: &'static str,
    /// Text the violation's message contains.
    message: &'static str,
}

/// Asserts that `violations`, in order, are exactly `expected`, so a message reported against the
/// wrong file fails.
fn assert_violations(violations: &[Violation], expected: &[Expected], why: &str) {
    assert_eq!(
        violations.len(),
        expected.len(),
        "{why}: expected {expected:?}, got {violations:?}"
    );
    for (violation, expected) in violations.iter().zip(expected) {
        assert!(
            violation.file() == expected.file && violation.message().contains(expected.message),
            "{why}: expected {expected:?}, got {violation:?}"
        );
    }
}

#[test]
fn a_dev_only_crate_is_allowed_to_every_member_only_as_a_dev_dependency() {
    let fixture = Fixture::new(
        "dev_only_crates = [\"testing\"]\n\n\
         [workspace.metadata.policy.internal_dependencies]\napp = [\"derive\"]\nderive = []\n\
         core = []\ntesting = [\"core\"]\ndev_user = []\nnormal_user = []\n\
         dev_and_normal = []\ndev_and_build = []\n",
    );
    fixture.member("core", "");
    fixture.member("testing", "[dependencies]\ncore = { path = \"../core\" }\n");
    fixture.member(
        "dev_user",
        "[dev-dependencies]\ntesting = { path = \"../testing\" }\n",
    );
    fixture.member(
        "normal_user",
        "[dependencies]\ntesting = { path = \"../testing\" }\n",
    );
    fixture.member(
        "dev_and_normal",
        "[dependencies]\ntesting = { path = \"../testing\" }\n\n\
         [dev-dependencies]\ntesting = { path = \"../testing\" }\n",
    );
    fixture.write("crates/dev_and_build/build.rs", "fn main() {}\n");
    fixture.member(
        "dev_and_build",
        "[build-dependencies]\ntesting = { path = \"../testing\" }\n\n\
         [dev-dependencies]\ntesting = { path = \"../testing\" }\n",
    );
    let violations = internal_violations(&fixture);
    assert_violations(
        &violations,
        &[
            Expected {
                file: "crates/dev_and_build/Cargo.toml",
                message: "`dev_and_build` depends on the test-support crate `testing` as more \
                          than a dev-dependency",
            },
            Expected {
                file: "crates/dev_and_normal/Cargo.toml",
                message: "`dev_and_normal` depends on the test-support crate `testing` as more \
                          than a dev-dependency",
            },
            Expected {
                file: "crates/normal_user/Cargo.toml",
                message: "`normal_user` depends on the test-support crate `testing` as more than \
                          a dev-dependency",
            },
        ],
        "`dev_user` uses `testing` only in tests and passes, though its entry lists nothing; a \
         normal or build dependency on `testing` fails, even alongside a dev-dependency",
    );
}

#[test]
fn forbidden_build_and_target_specific_dependencies_fail() {
    let fixture = Fixture::new(
        "\n[workspace.metadata.policy.internal_dependencies]\napp = [\"derive\"]\nderive = []\n\
         core = []\nbuild_user = []\ntarget_user = []\n",
    );
    fixture.member("core", "");
    fixture.write("crates/build_user/build.rs", "fn main() {}\n");
    fixture.member(
        "build_user",
        "[build-dependencies]\ncore = { path = \"../core\" }\n",
    );
    fixture.member(
        "target_user",
        "[target.'cfg(target_os = \"haiku\")'.dependencies]\ncore = { path = \"../core\" }\n",
    );
    let violations = internal_violations(&fixture);
    assert_violations(
        &violations,
        &[
            Expected {
                file: "crates/build_user/Cargo.toml",
                message: "`build_user` depends on the workspace crate `core`",
            },
            Expected {
                file: "crates/target_user/Cargo.toml",
                message: "`target_user` depends on the workspace crate `core`",
            },
        ],
        "a build-dependency counts as an edge, and so does a dependency only for a platform \
         other than the one the check runs on",
    );
}

/// The violations of `internal_dependency_versions::check`, sorted by file.
fn version_violations(fixture: &Fixture) -> Vec<Violation> {
    let mut violations = internal_dependency_versions::check(&fixture.workspace());
    violations.sort_by(|left, right| left.file().cmp(right.file()));
    violations
}

#[test]
fn a_version_on_a_path_dependency_fails_in_every_code_table() {
    let fixture = Fixture::new("");
    fixture.member("core", "publish = false\n");
    fixture.member("published", "");
    fixture.member("tested", "");
    fixture.member(
        "server",
        "publish = false\n\n[dependencies]\n\
         published = { version = \"0.1.0\", path = \"../published\" }\n\
         [dev-dependencies]\nhelper = { package = \"core\", path = \"../core\", version = \"0.1\" }\n",
    );
    fixture.member(
        "client",
        "[target.'cfg(target_os = \"haiku\")'.dependencies]\n\
         core = { path = \"../core/\", version = \"0.1.0\" }\n",
    );
    fixture.member(
        "library",
        "[dependencies]\npublished = { version = \"0.1.0\", path = \"../published\" }\n\
         [dev-dependencies]\ntesting = { package = \"tested\", version = \"0.1.0\", \
         path = \"../tested\" }\n",
    );
    fixture.write("crates/generated/build.rs", "fn main() {}\n");
    fixture.member(
        "generated",
        "publish = []\n\n[dependencies]\ncore = { path = \"../core\" }\n\
         [build-dependencies]\ncore = { path = \"../core\", version = \"0.1.0\" }\n",
    );
    let violations = version_violations(&fixture);
    assert_violations(
        &violations,
        &[
            Expected {
                file: "crates/client/Cargo.toml",
                message: "gives the workspace crate `core` a `version` beside its `path`. \
                          release-plz rewrites that `version` when it bumps `core` past what the \
                          requirement allows",
            },
            Expected {
                file: "crates/library/Cargo.toml",
                message: "`[dependencies] published` gives the workspace crate `published` a \
                          `version`",
            },
            Expected {
                file: "crates/library/Cargo.toml",
                message: "`[dev-dependencies] testing` gives the workspace crate `tested` a \
                          `version`",
            },
            Expected {
                file: "crates/server/Cargo.toml",
                message: "`[dependencies] published` gives the workspace crate `published` a \
                          `version`",
            },
            Expected {
                file: "crates/server/Cargo.toml",
                message: "`[dev-dependencies] helper` gives the workspace crate `core` a \
                          `version`",
            },
        ],
        "a normal or dev-dependency fails on any platform and under another name, whether or not \
         either crate is published, and a build-dependency or one without a `version` passes",
    );
    let fixes: Vec<&str> = violations
        .iter()
        .map(|violation| {
            let message = violation.message();
            if message.contains("`core` isn't published") {
                "core unpublished"
            } else if message.contains("`server` isn't published") {
                "server unpublished"
            } else if message.contains("out of the published package") {
                "dev-dependency"
            } else if message.contains("give it in `[workspace.dependencies]`") {
                "workspace dependency"
            } else {
                "none"
            }
        })
        .collect();
    assert_eq!(
        fixes,
        [
            "core unpublished",
            "workspace dependency",
            "dev-dependency",
            "server unpublished",
            "server unpublished",
        ],
        "each message gives the fix for its case: {violations:?}"
    );
    assert!(
        mentions(
            &violations,
            "as `published = { path = \"crates/published\", version = \"0.1.0\" }`, with the \
             `path` from the root, and write `published.workspace = true` here"
        ),
        "the published crate's message gives both lines, the root's path read from the root: \
         {violations:?}"
    );
}

#[test]
fn a_version_in_the_workspace_dependencies_passes() {
    let fixture = Fixture::new("");
    let root = fs::read_to_string(fixture.path().join("Cargo.toml"))
        .expect("the root manifest should be read");
    fixture.write(
        "Cargo.toml",
        &format!(
            "{root}\n[workspace.dependencies]\n\
             core = {{ version = \"0.1.0\", path = \"crates/core\" }}\n"
        ),
    );
    fixture.member("core", "");
    fixture.member("library", "[dependencies]\ncore.workspace = true\n");
    let violations = version_violations(&fixture);
    assert!(
        violations.is_empty(),
        "release-plz's rewrite of `[workspace.dependencies]` changes neither rules nor code, so a \
         published crate keeps the `version` there: {violations:?}"
    );
}

#[test]
fn a_published_library_without_a_release_plz_file_fails() {
    let fixture = Fixture::new("");
    let violations = release_changelog::check(&fixture.workspace())
        .unwrap_or_else(|error| panic!("the check should run, but {error}"));
    assert_violations(
        &violations,
        &[Expected {
            file: "release-plz.toml",
            message: "`release-plz.toml` is missing, so release-plz falls back to its defaults",
        }],
        "both members are published, and release-plz's defaults would write a changelog into \
         each crate",
    );
}

#[test]
fn the_release_changelog_must_take_in_every_other_member() {
    let fixture = Fixture::new("");
    fixture.write(
        "release-plz.toml",
        "[workspace]\nchangelog_update = false\n\n[[package]]\nname = \"app\"\n\
         changelog_path = \"./CHANGELOG.md\"\nchangelog_update = true\nchangelog_include = []\n",
    );
    let violations = release_changelog::check(&fixture.workspace())
        .unwrap_or_else(|error| panic!("the check should run, but {error}"));
    assert_eq!(
        files(&violations),
        ["release-plz.toml"],
        "only `derive` is missing"
    );
    assert!(
        mentions(&violations, "`derive` is a workspace member"),
        "the message should name the missing crate: {violations:?}"
    );
}

#[test]
fn a_changelog_release_plz_would_write_into_a_crate_fails() {
    let fixture = Fixture::new("");
    fixture.write(
        "release-plz.toml",
        "[workspace]\ngit_only = true\n\n[[package]]\nname = \"app\"\n\
         changelog_path = \"./CHANGELOG.md\"\nchangelog_update = true\n\
         changelog_include = [\"derive\"]\n",
    );
    let violations = release_changelog::check(&fixture.workspace())
        .unwrap_or_else(|error| panic!("the check should run, but {error}"));
    assert_violations(
        &violations,
        &[Expected {
            file: "release-plz.toml",
            message: "release-plz could write the changelog of `derive` to \
                      `crates/derive/CHANGELOG.md`, which is code",
        }],
        "`derive` has no entry and `[workspace]` leaves `changelog_update` at release-plz's \
         default, so its changelog lands in its crate; the main package's goes to the root",
    );
    fixture.write(
        "release-plz.toml",
        "[workspace]\ngit_only = true\nchangelog_update = false\n\n[[package]]\nname = \"app\"\n\
         changelog_path = \"./CHANGELOG.md\"\nchangelog_update = true\n\
         changelog_include = [\"derive\"]\n",
    );
    let violations = release_changelog::check(&fixture.workspace())
        .unwrap_or_else(|error| panic!("the check should run, but {error}"));
    assert!(
        violations.is_empty(),
        "with `changelog_update = false` in `[workspace]`, only the root changelog is written: \
         {violations:?}"
    );
}

#[test]
fn without_docs_settings_or_a_readme_the_layout_asks_for_them() {
    let fixture = Fixture::new("");
    let violations = docs_layout::check(&fixture.workspace())
        .unwrap_or_else(|error| panic!("the check should run, but {error}"));
    assert_eq!(
        files(&violations),
        ["Cargo.toml", "Cargo.toml", "README.md"],
        "each missing setting and the missing readme fail"
    );
    for text in [
        "`docs_files` isn't set in `[workspace.metadata.policy]`",
        "`readme_sections` isn't set in `[workspace.metadata.policy]`",
        "There is no root `README.md`.",
    ] {
        assert!(
            mentions(&violations, text),
            "a message should say {text:?}: {violations:?}"
        );
    }
}

#[test]
fn a_docs_layout_that_holds_passes() {
    let fixture = Fixture::new(
        "docs_files = [\"README.md\", \"docs/guide.md\"]\n\
         readme_sections = [\"Use it\", \"Documentation\"]\n",
    );
    fixture.write(
        "README.md",
        "# App\n\nAn app.\n\n## Use it\n\nRun it.\n\n## Documentation\n\n\
         - [Guide](docs/guide.md): learn what the app does.\n",
    );
    fixture.write(
        "docs/guide.md",
        "# Guide\n\nFor people new to the app: what it does.\n\n1. Run it.\n",
    );
    let violations = docs_layout::check(&fixture.workspace())
        .unwrap_or_else(|error| panic!("the check should run, but {error}"));
    assert!(
        violations.is_empty(),
        "the listed files exist, the sections match and the page opens with a paragraph: \
         {violations:?}"
    );
}

/// The violations of `license_files::check` on `fixture`, sorted by file.
fn license_violations(fixture: &Fixture) -> Vec<Violation> {
    let mut violations = license_files::check(&fixture.workspace())
        .unwrap_or_else(|error| panic!("the check should run, but {error}"));
    violations.sort_by(|left, right| left.file().cmp(right.file()));
    violations
}

/// Gives the fixture's workspace the `[workspace.package]` license `license`.
fn license_workspace(fixture: &Fixture, license: &str) {
    fixture.write(
        "Cargo.toml",
        &format!(
            "[workspace]\nresolver = \"3\"\nmembers = [\"crates/*\"]\n\n\
             [workspace.package]\nlicense = \"{license}\"\n\n[workspace.metadata.policy]\n"
        ),
    );
}

#[test]
fn a_published_crate_without_its_license_file_fails() {
    let fixture = Fixture::new("");
    license_workspace(&fixture, "MIT");
    fixture.write("LICENSE", "MIT License\n");
    let inherits = "license.workspace = true\n";
    fixture.member("bare", inherits);
    fixture.write("crates/sole/LICENSE", "MIT License\n");
    fixture.member("sole", inherits);
    for (name, file) in [
        ("named", "LICENSE-MIT"),
        ("lower", "license"),
        ("title", "License"),
        ("markdown", "LICENSE.md"),
        ("text", "LICENSE.txt"),
        ("folder", "LICENSE/MIT"),
    ] {
        fixture.write(&format!("crates/{name}/{file}"), "MIT License\n");
        fixture.member(name, inherits);
    }
    fixture.member("private", "license.workspace = true\npublish = false\n");
    fixture.member("nowhere", "license.workspace = true\npublish = []\n");
    fixture.member("own_file", "license-file = \"../../LICENSE\"\n");
    let violations = license_violations(&fixture);
    let expected: Vec<Expected> = [
        "crates/bare/Cargo.toml",
        "crates/folder/Cargo.toml",
        "crates/lower/Cargo.toml",
        "crates/markdown/Cargo.toml",
        "crates/named/Cargo.toml",
        "crates/text/Cargo.toml",
        "crates/title/Cargo.toml",
    ]
    .into_iter()
    .map(|file| Expected {
        file,
        message: "` is published under `MIT`, but `crates/",
    })
    .collect();
    assert_violations(
        &violations,
        &expected,
        "a published crate that inherits MIT needs a file named exactly LICENSE, the name \
         `just package` checks; a crate that isn't published, or names its own license file, \
         needs none",
    );
    let message = "`bare` is published under `MIT`, but `crates/bare/` lacks `LICENSE`. Cargo \
                   packages only the files under a crate's directory, so the crate would reach \
                   crates.io without its license text, which the license asks every copy to \
                   carry. Copy `LICENSE` from the repository root into `crates/bare/`, under the \
                   same name, which `just package` checks too.";
    assert_eq!(
        violations[0].message(),
        message,
        "the message names the file and where to copy it from"
    );
}

#[test]
fn a_dual_licensed_crate_needs_both_files() {
    let fixture = Fixture::new("");
    license_workspace(&fixture, "MIT OR Apache-2.0");
    fixture.write("LICENSE-MIT", "MIT License\n");
    fixture.write("LICENSE-APACHE", "Apache License\n");
    let inherits = "license.workspace = true\n";
    fixture.write("crates/both/LICENSE-MIT", "MIT License\n");
    fixture.write("crates/both/LICENSE-APACHE", "Apache License\n");
    fixture.member("both", inherits);
    fixture.write("crates/half/LICENSE-MIT", "MIT License\n");
    fixture.member("half", inherits);
    fixture.write("crates/sole/LICENSE", "MIT License\n");
    fixture.member("sole", inherits);
    fixture.write("crates/reversed/LICENSE-MIT", "MIT License\n");
    fixture.write("crates/reversed/LICENSE-APACHE", "Apache License\n");
    fixture.member("reversed", "license = \"Apache-2.0 OR MIT\"\n");
    fixture.write("crates/dual_left/LICENSE-MIT", "MIT License\n");
    fixture.write("crates/dual_left/LICENSE-APACHE", "Apache License\n");
    fixture.member("dual_left", "license = \"MIT\"\n");
    let violations = license_violations(&fixture);
    assert_violations(
        &violations,
        &[
            Expected {
                file: "crates/dual_left/Cargo.toml",
                message: "`dual_left` is published under `MIT`, but `crates/dual_left/` lacks \
                          `LICENSE`. Cargo packages",
            },
            Expected {
                file: "crates/half/Cargo.toml",
                message: "`half` is published under `MIT OR Apache-2.0`, but `crates/half/` lacks \
                          `LICENSE-APACHE`. Cargo packages",
            },
            Expected {
                file: "crates/sole/Cargo.toml",
                message: "but `crates/sole/` lacks `LICENSE-MIT` and `LICENSE-APACHE`. Cargo \
                          packages",
            },
        ],
        "under two licenses, in either order, a crate needs a file named for each and a sole \
         LICENSE doesn't do; under MIT alone, the files a dual license left don't do either",
    );
    assert!(
        mentions(
            &violations,
            "Copy `LICENSE-MIT` and `LICENSE-APACHE` from the repository root into \
             `crates/sole/`, under the same names"
        ),
        "the message names both files: {violations:?}"
    );
}
