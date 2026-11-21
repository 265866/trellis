//! `trellis-policy web-notices` on `node_modules` directories laid out as Bun installs them.

use std::ffi::OsStr;
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt as _;
use std::path::{MAIN_SEPARATOR, Path, PathBuf};
use std::process::{Command, Output};
use std::{env, fs};

use tempfile::TempDir;

/// The `about.toml` of a generated project, which accepts these licenses.
const ABOUT: &str = "accepted = [\"Apache-2.0\", \"BSD-2-Clause\", \"BSD-3-Clause\", \"ISC\", \
                     \"MIT\", \"Unicode-3.0\", \"Zlib\"]\n\n\
                     targets = [\"x86_64-unknown-linux-gnu\"]\n";

/// A web UI's install in a temporary directory, deleted on drop: `about.toml`, `node_modules/`,
/// and the notices the command writes, `notices.txt`.
struct Install {
    /// The temporary directory.
    directory: TempDir,
}

impl Install {
    /// An install with the generated project's `about.toml` and an empty `node_modules/`.
    fn new() -> Self {
        let install = Self {
            directory: TempDir::new().expect("the temporary directory should be created"),
        };
        install.write("about.toml", ABOUT);
        fs::create_dir(install.path("node_modules")).expect("node_modules should be created");
        install
    }

    /// The path of `relative` in the temporary directory.
    fn path(&self, relative: &str) -> PathBuf {
        self.directory.path().join(relative)
    }

    /// Writes `contents` to `relative` in the temporary directory.
    fn write(&self, relative: &str, contents: &str) {
        let path = self.path(relative);
        let directory = path
            .parent()
            .expect("a file in the install has a directory");
        fs::create_dir_all(directory).expect("the directory should be created");
        fs::write(path, contents).expect("the file should be written");
    }

    /// Writes a `package.json` with the name `name`, the version `version` and the `license`
    /// value `license`, written as JSON, in `directory`.
    fn package(&self, directory: &str, name: &str, version: &str, license: &str) {
        self.write(
            &format!("{directory}/package.json"),
            &format!(
                "{{\"name\": \"{name}\", \"version\": \"{version}\", \"license\": {license}}}\n"
            ),
        );
    }

    /// Links `link` to `target`, both relative to the temporary directory.
    fn link(&self, target: &str, link: &str) {
        let link = self.path(link);
        fs::create_dir_all(link.parent().expect("a link has a directory"))
            .expect("the link's directory should be created");
        symbolic_link(self.path(target), &link);
    }

    /// The names of the entries in the temporary directory, sorted.
    fn listing(&self) -> Vec<String> {
        let mut names = Vec::new();
        for entry in fs::read_dir(self.directory.path()).expect("the directory should be read") {
            let entry = entry.expect("the entry should be read");
            names.push(entry.file_name().to_string_lossy().into_owned());
        }
        names.sort();
        names
    }

    /// Runs `trellis-policy web-notices about.toml node_modules notices.txt` outside CI.
    fn run(&self) -> Run {
        self.run_under(Ci::Local)
    }

    /// Runs `trellis-policy web-notices about.toml node_modules notices.txt` under `ci`.
    fn run_under(&self, ci: Ci) -> Run {
        run(
            self.directory.path(),
            &["web-notices", "about.toml", "node_modules", "notices.txt"],
            ci,
        )
    }

    /// The notices the command wrote, if it wrote them.
    fn notices(&self) -> Option<String> {
        fs::read_to_string(self.path("notices.txt")).ok()
    }

