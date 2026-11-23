//! The `readme` rule on the readmes and galleries of two projects that wrote them, spareline and
//! teamdiff, copied verbatim into `tests/readme/`, and on copies that each break one clause.
//!
//! spareline's `README.md` and `docs/web-app.md` are from its commit da6fa00, and teamdiff's
//! `README.md` and `docs/usage.md` from its commit 8e6661b. Their images are kept small:
//! spareline's two smallest PNG files, `sign-in.png` and `sign-in-mobile.png`, are its real ones,
//! which carry only `IHDR`, `IDAT` and `IEND`, and its other PNG files and teamdiff's SVG files
//! are stand-ins of the same names. The other pages their indexes link are stand-ins written
//! here.

use std::ffi::OsString;
use std::path::Path;
use std::process::Command;
use std::{env, fs};

use tempfile::TempDir;
use trellis_policy::{Fails, Violation, Workspace, docs_index, docs_layout, readme};

/// The pages besides the gallery that both projects' indexes link.
const SHARED_PAGES: [&str; 6] = [
    "install",
    "verifying-releases",
    "design",
    "stability",
    "style",
    "maintaining",
];

/// A pilot project in a temporary directory, deleted on drop: its readme, gallery and images,
/// with a workspace around them.
struct Pilot {
    /// The workspace's directory.
    directory: TempDir,
}

impl Pilot {
    /// The spareline pilot, a service with a web UI, as its settings say.
    fn spareline() -> Self {
        Self::new("spareline", "[\"web\"]", &[])
    }

    /// The teamdiff pilot, a full-screen terminal app, as its settings say.
    fn teamdiff() -> Self {
        Self::new("teamdiff", "[\"terminal\"]", &["privacy"])
    }

    /// The pilot `name`, copied from `tests/readme/<name>`, with the interfaces `interfaces`, the
    /// readme sections both pilots have, their license, and a stand-in for each page in
    /// [`SHARED_PAGES`] and `pages`.
    fn new(name: &str, interfaces: &str, pages: &[&str]) -> Self {
        let pilot = Self {
            directory: TempDir::new().expect("the temporary directory should be created"),
        };
        copy_tree(
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/readme")
                .join(name),
            pilot.path(),
        );
        pilot.manifest(&format!(
            "interfaces = {interfaces}\n\
             readme_sections = [\"Quick start\", \"Documentation\", \"License\"]\n"
        ));
        pilot.write(
            "crates/app/Cargo.toml",
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        );
        pilot.write("crates/app/src/lib.rs", "//! The app.\n");
        for page in SHARED_PAGES.iter().chain(pages) {
            pilot.write(
                &format!("docs/{page}.md"),
                format!("# {page}\n\nA stand-in for the pilot's page.\n"),
            );
        }
        let cargo = env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"));
        let output = Command::new(cargo)
            .args(["generate-lockfile", "--offline"])
            .current_dir(pilot.path())
            .output()
            .expect("cargo should run");
        assert!(
            output.status.success(),
            "cargo generate-lockfile failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        pilot
    }

    /// Writes the root `Cargo.toml` with the `[workspace.metadata.policy]` settings `policy`.
    fn manifest(&self, policy: &str) {
        self.write(
            "Cargo.toml",
            format!(
                "[workspace]\nresolver = \"3\"\nmembers = [\"crates/*\"]\n\n\
                 [workspace.package]\nlicense = \"MIT OR Apache-2.0\"\n\n\
                 [workspace.metadata.policy]\ndocs_files = [\"README.md\"]\n{policy}"
            ),
        );
    }

    /// The workspace's directory.
    fn path(&self) -> &Path {
        self.directory.path()
    }

    /// Writes `contents` to `path` in the workspace.
    fn write(&self, path: &str, contents: impl AsRef<[u8]>) {
        let path = self.path().join(path);
        let directory = path
            .parent()
            .expect("a file in the workspace has a directory");
        fs::create_dir_all(directory).expect("the directory should be created");
        fs::write(path, contents).expect("the file should be written");
    }

    /// The text of `path` in the workspace.
    fn read(&self, path: &str) -> String {
        fs::read_to_string(self.path().join(path)).expect("the file should be read")
    }

    /// Replaces `from` with `to` in `path`, which must hold `from`.
    fn replace(&self, path: &str, from: &str, to: &str) {
        let text = self.read(path);
        assert!(text.contains(from), "{path} should hold {from:?}");
        self.write(path, text.replacen(from, to, 1));
    }

    /// Removes `path` from the workspace.
    fn remove(&self, path: &str) {
        fs::remove_file(self.path().join(path)).expect("the file should be removed");
    }

    /// The violations of the `readme` rule.
    fn readme(&self) -> Vec<Violation> {
        readme::check(&self.workspace())
            .unwrap_or_else(|error| panic!("the readme check should run, but {error}"))
    }

    /// The workspace, as the checks read it.
    fn workspace(&self) -> Workspace {
        Workspace::load(Some(&self.path().join("Cargo.toml")))
            .unwrap_or_else(|error| panic!("the checks should read the workspace, but {error}"))
    }

    /// Asserts that the `readme` rule reports exactly one violation, on `file`, whose message
    /// contains each of `texts`, and that fails as `fails` says.
    fn assert_breaks(&self, file: &str, fails: Fails, texts: &[&str]) {
        let release_only = fails == Fails::Release;
        let violations = self.readme();
        let [violation] = violations.as_slice() else {
            panic!("exactly one violation should be reported: {violations:#?}");
        };
        assert_eq!(violation.file(), file, "the violation names the file");
        for text in texts {
            assert!(
                violation.message().contains(text),
                "the message should say {text:?}: {violation}"
            );
        }
        let release = "`just policy` only notes this, but a release fails on it; `just \
                       policy-release` runs the release check locally.";
        assert_eq!(
            violation.fails(),
            fails,
            "the violation fails the checks its clause says: {violation}"
        );
        assert_eq!(
            violation.message().ends_with(release),
            release_only,
            "a release clause's message says a release fails on it, and no other's does: \
             {violation}"
        );
    }
}

/// Copies every file under `from` into `to`, keeping their paths.
fn copy_tree(from: &Path, to: &Path) {
    for entry in fs::read_dir(from).expect("the fixture should be read") {
        let entry = entry.expect("the fixture's entry should be read");
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            fs::create_dir_all(&target).expect("the directory should be created");
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).expect("the file should be copied");
        }
    }
}