    /// The name, version and license line of each entry in the notices, in order.
    fn entries(&self) -> Vec<String> {
        let notices = self.notices().expect("the notices should be written");
        let mut lines = notices.lines();
        let mut entries = Vec::new();
        while let Some(line) = lines.next() {
            if line.starts_with("=====") {
                entries.push(
                    lines
                        .next()
                        .expect("a separator starts an entry")
                        .to_owned(),
                );
            }
        }
        entries
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

/// What a run of the command did.
#[derive(Debug)]
struct Run {
    /// The exit code.
    code: Option<i32>,
    /// What it wrote to standard output, with the system's path separator written as `/`. The
    /// command prints file system paths as the system writes them, so on Windows they hold `\`.
    stdout: String,
    /// What it wrote to standard error.
    stderr: String,
}

impl Run {
    /// Asserts that the run exited with `code` and printed a line that contains each of `texts`.
    fn assert_reports(&self, code: i32, texts: &[&str]) {
        assert_eq!(
            self.code,
            Some(code),
            "the exit code should be {code}: {self:#?}"
        );
        for text in texts {
            assert!(
                self.stdout.lines().any(|line| line.contains(text)),
                "a line should contain {text:?}: {self:#?}"
            );
        }
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
fn run(directory: &Path, arguments: &[impl AsRef<OsStr>], ci: Ci) -> Run {
    let mut command = Command::new(env!("CARGO_BIN_EXE_trellis-policy"));
    command
        .args(arguments)
        .current_dir(directory)
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
        stdout: String::from_utf8_lossy(&stdout).replace(MAIN_SEPARATOR, "/"),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
    }
}

#[test]
fn the_notices_have_a_heading_then_an_entry_per_package() {
    let install = Install::new();
    install.write(
        "node_modules/zed/package.json",
        "{\"name\": \"zed\", \"version\": \"0.1.0\", \"license\": \"ISC\", \"author\": \
         {\"name\": \"Zed\", \"email\": \"zed@example.com\"}, \"contributors\": [\"Ann\", \
         {\"name\": \"Bo\"}], \"maintainers\": [{\"name\": \"Cy\", \"email\": \"cy@example.com\"}]}\n",
    );
    install.package("node_modules/plain", "plain", "1.0.0", "\"MIT\"");
    install.package("node_modules/alpha", "alpha", "1.2.3", "\"MIT\"");
    install.write(
        "node_modules/alpha/LICENSE",
        "MIT License\n\nCopyright alpha\n\n\n",
    );
    install.write("node_modules/alpha/notice.txt", "Alpha notice\n");
    install.write("node_modules/alpha/README.md", "Not a license\n");
    install.write(
        "node_modules/alpha/lib/LICENSE",
        "Not directly in the package\n",
    );
    install.run().assert_reports(
        0,
        &["Wrote the licenses of 3 npm package(s) under node_modules to notices.txt."],
    );
    let separator = "=".repeat(80);
    assert_eq!(
        install.notices().as_deref(),
        Some(
            format!(
                "Licenses of the installed npm packages, development and transitive dependencies \
                 included\n\n{separator}\nalpha 1.2.3: MIT\n\nMIT License\n\nCopyright alpha\n\n\
                 Alpha notice\n\n{separator}\nplain 1.0.0: MIT\n\nplain 1.0.0 ships no license \
                 file. Its license is the standard text of MIT.\nIt states no copyright holder.\n\n\
                 {separator}\nzed 0.1.0: ISC\n\nzed 0.1.0 ships no license file. Its license is \
                 the standard text of ISC.\nAuthor: Zed <zed@example.com>\nContributor: Ann\n\
                 Contributor: Bo\nMaintainer: Cy <cy@example.com>\n"
            )
            .as_str()
        ),
        "the notices list each package with its license files, or its people"
    );
    assert_eq!(
        install.listing(),
        ["about.toml", "node_modules", "notices.txt"],
        "nothing but the notices is left beside them"
    );
}

#[test]
fn third_party_notices_count_as_license_files_and_code_doesnt() {
    let install = Install::new();
    install.package("node_modules/a", "a", "1.0.0", "\"MIT\"");
    for (file, text) in [
        ("MIT-LICENSE", "mit license text"),
        ("THIRD-PARTY-LICENSE", "third party license text"),
        ("ThirdPartyNoticeText.txt", "third party notice text"),
        ("CopyrightNotice.txt", "copyright notice text"),
        ("license.js", "license code"),
        ("licenses.json", "license data"),
        ("notice.d.ts", "notice types"),
    ] {
        install.write(&format!("node_modules/a/{file}"), text);
    }
    install.run().assert_reports(0, &[]);
    let notices = install.notices().expect("the notices should be written");
    assert!(
        notices.contains(
            "\ncopyright notice text\n\nmit license text\n\nthird party license text\n\n\
             third party notice text\n"
        ),
        "every notice file is included, sorted by name: {notices}"
    );
    for text in ["license code", "license data", "notice types"] {
        assert!(
            !notices.contains(text),
            "{text} is code, not a notice: {notices}"
        );
    }

    let binary = Install::new();
    binary.package("node_modules/b", "b", "1.0.0", "\"MIT\"");
    fs::write(
        binary.path("node_modules/b/LICENSE.png"),
        [0x89_u8, 0xff, 0x00],
    )
    .expect("the image should be written");
    fs::write(
        binary.path("node_modules/b/NOTICE.bin"),
        b"text\0with a NUL",
    )
    .expect("the file should be written");
    binary.run().assert_reports(0, &[]);
    let notices = binary.notices().expect("the notices should be written");
    assert!(
        notices.contains("b 1.0.0 ships no license file.") && !notices.contains("with a NUL"),
        "a license file that isn't text is skipped: {notices}"
    );
}

#[test]
fn hoisted_scoped_and_nested_packages_are_listed_once_each() {
    let install = Install::new();
    install.package("node_modules/b", "b", "1.0.0", "\"MIT\"");
    install.package("node_modules/a", "a", "2.0.0", "\"MIT\"");
    install.package("node_modules/a/node_modules/b", "b", "0.9.0", "\"ISC\"");
    install.package("node_modules/c/node_modules/b", "b", "0.9.0", "\"ISC\"");
    install.package("node_modules/c", "c", "1.0.0", "\"MIT\"");
    install.package("node_modules/@scope/d", "@scope/d", "3.0.0", "\"Zlib\"");
    install.package(
        "node_modules/@scope/d/node_modules/@other/e",
        "@other/e",
        "1.0.0",
        "\"BSD-2-Clause\"",
    );
    install
        .run()
        .assert_reports(0, &["of 6 npm package(s) under node_modules"]);
    assert_eq!(
        install.entries(),
        [
            "@other/e 1.0.0: BSD-2-Clause",
            "@scope/d 3.0.0: Zlib",
            "a 2.0.0: MIT",
            "b 0.9.0: ISC",
            "b 1.0.0: MIT",
            "c 1.0.0: MIT",
        ],
        "every package is listed once, sorted by name then version"
    );
}

#[test]
fn copies_of_a_package_that_differ_are_each_judged_and_listed() {
    let install = Install::new();
    install.package("node_modules/b", "b", "1.0.0", "\"MIT\"");
    install.package(
        "node_modules/a/node_modules/b",
        "b",
        "1.0.0",
        "\"GPL-3.0-only\"",
    );
    install.package("node_modules/a", "a", "1.0.0", "\"MIT\"");
    install.run().assert_reports(
        1,
        &["error: b 1.0.0 in node_modules/a/node_modules/b has the license `GPL-3.0-only`"],
    );

    let texts = Install::new();
    texts.package("node_modules/b", "b", "1.0.0", "\"MIT\"");
    texts.write("node_modules/b/LICENSE", "Copyright one\n");
    texts.package("node_modules/a/node_modules/b", "b", "1.0.0", "\"MIT\"");
    texts.write("node_modules/a/node_modules/b/LICENSE", "Copyright two\n");
    texts.package("node_modules/c/node_modules/b", "b", "1.0.0", "\"MIT\"");
    texts.write("node_modules/c/node_modules/b/LICENSE", "Copyright two\n");
    texts.package("node_modules/a", "a", "1.0.0", "\"MIT\"");
    texts.package("node_modules/c", "c", "1.0.0", "\"MIT\"");
    texts.run().assert_reports(0, &[]);
    let notices = texts.notices().expect("the notices should be written");
    assert_eq!(
        texts.entries(),
        [
            "a 1.0.0: MIT",
            "b 1.0.0: MIT",
            "b 1.0.0: MIT",
            "c 1.0.0: MIT"
        ],
        "the copies with different license texts each get an entry: {notices}"
    );
    assert!(
        notices.contains("Copyright one") && notices.matches("Copyright two").count() == 1,
        "the identical copies get one entry: {notices}"
    );
}

#[test]
fn the_isolated_layout_lists_each_package_once_without_following_links() {
    let install = Install::new();
    install.package(
        "node_modules/.bun/a@1.0.0/node_modules/a",
        "a",
        "1.0.0",
        "\"MIT\"",
    );
    install.package(
        "node_modules/.bun/@scope+b@2.0.0/node_modules/@scope/b",
        "@scope/b",
        "2.0.0",
        "\"ISC\"",
    );
    install.write(
        "node_modules/.bun/@scope+b@2.0.0/node_modules/@scope/b/LICENSE",
        "ISC License\n",
    );
    install.link("node_modules/.bun/a@1.0.0/node_modules/a", "node_modules/a");
    install.link(
        "node_modules/.bun/@scope+b@2.0.0/node_modules/@scope/b",
        "node_modules/@scope/b",
    );
    install.link(
        "node_modules/.bun/a@1.0.0/node_modules/a",
        "node_modules/.bun/@scope+b@2.0.0/node_modules/a",
    );
    install.link("outside", "node_modules/.bun/outside");
    install
        .run()
        .assert_reports(0, &["of 2 npm package(s) under node_modules"]);
    assert_eq!(
        install.entries(),
        ["@scope/b 2.0.0: ISC", "a 1.0.0: MIT"],
        "each package is read from its directory in .bun, not through the links"
    );
    let notices = install.notices().expect("the notices should be written");
    assert!(
        notices.contains("\nISC License\n"),
        "the scoped package's license file is read: {notices}"
    );
}

#[test]
fn a_link_in_a_package_place_must_lead_to_another_package_place() {
    let install = Install::new();
    install.package("node_modules/a", "a", "1.0.0", "\"MIT\"");
    install.write("node_modules/a/lib/index.js", "\n");
    install.link("node_modules/a", "node_modules/fine");
    install.package("outside/gpl", "gpl", "1.0.0", "\"GPL-3.0-only\"");
    install.link("outside/gpl", "node_modules/gpl");
    install.link("node_modules/missing", "node_modules/broken");
    install.link("node_modules/a/lib", "node_modules/inner");
    install.package("node_modules/@scope/s", "@scope/s", "1.0.0", "\"MIT\"");
    install.link("node_modules/@scope", "node_modules/@linked");
    install.link("outside", "node_modules/.hidden");
    let run = install.run();
    let refused = "which isn't a package's place under node_modules that the check reads, so its \
                   license can't be checked.";
    run.assert_reports(
        1,
        &[
            "error: node_modules/broken is a link the check can't follow: ",
            "error: node_modules/gpl is a link to ",
            &format!("/outside/gpl, {refused}"),
            "error: node_modules/inner is a link to ",
            &format!("/node_modules/a/lib, {refused}"),
            "error: node_modules/@linked is a link to ",
            &format!("/node_modules/@scope, {refused}"),
            "4 of 6 npm package(s) under node_modules can't be accepted under about.toml",
        ],
    );
    assert!(
        !run.stdout.contains("node_modules/fine") && !run.stdout.contains(".hidden"),
        "a link to a package's place, and one outside a package's place, pass: {run:#?}"
    );
}

#[test]
fn a_linked_package_json_is_refused() {
    let install = Install::new();
    install.write(
        "outside/package.json",
        "{\"name\": \"linked\", \"version\": \"1.0.0\", \"license\": \"MIT\"}\n",
    );
    install.link("outside/package.json", "node_modules/linked/package.json");
    install.run().assert_reports(
        1,
        &[
            "error: node_modules/linked has a package.json that is a symbolic link, which the \
           check doesn't follow, so its license can't be checked.",
        ],
    );
}

#[test]
fn linked_license_files_are_read_only_from_inside_node_modules() {
    let install = Install::new();
    install.package("node_modules/a", "a", "1.0.0", "\"MIT\"");
    install.write("node_modules/.shared/text", "shared license text\n");
    install.link("node_modules/.shared/text", "node_modules/a/LICENSE");
    install.write("outside/COPYING", "outside license text\n");
    install.link("outside/COPYING", "node_modules/a/COPYING");
    install.link("node_modules/missing", "node_modules/a/NOTICE");
    install.run().assert_reports(0, &["of 1 npm package(s)"]);
    let notices = install.notices().expect("the notices should be written");
    assert!(
        notices.contains("shared license text") && !notices.contains("outside license text"),
        "a linked license file is read only from inside node_modules: {notices}"
    );
}

#[test]
fn only_a_package_json_where_an_installer_places_a_package_counts() {
    let install = Install::new();
    install.package("node_modules/a", "a", "1.0.0", "\"MIT\"");
    install.write(
        "node_modules/a/esm/package.json",
        "{\"type\": \"module\"}\n",
    );
    install.package("node_modules/a/lib/b", "b", "1.0.0", "\"GPL-3.0-only\"");
    install.package(
        "node_modules/.bin/tool",
        "tool",
        "1.0.0",
        "\"GPL-3.0-only\"",
    );
    install.package(
        "node_modules/.cache/cached",
        "cached",
        "1.0.0",
        "\"GPL-3.0-only\"",
    );
    install.package("node_modules/@scope", "@scope", "1.0.0", "\"GPL-3.0-only\"");
    install.package(
        "node_modules/.hidden",
        "hidden",
        "1.0.0",
        "\"GPL-3.0-only\"",
    );
    fs::create_dir(install.path("node_modules/empty")).expect("the directory should be created");
    install
        .run()
        .assert_reports(0, &["of 1 npm package(s) under node_modules"]);
    assert_eq!(
        install.entries(),
        ["a 1.0.0: MIT"],
        "only the package at a package's path counts"
    );
}

#[test]
fn a_place_without_a_usable_package_json_is_refused() {
    let install = Install::new();
    install.package("node_modules/a", "a", "1.0.0", "\"MIT\"");
    for (directory, manifest) in [
        (
            "unversioned",
            "{\"name\": \"unversioned\", \"license\": \"MIT\"}",
        ),
        (
            "numbered",
            "{\"name\": \"numbered\", \"version\": 1, \"license\": \"MIT\"}",
        ),
        ("unnamed", "{\"version\": \"1.0.0\", \"license\": \"MIT\"}"),
        (
            "empty",
            "{\"name\": \"\", \"version\": \"1.0.0\", \"license\": \"MIT\"}",
        ),
        ("@scope/array", "[\"array\"]"),
    ] {
        install.write(&format!("node_modules/{directory}/package.json"), manifest);
    }
    install.write("node_modules/no-manifest/index.js", "\n");
    fs::create_dir_all(install.path("node_modules/manifest-directory/package.json"))
        .expect("the directory should be created");
    let run = install.run();
    let refused = "holds a package.json without a string `name` and `version`, so its license \
                   can't be checked.";
    run.assert_reports(
        1,
        &[
            &format!("error: node_modules/unversioned {refused}"),
            &format!("error: node_modules/numbered {refused}"),
            &format!("error: node_modules/unnamed {refused}"),
            &format!("error: node_modules/empty {refused}"),
            &format!("error: node_modules/@scope/array {refused}"),
            "error: node_modules/no-manifest holds files but no package.json, so its license \
             can't be checked. Drop the package, or ask the trellis owner to support this layout \
             if an installer wrote it.",
            "error: node_modules/manifest-directory has a package.json that isn't a file, so its \
             license can't be checked.",
            "7 of 8 npm package(s) under node_modules can't be accepted under about.toml, so \
             notices.txt wasn't written.",
        ],
    );
    assert_eq!(install.notices(), None, "no notices are written");
}

#[test]
fn an_alias_is_named_and_judged_by_its_package_json() {
    let install = Install::new();
    install.package("node_modules/leftpad", "left-pad", "1.3.0", "\"WTFPL\"");
    install.package("node_modules/my-react", "react", "19.0.0", "\"MIT\"");
    install.run().assert_reports(
        1,
        &[
            "error: left-pad 1.3.0 in node_modules/leftpad has the license `WTFPL`, which isn't \
             accepted.",
            "1 of 2 npm package(s)",
        ],
    );

    let accepted = Install::new();
    accepted.package("node_modules/my-react", "react", "19.0.0", "\"MIT\"");
    accepted.run().assert_reports(0, &[]);
    assert_eq!(
        accepted.entries(),
        ["react 19.0.0: MIT"],
        "an alias is listed under the package's own name"
    );
}

#[test]
fn a_package_json_with_a_byte_order_mark_is_read() {
    let install = Install::new();
    install.write(
        "node_modules/a/package.json",
        "\u{feff}{\"name\": \"a\", \"version\": \"1.0.0\", \"license\": \"MIT\"}\n",
    );
    install.run().assert_reports(0, &[]);
    assert_eq!(
        install.entries(),
        ["a 1.0.0: MIT"],
        "the byte order mark is skipped"
    );
}

#[test]
fn a_license_object_or_licenses_array_gives_the_expression() {
    let install = Install::new();
    install.package(
        "node_modules/object",
        "object",
        "1.0.0",
        "{\"type\": \"MIT\"}",
    );
    install.write(
        "node_modules/array/package.json",
        "{\"name\": \"array\", \"version\": \"1.0.0\", \"licenses\": [{\"type\": \"MIT\", \
         \"url\": \"https://example.com\"}, {\"type\": \"GPL-2.0-only\"}]}\n",
    );
    install.run().assert_reports(0, &[]);
    assert_eq!(
        install.entries(),
        ["array 1.0.0: (MIT) OR (GPL-2.0-only)", "object 1.0.0: MIT"],
        "the object's type, and the array's types each in parentheses joined with OR, are the \
         expressions"
    );
}

#[test]
fn malformed_license_fields_are_refused() {
    let install = Install::new();
    install.write(
        "node_modules/split/package.json",
        "{\"name\": \"split\", \"version\": \"1.0.0\", \"licenses\": [{\"type\": \
         \"(GPL-3.0-only\"}, {\"type\": \"MIT)\"}]}\n",
    );
    install.write(
        "node_modules/untyped/package.json",
        "{\"name\": \"untyped\", \"version\": \"1.0.0\", \"licenses\": [{\"type\": \
         \"MIT\"}, {\"url\": \"https://example.com\"}]}\n",
    );
    install.package(
        "node_modules/object",
        "object",
        "1.0.0",
        "{\"url\": \"https://example.com\"}",
    );
    let unreadable = "which isn't a strict SPDX expression: current license ids, in their exact \
                      case, joined with AND, OR and parentheses.";
    install.run().assert_reports(
        1,
        &[
            &format!(
                "error: split 1.0.0 in node_modules/split has the license \
                 `((GPL-3.0-only) OR (MIT))`, {unreadable}"
            ),
            &format!(
                "error: untyped 1.0.0 in node_modules/untyped has the license `(MIT) OR ()`, \
                 {unreadable}"
            ),
            "error: object 1.0.0 in node_modules/object states no license in its package.json.",
            "3 of 3 npm package(s)",
        ],
    );
}

#[test]
fn expressions_with_or_and_parentheses_and_plus_are_accepted() {
    let install = Install::new();
    for (index, expression) in [
        "MIT OR GPL-3.0-only",
        "MIT AND ISC",
        "(MIT OR GPL-3.0-only) AND (Apache-2.0 OR MPL-2.0)",
        "GPL-3.0-only AND MPL-2.0 OR Zlib",
        "Apache-2.0+",
    ]
    .iter()
    .enumerate()
    {
        install.package(
            &format!("node_modules/p{index}"),
            &format!("p{index}"),
            "1.0.0",
            &format!("\"{expression}\""),
        );
    }
    install
        .run()
        .assert_reports(0, &["of 5 npm package(s) under node_modules"]);
}

#[test]
fn every_package_whose_license_isnt_accepted_fails_and_nothing_is_written() {
    let install = Install::new();
    install.package("node_modules/fine", "fine", "1.0.0", "\"MIT\"");
    install.package("node_modules/gpl", "gpl", "1.0.0", "\"GPL-3.0-only\"");
    install.package("node_modules/both", "both", "1.0.0", "\"MIT AND MPL-2.0\"");
    install.package(
        "node_modules/with",
        "with",
        "1.0.0",
        "\"Apache-2.0 WITH LLVM-exception\"",
    );
    install.package("node_modules/private", "private", "1.0.0", "\"UNLICENSED\"");
    install.package(
        "node_modules/see",
        "see",
        "1.0.0",
        "\"SEE LICENSE IN LICENSE.md\"",
    );
    install.write(
        "node_modules/@scope/none/package.json",
        "{\"name\": \"@scope/none\", \"version\": \"2.0.0\"}\n",
    );
    let run = install.run();
    let add = "If that license is acceptable, add its SPDX id to `accepted` in about.toml; \
               otherwise drop the package.";
    let drop = "No entry in `accepted` in about.toml can accept it, so drop the package.";
    run.assert_reports(
        1,
        &[
            &format!(
                "error: gpl 1.0.0 in node_modules/gpl has the license `GPL-3.0-only`, which isn't \
                 accepted. {add}"
            ),
            &format!(
                "error: both 1.0.0 in node_modules/both has the license `MIT AND MPL-2.0`, which \
                 isn't accepted. {add}"
            ),
            "error: with 1.0.0 in node_modules/with has the license `Apache-2.0 WITH \
             LLVM-exception`, which adds an exception. trellis doesn't support `WITH` \
             exceptions, so drop the package, or ask the trellis owner to support this one.",
            &format!(
                "error: private 1.0.0 in node_modules/private has the license `UNLICENSED`, which \
                 isn't a strict SPDX expression: current license ids, in their exact case, \
                 joined with AND, OR and parentheses. {drop}"
            ),
            "error: see 1.0.0 in node_modules/see has the license `SEE LICENSE IN LICENSE.md`",
            &format!(
                "error: @scope/none 2.0.0 in node_modules/@scope/none states no license in its \
                 package.json. {drop}"
            ),
            "6 of 7 npm package(s) under node_modules can't be accepted under about.toml, so \
             notices.txt wasn't written.",
        ],
    );
    assert_eq!(
        run.stdout
            .lines()
            .filter(|line| line.starts_with("error: "))
            .count(),
        6,
        "one error per refused package: {run:#?}"
    );
    assert!(
        !run.stdout.contains("fine"),
        "an accepted package isn't reported: {run:#?}"
    );
    assert_eq!(install.notices(), None, "no notices are left");
}

#[test]
fn errors_are_annotations_under_github_actions() {
    let install = Install::new();
    install.package("node_modules/gpl", "gpl", "1.0.0", "\"GPL-3.0-only\"");
    install.run_under(Ci::GitHubActions).assert_reports(
        1,
        &["::error::gpl 1.0.0 in node_modules/gpl has the license"],
    );
}

#[test]
fn a_usage_error_exits_with_2() {
    let install = Install::new();
    for arguments in [
        &["web-notices"][..],
        &["web-notices", "about.toml", "node_modules"],
        &[
            "web-notices",
            "about.toml",
            "node_modules",
            "notices.txt",
            "extra",
        ],
        &["web-notices", "--about", "node_modules", "notices.txt"],
    ] {
        let run = run(install.directory.path(), arguments, Ci::Local);
        assert_eq!(
            run.code,
            Some(2),
            "{arguments:?} is a usage error: {run:#?}"
        );
        assert!(
            run.stderr
                .contains("trellis-policy web-notices <about.toml> <node_modules> <output>"),
            "{arguments:?} prints the usage: {run:#?}"
        );
    }
}

#[cfg(unix)]
#[test]
fn an_argument_that_isnt_utf8_is_a_usage_error() {
    let install = Install::new();
    let arguments = [
        OsStr::new("web-notices"),
        OsStr::from_bytes(b"about\xff.toml"),
        OsStr::new("node_modules"),
        OsStr::new("notices.txt"),
    ];
    let run = run(install.directory.path(), &arguments, Ci::Local);
    assert_eq!(run.code, Some(2), "the argument is a usage error: {run:#?}");
    assert!(
        run.stderr.starts_with("error: the argument ") && run.stderr.contains("isn't UTF-8"),
        "the error names the argument: {run:#?}"
    );
    assert!(
        run.stderr.contains("usage: trellis-policy check"),
        "the usage is printed: {run:#?}"
    );
}

#[test]
fn a_read_error_exits_with_2() {
    let missing = Install::new();
    fs::remove_dir(missing.path("node_modules")).expect("node_modules should be removed");
    missing.write("notices.txt", "notices from an earlier run\n");
    missing
        .run()
        .assert_reports(2, &["error: couldn't read node_modules: "]);
    assert_eq!(
        missing.notices(),
        None,
        "no notices are written, and an earlier run's are removed"
    );

    for (about, message) in [
        (
            "accepted = \"MIT\"\n",
            "`accepted` should be an array of SPDX license ids",
        ),
        (
            "accepted = [\"MIT\", 1]\n",
            "`accepted` should be an array of SPDX license ids",
        ),
        (
            "targets = []\n",
            "`accepted` should be an array of SPDX license ids",
        ),
        (
            "accepted = [\"mit\"]\n",
            "`accepted` should be an array of SPDX license ids",
        ),
        ("accepted = [\n", "about.toml isn't valid TOML"),
    ] {
        let install = Install::new();
        install.write("about.toml", about);
        install
            .run()
            .assert_reports(2, &["error: about.toml", message]);
    }

    let not_json = Install::new();
    not_json.write("node_modules/broken/package.json", "{\"name\": \n");
    not_json.run().assert_reports(
        2,
        &["error: node_modules/broken/package.json isn't valid JSON: "],
    );

    let unwritable = Install::new();
    unwritable.package("node_modules/a", "a", "1.0.0", "\"MIT\"");
    let run = run(
        unwritable.directory.path(),
        &[
            "web-notices",
            "about.toml",
            "node_modules",
            "missing/notices.txt",
        ],
        Ci::Local,
    );
    run.assert_reports(2, &["error: couldn't write missing/notices.txt: "]);
    assert!(
        !unwritable.path("missing").exists(),
        "no directory is created for the output: {run:#?}"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn a_directory_name_that_isnt_utf8_is_a_read_error() {
    let install = Install::new();
    install.package("node_modules/a", "a", "1.0.0", "\"MIT\"");
    let name = OsStr::from_bytes(b"b\xff");
    fs::create_dir(install.path("node_modules/a").join(name))
        .expect("the directory should be created");
    install.run().assert_reports(
        2,
        &[
            "error: node_modules/a/b\u{fffd}: the name isn't UTF-8, so the packages in it can't \
             be named",
        ],
    );
}