/// `bytes`, a PNG, with a `tEXt` chunk after its `IHDR`, as an image editor writes one.
fn with_text_chunk(bytes: &[u8]) -> Vec<u8> {
    let end_of_header = 8 + 4 + 4 + 13 + 4;
    assert_eq!(&bytes[12..16], b"IHDR", "a PNG's first chunk is IHDR");
    let data = b"Software\0An editor";
    let mut chunk = u32::try_from(data.len())
        .expect("the chunk is short")
        .to_be_bytes()
        .to_vec();
    chunk.extend(b"tEXt");
    chunk.extend(data);
    chunk.extend([0; 4]);
    [&bytes[..end_of_header], &chunk, &bytes[end_of_header..]].concat()
}

#[test]
fn both_pilots_pass_every_docs_check_in_the_release_form() {
    for pilot in [Pilot::spareline(), Pilot::teamdiff()] {
        let workspace = pilot.workspace();
        let mut violations = readme::check(&workspace).unwrap();
        violations.extend(docs_index::check(&workspace).unwrap());
        violations.extend(docs_layout::check(&workspace).unwrap());
        assert!(
            violations.is_empty(),
            "a pilot's readme, written by hand, passes as a release checks it: {violations:#?}"
        );
    }
}

#[test]
fn a_missing_interfaces_setting_fails() {
    let pilot = Pilot::spareline();
    pilot.manifest("readme_sections = [\"Quick start\", \"Documentation\", \"License\"]\n");
    pilot.assert_breaks(
        "Cargo.toml",
        Fails::Always,
        &["`interfaces` isn't set in `[workspace.metadata.policy]`"],
    );
}

#[test]
fn readme_sections_without_the_three_parts_in_order_fail() {
    let pilot = Pilot::spareline();
    pilot.manifest(
        "interfaces = [\"web\"]\nreadme_sections = [\"Documentation\", \"Quick start\", \
         \"License\"]\n",
    );
    pilot.assert_breaks(
        "Cargo.toml",
        Fails::Always,
        &[
            "lists `Documentation`, `Quick start` and `License`, but a readme has `Quick start`, \
           `Documentation` and `License`, in that order",
        ],
    );
}

#[test]
fn a_library_without_a_license_needs_no_license_section() {
    let pilot = Pilot::spareline();
    pilot.write(
        "Cargo.toml",
        "[workspace]\nresolver = \"3\"\nmembers = [\"crates/*\"]\n\n\
         [workspace.metadata.policy]\ninterfaces = [\"library\"]\n\
         readme_sections = [\"Quick start\", \"Documentation\"]\n",
    );
    assert!(
        pilot.readme().is_empty(),
        "a library without a license has no License section: {:#?}",
        pilot.readme()
    );
    pilot.replace("Cargo.toml", "[\"library\"]", "[\"cli\"]");
    pilot.assert_breaks(
        "Cargo.toml",
        Fails::Always,
        &["`Quick start`, `Documentation` and `License`"],
    );
}

#[test]
fn a_license_file_the_license_section_doesnt_link_fails() {
    let pilot = Pilot::spareline();
    pilot.replace(
        "README.md",
        "[Apache License 2.0](LICENSE-APACHE)",
        "Apache License 2.0",
    );
    pilot.assert_breaks(
        "README.md",
        Fails::Always,
        &[
            "The `## License` section doesn't link `LICENSE-APACHE`, which `license = \"MIT OR \
             Apache-2.0\"` in `[workspace.package]` names.",
        ],
    );
    pilot.replace("README.md", "(LICENSE-MIT)", "(./LICENSE-MIT#top)");
    pilot.replace(
        "README.md",
        "Apache License 2.0,",
        "<a href=\"LICENSE-APACHE\">Apache License 2.0</a>,",
    );
    assert!(
        pilot.readme().is_empty(),
        "a `./` prefix, an anchor and an HTML link still link the file: {:#?}",
        pilot.readme()
    );
}

#[test]
fn a_missing_image_fails_in_the_readme_or_a_page() {
    let pilot = Pilot::spareline();
    pilot.remove("docs/screenshots/sign-in.png");
    pilot.assert_breaks(
        "docs/web-app.md",
        Fails::Always,
        &[
            "shows the image `screenshots/sign-in.png`, which is `docs/screenshots/sign-in.png` \
             from the root, but there is no such file.",
        ],
    );
    let pilot = Pilot::teamdiff();
    pilot.remove("docs/screenshots/in-game.svg");
    let violations = pilot.readme();
    let files: Vec<&str> = violations.iter().map(Violation::file).collect();
    assert_eq!(
        files,
        ["README.md", "docs/usage.md"],
        "the hero and its gallery row both show the missing image: {violations:#?}"
    );
    assert!(
        violations[0]
            .message()
            .contains("shows the image `docs/screenshots/in-game.svg`, but there is no such file"),
        "the readme's message names the image: {}",
        violations[0]
    );
}

#[test]
fn a_png_with_metadata_or_that_isnt_a_png_fails() {
    let pilot = Pilot::spareline();
    let real = fs::read(pilot.path().join("docs/screenshots/sign-in.png")).unwrap();
    pilot.write("docs/screenshots/sign-in.png", with_text_chunk(&real));
    pilot.assert_breaks(
        "docs/screenshots/sign-in.png",
        Fails::Always,
        &[
            "It carries the `tEXt` chunk",
            "`oxipng --strip safe docs/screenshots/sign-in.png`",
        ],
    );
    pilot.write("docs/screenshots/sign-in.png", "<svg/>");
    pilot.assert_breaks(
        "docs/screenshots/sign-in.png",
        Fails::Always,
        &["isn't a whole PNG file: it doesn't start with the PNG signature"],
    );
}

#[test]
fn a_hero_without_alt_text_fails() {
    let pilot = Pilot::spareline();
    let readme = pilot.read("README.md");
    let alt_start = readme.find("[![The Messages page").unwrap() + 3;
    let alt_end = alt_start + readme[alt_start..].find("](docs/screenshots").unwrap();
    pilot.write(
        "README.md",
        format!("{}{}", &readme[..alt_start], &readme[alt_end..]),
    );
    pilot.assert_breaks(
        "README.md",
        Fails::Always,
        &["shows the image `docs/screenshots/messages.png`, linked to a page in `docs/` before the first `##`, with no alt text"],
    );
}

#[test]
fn an_html_hero_without_alt_text_fails() {
    let pilot = Pilot::spareline();
    let readme = pilot.read("README.md");
    let start = readme.find("[![The Messages page").unwrap();
    let end = readme.find("\n\nSee every page").unwrap();
    let hero =
        "[<img src=\"docs/screenshots/messages.png\" width=\"720\">](docs/web-app.md#screenshots)";
    pilot.write(
        "README.md",
        format!("{}{hero}{}", &readme[..start], &readme[end..]),
    );
    pilot.assert_breaks(
        "README.md",
        Fails::Always,
        &["shows the image `docs/screenshots/messages.png`, linked to a page in `docs/` before the first `##`, with no alt text"],
    );
}

/// Only the images that link a page in `docs/`, the hero candidates, need alt text: a decorative
/// logo with `alt=""` and a badge with an empty one may stay as they are.
#[test]
fn a_decorative_image_and_a_badge_may_have_empty_alt_text() {
    let pilot = Pilot::spareline();
    pilot.write(
        "docs/screenshots/logo.png",
        fs::read(pilot.path().join("docs/screenshots/sign-in.png")).unwrap(),
    );
    pilot.replace(
        "README.md",
        "# spareline\n\n",
        "# spareline\n\n<p><img src=\"docs/screenshots/logo.png\" alt=\"\" width=\"64\"></p>\n\n\
         [![](https://img.shields.io/badge/demo-yes-green)](https://example.com/demo)\n\n",
    );
    assert!(
        pilot.readme().is_empty(),
        "a decorative image and a badge link no page in `docs/`: {:#?}",
        pilot.readme()
    );
}

/// A gallery's heading may carry an emoji before it and another case, as `## 📸 Screenshots`.
#[test]
fn a_gallery_heading_with_an_emoji_counts() {
    let pilot = Pilot::teamdiff();
    pilot.replace("docs/usage.md", "## Screenshots\n", "## 📸 screenshots\n");
    pilot.replace(
        "README.md",
        "](docs/usage.md#screenshots)\n\nEvery",
        "](docs/usage.md)\n\nEvery",
    );
    assert!(
        pilot.readme().is_empty(),
        "`## 📸 screenshots` makes the page a gallery a hero may link without an anchor: {:#?}",
        pilot.readme()
    );
}

#[test]
fn a_hero_linking_a_page_that_doesnt_exist_fails_only_a_release() {
    let pilot = Pilot::spareline();
    pilot.replace(
        "README.md",
        "](docs/web-app.md#screenshots)\n\nSee every page",
        "](docs/gallery.md#screenshots)\n\nSee every page",
    );
    pilot.assert_breaks(
        "README.md",
        Fails::Release,
        &["links `docs/gallery.md`, which doesn't exist"],
    );
}

#[test]
fn a_gallery_the_index_doesnt_link_fails() {
    let pilot = Pilot::teamdiff();
    pilot.replace(
        "README.md",
        "- [Use teamdiff](docs/usage.md): run it, its keys and options, and how it updates \
         itself.\n- [Screenshots](docs/usage.md#screenshots): every screen, from waiting for the \
         game to the post-game scoreboard.\n",
        "",
    );
    pilot.assert_breaks(
        "README.md",
        Fails::Always,
        &[
            "`docs/usage.md` is a gallery, a page titled `# Screenshots` or with a \
             `## Screenshots` section, but the `## Documentation` section doesn't link it",
        ],
    );
}

#[test]
fn a_prompt_left_in_a_page_fails_only_a_release() {
    let pilot = Pilot::spareline();
    pilot.replace(
        "docs/web-app.md",
        "## Screenshots\n",
        "## Screenshots\n\n> **To write:** the line that says the data is invented.\n",
    );
    pilot.assert_breaks(
        "docs/web-app.md",
        Fails::Release,
        &["Line 22 holds a prompt of the outline trellis wrote"],
    );
}

/// The mutation removes the sentence after the hero too, since its words outside its link would
/// open the readme as well.
#[test]
fn a_readme_without_an_opening_paragraph_fails_only_a_release() {
    let pilot = Pilot::teamdiff();
    let readme = pilot.read("README.md");
    let start = readme.find("teamdiff shows you").unwrap();
    let end = readme.find("- **Agent select:**").unwrap();
    let readme = format!("{}{}", &readme[..start], &readme[end..]);
    let start = readme.find("Every other screen").unwrap();
    let end = readme.find("> [!NOTE]").unwrap();
    let readme = format!("{}{}", &readme[..start], &readme[end..]);
    pilot.write("README.md", &readme);
    pilot.assert_breaks(
        "README.md",
        Fails::Release,
        &["The readme has no opening paragraph before its first `##`"],
    );
    let centred = "<p align=\"center\">teamdiff shows you who is in your lobby.</p>\n\n";
    pilot.write(
        "README.md",
        readme.replacen(
            "- **Agent select:**",
            &format!("{centred}- **Agent select:**"),
            1,
        ),
    );
    assert!(
        pilot.readme().is_empty(),
        "an HTML paragraph with words opens the readme too: {:#?}",
        pilot.readme()
    );
}

#[test]
fn a_ui_readme_without_its_hero_image_fails_only_a_release() {
    let pilot = Pilot::spareline();
    let readme = pilot.read("README.md");
    let start = readme.find("[![The Messages page").unwrap();
    let end = readme.find("See every page").unwrap();
    pilot.write(
        "README.md",
        format!("{}{}", &readme[..start], &readme[end..]),
    );
    pilot.assert_breaks(
        "README.md",
        Fails::Release,
        &["The readme has no picture before its first `##`: no image links a page in `docs/`"],
    );
    pilot.replace(
        "Cargo.toml",
        "interfaces = [\"web\"]",
        "interfaces = [\"service\", \"web\"]",
    );
    assert!(
        pilot.readme().is_empty(),
        "only a UI as the primary interface needs a hero: {:#?}",
        pilot.readme()
    );
}

#[test]
fn a_hero_linking_a_heading_the_gallery_lacks_fails_only_a_release() {
    let pilot = Pilot::teamdiff();
    pilot.replace(
        "README.md",
        "](docs/usage.md#screenshots)\n\nEvery",
        "](docs/usage.md#gallery)\n\nEvery",
    );
    pilot.assert_breaks(
        "README.md",
        Fails::Release,
        &[
            "the image on line 31 links `docs/usage.md#gallery`, but `docs/usage.md` has no \
             heading with that anchor",
        ],
    );
}

/// A hero may link a gallery page without an anchor, as welldone's links `docs/screenshots.md`,
/// a page that is all gallery: one with a `## Screenshots` section or titled `# Screenshots`.
#[test]
fn a_hero_linking_a_gallery_without_an_anchor_passes() {
    let pilot = Pilot::teamdiff();
    pilot.replace(
        "README.md",
        "](docs/usage.md#screenshots)\n\nEvery",
        "](docs/usage.md)\n\nEvery",
    );
    assert!(
        pilot.readme().is_empty(),
        "a page with a `## Screenshots` section is a gallery: {:#?}",
        pilot.readme()
    );
    pilot.write(
        "docs/screenshots.md",
        "# Screenshots\n\nEvery screen of `teamdiff --demo`, so every player is made up.\n\n\
         ![The waiting screen](screenshots/waiting.svg)\n",
    );
    pilot.replace(
        "README.md",
        "](docs/usage.md)\n\nEvery",
        "](docs/screenshots.md)\n\nEvery",
    );
    pilot.assert_breaks(
        "README.md",
        Fails::Always,
        &[
            "`docs/screenshots.md` is a gallery, a page titled `# Screenshots` or with a \
           `## Screenshots` section, but the `## Documentation` section doesn't link it",
        ],
    );
    pilot.replace(
        "README.md",
        "## Documentation\n\n",
        "## Documentation\n\n- [Screenshots](docs/screenshots.md): every screen.\n",
    );
    assert!(
        pilot.readme().is_empty(),
        "a page titled `# Screenshots`, linked from the index, is a gallery a hero may link \
         without an anchor: {:#?}",
        pilot.readme()
    );
}

#[test]
fn a_hero_linking_another_page_without_an_anchor_fails_only_a_release() {
    let pilot = Pilot::spareline();
    pilot.replace(
        "README.md",
        "](docs/web-app.md#screenshots)\n\nSee every page",
        "](docs/install.md)\n\nSee every page",
    );
    pilot.assert_breaks(
        "README.md",
        Fails::Release,
        &["links `docs/install.md` without an anchor, but that page isn't a gallery"],
    );
}

/// The hero needs no text link beside it: steam-tracker's sentence under its hero says "Select
/// the image to open the gallery" without a link of its own.
#[test]
fn a_hero_needs_no_text_link_to_its_gallery() {
    let pilot = Pilot::spareline();
    let readme = pilot.read("README.md");
    let (head, rest) = readme.split_at(readme.find("## Quick start").unwrap());
    let head = head
        .replace("[Use the web app](docs/web-app.md)", "Use the web app")
        .replace("[Pages](docs/web-app.md#pages)", "Pages")
        .replace(
            "in [Screenshots](docs/web-app.md#screenshots).",
            "in Screenshots. Select the image to open the gallery.",
        );
    pilot.write("README.md", format!("{head}{rest}"));
    assert!(
        pilot.readme().is_empty(),
        "the image's own link and the index lead to the gallery: {:#?}",
        pilot.readme()
    );
}
