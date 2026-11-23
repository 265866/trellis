//! The root `README.md` and the pages in `docs/` have what a stranger needs: the form that is
//! wrong at any age fails every check, and what a project grows into, its written readme and the
//! picture at its top, fails a release.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use comrak::Arena;
use comrak::nodes::{Node, NodeValue};
use toml::Value;

use crate::docs_index::{self, BYTE_ORDER_MARK, INDEX, README};
use crate::html::{Html, Tag};
use crate::interface::Interface;
use crate::markdown::{self, Markdown};
use crate::policy_error::PolicyError;
use crate::violation::Violation;
use crate::workspace::Workspace;
use crate::{license_files, repository_files, toml_file};

/// The manifest that holds the settings, relative to the root.
const MANIFEST: &str = "Cargo.toml";

/// The readme section that gets a reader from nothing to the project working.
const QUICK_START: &str = "Quick start";

/// The readme section that links the license files.
const LICENSE: &str = "License";

/// The heading of a gallery page's section of screenshots.
const SCREENSHOTS: &str = "Screenshots";

/// The bold words a prompt of the readme outline opens with.
const PROMPT: &str = "To write:";

/// The PNG chunks a screenshot may carry: the ones that draw the image, its colours and its
/// animation. Any other, such as `tEXt`, `eXIf`, `tIME`, `caBX` or a private chunk, can carry
/// metadata such as the software, the author, the time or a place.
const IMAGE_CHUNKS: [&str; 20] = [
    "IHDR", "PLTE", "IDAT", "IEND", "tRNS", "cHRM", "gAMA", "iCCP", "sBIT", "sRGB", "cICP", "mDCV",
    "cLLI", "bKGD", "hIST", "pHYs", "sPLT", "acTL", "fcTL", "fdAT",
];

/// What a Git LFS pointer file starts with, which a checkout without Git LFS holds in place of
/// the file.
const LFS_POINTER: &[u8] = b"version https://git-lfs.github.com/spec/v1";

/// The 8 bytes every PNG file starts with.
const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

/// The chunk that ends a PNG file.
const PNG_END: &[u8; 4] = b"IEND";

/// What every message that fails only a release ends with.
const RELEASE: &str = "`just policy` only notes this, but a release fails on it; `just \
                       policy-release` runs the release check locally.";

/// Checks the readme and the pages in `docs/`, as `interfaces` and `readme_sections` in
/// `[workspace.metadata.policy]` and `license` in `[workspace.package]` set them. It reads
/// Markdown as `docs_layout` does, as GitHub renders it: text in code, in an HTML comment or in
/// HTML other than an `<img>` or `<a>` tag is neither an image nor a link.
///
/// These fail every check, since they are wrong at any age:
///
/// - `interfaces` isn't set. A missing setting is a violation rather than a default, since no
///   default could say what a person who uses the project sees.
/// - `readme_sections` doesn't hold `Quick start`, `Documentation` and `License`, in that order,
///   with any others between them. `License` is left out only for a library with no `license`,
///   as the template leaves it out: a primary interface of `library`, which the template gives a
///   library alone, and no `license` in `[workspace.package]`.
/// - The readme's `## License` section doesn't link each license file the workspace's `license`
///   names, as `license_files` names them, with a relative link: `LICENSE`, or `LICENSE-MIT` and
///   `LICENSE-APACHE`.
/// - A local image that the readme or a page shows, as a Markdown image or an HTML `<img src>`,
///   doesn't exist. A path is relative to the file that shows it; URLs aren't checked.
/// - A PNG under `docs/` carries a chunk other than the ones that draw the image, its colours and
///   its animation, as `IMAGE_CHUNKS` lists them, since another, such as `tEXt`, `eXIf`, `tIME`
///   or a private chunk, can hold private values; or it holds data after its `IEND` chunk, where a
///   cropping tool can leave the uncropped image (CVE-2023-21036); or a file there named `.png`
///   isn't a whole PNG. A Git LFS pointer named `.png` gets a notice that it can't be checked,
///   which no check fails on, since CI's checkout doesn't fetch Git LFS files.
/// - An image before the readme's first `##` that links a page under `docs/`, a hero candidate,
///   has no alt text. Other images there, such as a decorative logo with `alt=""` or a badge, may
///   leave it empty.
/// - A gallery page, a page under `docs/` titled `# Screenshots` or with a `## Screenshots`
///   section, isn't linked from the readme's `## Documentation` section, with or without an
///   anchor.
///
/// These fail only a release, as `trellis-policy check --release` runs it; otherwise the check
/// prints them as notices, since a new project's readme is an outline until someone writes it:
///
/// - A `**To write:**` prompt of the outline, a quote that opens with those bold words, remains
///   in the readme or a page.
/// - The readme has no opening paragraph before its first `##`: a paragraph with words outside
///   badges, images and links.
/// - The primary interface is `web`, `desktop` or `terminal`, and before the readme's first `##`
///   there is no image linked to a page under `docs/` that exists: with an anchor,
///   `docs/<page>.md#<anchor>`, to a page with a heading whose GitHub anchor is `<anchor>`, or
///   without one to a gallery page. That is the hero, which shows what the project looks like and
///   leads to the gallery. The clause can't tell a badge from a picture, so a badge linked to a
///   heading in `docs/` stands in for the hero: it checks that the link is there, not what it
///   shows.
///
/// There is deliberately no clause for a text link to the gallery beside the hero. The image's
/// own link and the gallery's entry under `## Documentation` already lead there, and readmes
/// written without these clauses say it in words that link nothing, such as "Select the image to
/// open the gallery".
///
/// There is deliberately no clause for a sample of a `cli`, `service` or `library` project's
/// output. Requiring one before the first `##` would require a form no pilot readme validated,
/// and requiring one before `## Documentation` could never fail, since any quick start has a code
/// block. The outline's prompt asks for the sample instead.
///
/// Each clause checks that something is there, never its wording or layout, which review judges.
///
/// It reads the files git tracks and the untracked ones `.gitignore` doesn't exclude.
///
/// # Errors
///
/// Fails if the repository's files can't be listed, or the readme, a page or a PNG under `docs/`
/// can't be read.
pub fn check(workspace: &Workspace) -> Result<Vec<Violation>, PolicyError> {
    let root = workspace.root();
    let config = workspace.config();
    let files = repository_files::list(root)?;
    let present: BTreeSet<&str> = files.iter().map(String::as_str).collect();
    let license = toml_file::lookup(workspace.manifest(), &["workspace", "package", "license"])
        .and_then(Value::as_str);
    let interfaces = config.interfaces();
    let mut violations = Vec::new();
    if interfaces.is_none() {
        violations.push(Violation::new(
            MANIFEST.to_owned(),
            "`interfaces` isn't set in `[workspace.metadata.policy]`, so nothing says what a \
             person who uses the project sees, which decides what the readme must show. Set it \
             to what they see, the primary one first, from `web`, `desktop`, `terminal`, `cli`, \
             `service` and `library`, such as `interfaces = [\"cli\"]`."
                .to_owned(),
        ));
    }
    if let Some(sections) = config.readme_sections() {
        let is_library =
            interfaces.is_some_and(|interfaces| interfaces.first() == Some(&Interface::Library));
        let license_section = if license.is_none() && is_library {
            LicenseSection::NotNeeded
        } else {
            LicenseSection::Needed
        };
        violations.extend(sections_problem(sections, license_section));
    }
    let pngs = files
        .iter()
        .filter(|file| docs_index::is_under_docs(file) && is_png(file));
    for file in pngs {
        let path = root.join(file);
        let bytes = fs::read(&path).map_err(|source| PolicyError::Read { path, source })?;
        violations.extend(png_problem(file, &bytes));
    }
    let readme_text = docs_index::read_readme(root)?;
    let mut page_texts = Vec::new();
    let pages = files.iter().filter(|file| docs_index::is_page(file));
    for page in pages {
        page_texts.push((page.as_str(), docs_index::read_page(root, page)?));
    }
    let arena = Arena::new();
    let readme = readme_text
        .as_deref()
        .map(|text| Markdown::parse(&arena, text.trim_start_matches(BYTE_ORDER_MARK)));
    let pages: BTreeMap<&str, Markdown<'_>> = page_texts
        .iter()
        .map(|(page, text)| {
            let text = text.trim_start_matches(BYTE_ORDER_MARK);
            (*page, Markdown::parse(&arena, text))
        })
        .collect();
    let documents = readme
        .iter()
        .map(|readme| (README, readme))
        .chain(pages.iter().map(|(page, markdown)| (*page, markdown)));
    for (file, markdown) in documents {
        violations.extend(missing_images(file, markdown, &present));
        violations.extend(prompts(file, markdown));
    }
    if let Some(readme) = &readme {
        let readme = Readme {
            markdown: readme,
            head: head(readme),
        };
        violations.extend(readme.license_problem(license));
        violations.extend(readme.alt_text_problems());
        violations.extend(readme.unlinked_galleries(&pages));
        violations.extend(readme.opening_problem());
        if interfaces
            .and_then(<[Interface]>::first)
            .is_some_and(|primary| {
                matches!(
                    primary,
                    Interface::Web | Interface::Desktop | Interface::Terminal
                )
            })
        {
            violations.extend(readme.hero_problem(&pages));
        }
    }
    Ok(violations)
}

/// Whether a readme needs a `License` section.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LicenseSection {
    /// It needs one: the workspace has a license, or the project isn't a library.
    Needed,
    /// It needs none: a library without a license.
    NotNeeded,
}

/// What is wrong with `sections`, the `readme_sections` setting, if it doesn't hold
/// `Quick start`, `Documentation` and, when `license` says so, `License`, in that order.
fn sections_problem(sections: &[String], license: LicenseSection) -> Option<Violation> {
    let mut required = vec![QUICK_START, INDEX];
    if license == LicenseSection::Needed {
        required.push(LICENSE);
    }
    let mut wanted = required.iter().peekable();
    for section in sections {
        if wanted.peek().is_some_and(|next| *next == section) {
            wanted.next();
        }
    }
    wanted.peek()?;
    let listed = if sections.is_empty() {
        "no sections".to_owned()
    } else {
        code_list(sections)
    };
    Some(Violation::new(
        MANIFEST.to_owned(),
        format!(
            "`readme_sections` in `[workspace.metadata.policy]` lists {listed}, but a readme has \
             {}, in that order, with any other sections between them. Give the readme those \
             `##` sections, and list them in `readme_sections` in a rule change.",
            code_list(&required)
        ),
    ))
}

/// `items` as code spans, joined with commas and a final "and".
fn code_list(items: &[impl AsRef<str>]) -> String {
    let quoted: Vec<String> = items
        .iter()
        .map(|item| format!("`{}`", item.as_ref()))
        .collect();
    match quoted.split_last() {
        None => String::new(),
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
    }
}

/// Whether `file` has the `.png` extension, in any case.
fn is_png(file: &str) -> bool {
    file.rsplit_once('.')
        .is_some_and(|(_, extension)| extension.eq_ignore_ascii_case("png"))
}

/// The violation of `file`, a PNG under `docs/` whose contents are `bytes`, if it isn't a whole
/// PNG, carries a chunk other than [`IMAGE_CHUNKS`], or holds data after its `IEND` chunk. A PNG
/// is an 8-byte signature, then chunks, each a 4-byte big-endian length, a 4-byte type, the data
/// and a 4-byte CRC, up to the `IEND` chunk. A Git LFS pointer gets a notice that it can't be
/// checked, since a checkout without Git LFS, such as CI's, holds the pointer.
fn png_problem(file: &str, bytes: &[u8]) -> Option<Violation> {
    let not_png = |why: &str| {
        Some(Violation::new(
            file.to_owned(),
            format!(
                "It has the `.png` extension but isn't a whole PNG file: {why}. Save it as a PNG, \
                 or give it the extension of its format."
            ),
        ))
    };
    if bytes.starts_with(LFS_POINTER) {
        return Some(Violation::notice(
            file.to_owned(),
            "It is a Git LFS pointer, so the check can't read the image for metadata; a checkout \
             without Git LFS, such as CI's, holds the pointer in place of the file."
                .to_owned(),
        ));
    }
    let Some(mut chunks) = bytes.strip_prefix(PNG_SIGNATURE) else {
        return not_png("it doesn't start with the PNG signature");
    };
    let mut found: Vec<String> = Vec::new();
    let after_end = loop {
        let Some((length, rest)) = chunks.split_first_chunk::<4>() else {
            return not_png("it ends before its `IEND` chunk");
        };
        let Some((kind, rest)) = rest.split_first_chunk::<4>() else {
            return not_png("it ends before its `IEND` chunk");
        };
        let Some(rest) = usize::try_from(u32::from_be_bytes(*length))
            .ok()
            .and_then(|length| length.checked_add(4))
            .and_then(|length| rest.get(length..))
        else {
            return not_png("a chunk runs past the end of the file");
        };
        let name = String::from_utf8_lossy(kind).into_owned();
        if !IMAGE_CHUNKS.contains(&name.as_str()) && !found.contains(&name) {
            found.push(name);
        }
        if kind == PNG_END {
            break rest.len();
        }
        chunks = rest;
    };
    let mut problems = Vec::new();
    if !found.is_empty() {
        let noun = if found.len() == 1 { "chunk" } else { "chunks" };
        problems.push(format!(
            "It carries the {} {noun}, which aren't image data and can carry metadata such as \
             the software, the author, the time or a place.",
            code_list(&found)
        ));
    }
    if after_end > 0 {
        problems.push(format!(
            "It holds {after_end} bytes after its `IEND` chunk, where a cropping tool can leave \
             the image before it was cropped (CVE-2023-21036, \"aCropalypse\")."
        ));
    }
    if problems.is_empty() {
        return None;
    }
    Some(Violation::new(
        file.to_owned(),
        format!(
            "{} Strip it, such as with `oxipng --strip safe {file}`, which keeps only the image.",
            problems.join(" ")
        ),
    ))
}

/// An image that a readme or a page shows.
#[derive(Debug)]
struct Image {
    /// The 1-based line it is on.
    line: usize,
    /// Its source, as written: a Markdown image's destination or an `<img>` tag's `src`.
    source: String,
    /// Its alt text, or `None` for an `<img>` tag without an `alt`.
    alt: Option<String>,
}

/// The images in `node` and the nodes inside it: Markdown images, and `<img>` tags in HTML
/// blocks and inline HTML. HTML that doesn't close, which `docs_layout` reports, is skipped.
fn images(node: Node<'_>) -> Vec<Image> {
    let mut images = Vec::new();
    for descendant in node.descendants() {
        images.extend(markdown_image(descendant));
        if let Some(html) = markdown::html(descendant) {
            for tag in Html::new(&html).tags().unwrap_or_default() {
                images.extend(tag_image(descendant, &html, &tag));
            }
        }
    }
    images
}

/// The image `node` is, if it is a Markdown image.
fn markdown_image(node: Node<'_>) -> Option<Image> {
    let NodeValue::Image(link) = &node.data.borrow().value else {
        return None;
    };
    Some(Image {
        line: markdown::first_line(node),
        source: link.url.clone(),
        alt: Some(node.children().map(markdown::text).collect()),
    })
}

/// The image `tag` shows, if it is an `<img>` tag with a `src`, in `html`, the HTML of `node`.
fn tag_image(node: Node<'_>, html: &str, tag: &Tag) -> Option<Image> {
    if tag.name != "img" || tag.closing {
        return None;
    }
    Some(Image {
        line: markdown::first_line(node) + markdown::line_breaks(&html[..tag.offset]),
        source: tag.attribute("src")?.to_owned(),
        alt: tag.attribute("alt").map(str::to_owned),
    })
}

/// Where a link or image source in a file in `directory` points.
#[derive(Debug, PartialEq, Eq)]
enum Target {
    /// Somewhere outside the repository: a URL, a `data:` URI, or an anchor in the same file.
    Elsewhere,
    /// A file in the repository.
    File {
        /// Its path, relative to the root.
        path: String,
        /// The anchor after the path, percent-decoded, if there is one.
        anchor: Option<String>,
    },
    /// A path that names no file in the repository, such as one that leaves it.
    Nowhere,
}

/// Where `source`, a link or image source in a file in `directory`, points. A path that starts
/// with `/` starts at the root, as GitHub reads it; a query and `.` parts are left out.
fn target(directory: &str, source: &str) -> Target {
    let source = source.trim();
    let (path, anchor) = source
        .split_once('#')
        .map_or((source, None), |(path, anchor)| (path, Some(anchor)));
    let path = path.split_once('?').map_or(path, |(path, _)| path);
    let before_path = path.split('/').next().unwrap_or(path);
    if path.is_empty() || before_path.contains(':') || path.starts_with("//") {
        return Target::Elsewhere;
    }
    let decoded = percent_decoded(path);
    let parts: Vec<&str> = decoded.split('/').filter(|part| *part != ".").collect();
    let path = parts.join("/");
    let resolved = match path.strip_prefix('/') {
        Some(from_root) => docs_index::resolve("", from_root),
        None => docs_index::resolve(directory, &path),
    };
    match resolved {
        Some(path) => Target::File {
            path,
            anchor: anchor.map(percent_decoded),
        },
        None => Target::Nowhere,
    }
}

/// `text` with each `%` and two hex digits replaced by the byte they name, read as UTF-8.
fn percent_decoded(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while let Some(&byte) = bytes.get(index) {
        let hex = bytes
            .get(index + 1..index + 3)
            .and_then(|digits| str::from_utf8(digits).ok())
            .and_then(|digits| u8::from_str_radix(digits, 16).ok());
        match hex {
            Some(value) if byte == b'%' => {
                decoded.push(value);
                index += 3;
            }
            _ => {
                decoded.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

/// The directory of `file`, relative to the root and without a trailing `/`, or `""` at the
/// root.
fn directory(file: &str) -> &str {
    file.rsplit_once('/').map_or("", |(directory, _)| directory)
}

/// The violations of `file`, parsed as `markdown`, for each local image it shows that isn't
/// one of the repository's files, `present`.
fn missing_images(file: &str, markdown: &Markdown<'_>, present: &BTreeSet<&str>) -> Vec<Violation> {
    let mut violations = Vec::new();
    for block in markdown.blocks() {
        for image in images(block) {
            let missing = match target(directory(file), &image.source) {
                Target::Elsewhere => None,
                Target::File { path, .. } if present.contains(path.as_str()) => None,
                Target::File { path, .. } if path == image.source => Some(String::new()),
                Target::File { path, .. } => Some(format!(", which is `{path}` from the root")),
                Target::Nowhere => Some(", which leaves the repository".to_owned()),
            };
            if let Some(resolved) = missing {
                violations.push(Violation::new(
                    file.to_owned(),
                    format!(
                        "Line {} shows the image `{}`{resolved}, but there is no such file. Add \
                         the image, or fix the path, which is relative to this file.",
                        image.line, image.source
                    ),
                ));
            }
        }
    }
    violations
}

/// The violation of `file`, parsed as `markdown`, if a prompt of the readme outline remains in
/// it: a quote whose first paragraph opens with the bold words `To write:`.
fn prompts(file: &str, markdown: &Markdown<'_>) -> Option<Violation> {
    let mut lines = Vec::new();
    for block in markdown.blocks() {
        for node in block.descendants() {
            let is_quote = matches!(node.data.borrow().value, NodeValue::BlockQuote);
            let opens_with_prompt = node
                .first_child()
                .filter(|first| matches!(first.data.borrow().value, NodeValue::Paragraph))
                .and_then(comrak::arena_tree::Node::first_child)
                .filter(|first| matches!(first.data.borrow().value, NodeValue::Strong(_)))
                .is_some_and(|strong| markdown::text(strong).trim() == PROMPT);
            if is_quote && opens_with_prompt {
                lines.push(markdown::first_line(node).to_string());
            }
        }
    }
    let (noun, holds) = match lines.len() {
        0 => return None,
        1 => ("Line", "holds a prompt"),
        _ => ("Lines", "hold prompts"),
    };
    Some(Violation::release_only(
        file.to_owned(),
        format!(
            "{noun} {} {holds} of the outline trellis wrote, a quote that opens with \
             `**{PROMPT}**`. Write what each asks for in place of it. {RELEASE}",
            lines.join(", ")
        ),
    ))
}

/// The top-level blocks of `readme` before its first top-level `##` heading.
fn head<'tree>(readme: &Markdown<'tree>) -> Vec<Node<'tree>> {
    readme
        .blocks()
        .into_iter()
        .take_while(|block| markdown::heading(block).is_none_or(|heading| heading.level != 2))
        .collect()
}

/// The links in `blocks` and the nodes inside them: Markdown links, and `<a href>` tags up to
/// their `</a>`, which may wrap a Markdown image or an `<img>` tag in a later block.
fn links(blocks: &[Node<'_>]) -> Vec<Link> {
    let mut links = Vec::new();
    let mut open: Option<Link> = None;
    for block in blocks {
        for node in block.descendants() {
            if let Some(link) = &mut open {
                link.images.extend(markdown_image(node));
            }
            if let NodeValue::Link(link) = &node.data.borrow().value {
                links.push(Link {
                    line: markdown::first_line(node),
                    url: link.url.clone(),
                    images: images(node),
                });
            } else if let Some(html) = markdown::html(node) {
                for tag in Html::new(&html).tags().unwrap_or_default() {
                    if tag.name == "a" && !tag.closing {
                        links.extend(open.take());
                        open = tag.attribute("href").map(|href| Link {
                            line: markdown::first_line(node)
                                + markdown::line_breaks(&html[..tag.offset]),
                            url: href.to_owned(),
                            images: Vec::new(),
                        });
                    } else if tag.name == "a" {
                        links.extend(open.take());
                    } else if let Some(link) = &mut open {
                        link.images.extend(tag_image(node, &html, &tag));
                    }
                }
            }
        }
    }
    links.extend(open);
    links
}

/// A link in a readme.
#[derive(Debug)]
struct Link {
    /// The 1-based line it starts on.
    line: usize,
    /// Its destination, as written.
    url: String,
    /// The images it wraps.
    images: Vec<Image>,
}

impl Link {
    /// Whether it wraps an image.
    fn wraps_image(&self) -> bool {
        !self.images.is_empty()
    }

    /// Where it points, if that is a page under `docs/`: a hero candidate, when it wraps an
    /// image before the readme's first `##`.
    fn docs_page(&self) -> Option<Target> {
        let target = target("", &self.url);
        matches!(&target, Target::File { path, .. } if docs_index::is_page(path)).then_some(target)
    }
}

/// The GitHub anchors of `page`: one for each heading, its text in lowercase without
/// punctuation other than `-` and `_` and with spaces as `-`, and `-1`, `-2` and so on after a
/// repeated one, as `lychee --include-fragments` reads them; and the `id` and `name` of each
/// HTML tag.
fn anchors(page: &Markdown<'_>) -> BTreeSet<String> {
    let mut anchors = BTreeSet::new();
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    for heading in page.headings() {
        let anchor: String = heading
            .text
            .to_lowercase()
            .chars()
            .filter_map(|character| {
                if character.is_alphanumeric() || matches!(character, '-' | '_') {
                    Some(character)
                } else if character.is_whitespace() {
                    Some('-')
                } else {
                    None
                }
            })
            .collect();
        let count = seen.entry(anchor.clone()).or_insert(0);
        anchors.insert(if *count == 0 {
            anchor
        } else {
            format!("{anchor}-{count}")
        });
        *count += 1;
    }
    for block in page.blocks() {
        for node in block.descendants() {
            if let Some(html) = markdown::html(node) {
                for tag in Html::new(&html).tags().unwrap_or_default() {
                    anchors.extend(
                        ["id", "name"]
                            .iter()
                            .filter_map(|attribute| tag.attribute(attribute))
                            .map(str::to_owned),
                    );
                }
            }
        }
    }
    anchors
}

/// The root readme, parsed.
struct Readme<'markdown, 'tree> {
    /// The parsed readme.
    markdown: &'markdown Markdown<'tree>,
    /// Its top-level blocks before its first `##` heading.
    head: Vec<Node<'tree>>,
}

impl Readme<'_, '_> {
    /// The violation if the `## License` sections don't link each license file `license`, the
    /// workspace's license, names. A readme without the section is left to the section checks.
    fn license_problem(&self, license: Option<&str>) -> Option<Violation> {
        let expression = license?;
        let files = license_files::license_files(expression)?;
        let sections = self.markdown.sections(LICENSE);
        if sections.is_empty() {
            return None;
        }
        let linked: BTreeSet<String> = links(&sections.concat())
            .into_iter()
            .filter_map(|link| match target("", &link.url) {
                Target::File { path, .. } => Some(path),
                Target::Elsewhere | Target::Nowhere => None,
            })
            .collect();
        let missing: Vec<&str> = files
            .into_iter()
            .filter(|file| !linked.contains(*file))
            .collect();
        if missing.is_empty() {
            return None;
        }
        let example = match missing[0] {
            "LICENSE-APACHE" => "[Apache License 2.0](LICENSE-APACHE)",
            "LICENSE-MIT" => "[MIT License](LICENSE-MIT)",
            _ if expression == "MIT" => "[MIT License](LICENSE)",
            _ => "[License](LICENSE)",
        };
        Some(Violation::new(
            README.to_owned(),
            format!(
                "The `## {LICENSE}` section doesn't link {}, which `license = \"{expression}\"` in \
                 `[workspace.package]` names. Link each license file with a relative link, such \
                 as `{example}`.",
                code_list(&missing)
            ),
        ))
    }

    /// The violations for the images before the first `##` that link a page under `docs/`, the
    /// hero candidates, and have no alt text. Other images, such as a decorative logo with
    /// `alt=""` or a badge, may leave it empty.
    fn alt_text_problems(&self) -> Vec<Violation> {
        let mut violations = Vec::new();
        let candidates = links(&self.head)
            .into_iter()
            .filter(|link| link.docs_page().is_some());
        for link in candidates {
            for image in link.images {
                if image.alt.as_deref().is_none_or(|alt| alt.trim().is_empty()) {
                    violations.push(Violation::new(
                        README.to_owned(),
                        format!(
                            "Line {} shows the image `{}`, linked to a page in `docs/` before the \
                             first `##`, with no alt text. Say what it shows in the alt text, \
                             such as `![The inbox, with three invented \
                             texts](docs/screenshots/inbox.png)`, or `alt=\"...\"` in an `<img>` \
                             tag.",
                            image.line, image.source
                        ),
                    ));
                }
            }
        }
        violations
    }

    /// The violations for each gallery page in `pages`, as [`is_gallery`] reads one, that no
    /// `## Documentation` section links. A readme without the section is left to the section
    /// checks.
    fn unlinked_galleries(&self, pages: &BTreeMap<&str, Markdown<'_>>) -> Vec<Violation> {
        let sections = self.markdown.sections(INDEX);
        if sections.is_empty() {
            return Vec::new();
        }
        let linked: BTreeSet<String> = sections
            .iter()
            .flatten()
            .flat_map(|block| markdown::link_urls(block))
            .filter_map(|url| docs_index::resolve("", &url))
            .collect();
        let mut violations = Vec::new();
        for (page, markdown) in pages {
            if is_gallery(markdown) && !linked.contains(*page) {
                violations.push(Violation::new(
                    README.to_owned(),
                    format!(
                        "`{page}` is a gallery, a page titled `# {SCREENSHOTS}` or with a \
                         `## {SCREENSHOTS}` section, but the `## {INDEX}` section doesn't link \
                         it, so readers don't find the screenshots. Add an entry such as \
                         `- [{SCREENSHOTS}]({page}): every screen, with screenshots.`"
                    ),
                ));
            }
        }
        violations
    }

    /// The violation if no paragraph before the first `##`, in Markdown or an HTML block such as
    /// a centred `<p>`, has words outside badges, images and links.
    fn opening_problem(&self) -> Option<Violation> {
        let opens = self.head.iter().any(|block| {
            let value = &block.data.borrow().value;
            if let NodeValue::HtmlBlock(html) = value {
                Html::new(&html.literal).has_words_outside("a")
            } else {
                matches!(value, NodeValue::Paragraph) && has_own_words(block)
            }
        });
        (!opens).then(|| {
            Violation::release_only(
                README.to_owned(),
                format!(
                    "The readme has no opening paragraph before its first `##`: a paragraph with \
                     words outside badges, images and links. Write one or two that say who the \
                     project is for, what it does for them and how it differs. {RELEASE}"
                ),
            )
        })
    }

    /// The violation if the head lacks the hero of a project with a UI: an image linked to a
    /// page of `pages`, with an anchor that page has, or without an anchor to a gallery page.
    fn hero_problem(&self, pages: &BTreeMap<&str, Markdown<'_>>) -> Option<Violation> {
        let mut problem = None;
        let image_links = links(&self.head).into_iter().filter(Link::wraps_image);
        for link in image_links {
            let Some(Target::File { path: page, anchor }) = link.docs_page() else {
                continue;
            };
            let line = link.line;
            let found = match (pages.get(page.as_str()), anchor) {
                (None, _) => Err(format!(
                    "the image on line {line} links `{page}`, which doesn't exist"
                )),
                (Some(markdown), None) if !is_gallery(markdown) => Err(format!(
                    "the image on line {line} links `{page}` without an anchor, but that page \
                     isn't a gallery, titled `# {SCREENSHOTS}` or with a `## {SCREENSHOTS}` \
                     section; link its screenshots' heading, such as `{page}#screenshots`"
                )),
                (Some(markdown), Some(anchor)) if !anchors(markdown).contains(&anchor) => {
                    Err(format!(
                        "the image on line {line} links `{page}#{anchor}`, but `{page}` has no \
                         heading with that anchor"
                    ))
                }
                (Some(_), _) => Ok(()),
            };
            match found {
                Ok(()) => return None,
                Err(found) => {
                    problem.get_or_insert(found);
                }
            }
        }
        let problem = problem.unwrap_or_else(|| "no image links a page in `docs/`".to_owned());
        Some(Violation::release_only(
            README.to_owned(),
            format!(
                "The readme has no picture before its first `##`: {problem}. A project with a UI \
                 shows its main screen there as an image that links its place in the gallery, \
                 such as `[![What it shows, with invented \
                 data](docs/screenshots/main.png)](docs/usage.md#screenshots)`. {RELEASE}"
            ),
        ))
    }
}

/// Whether `page` is a gallery: its `#` title, or one of its top-level `##` headings, is
/// `Screenshots` in any case, after any emoji or other symbols before it, such as
/// `## 📸 Screenshots`.
fn is_gallery(page: &Markdown<'_>) -> bool {
    page.headings().iter().any(|heading| {
        let text = heading
            .text
            .trim_start_matches(|character: char| !character.is_alphanumeric())
            .trim_end();
        matches!(heading.level, 1 | 2)
            && heading.top_level
            && text.eq_ignore_ascii_case(SCREENSHOTS)
    })
}

/// Whether `node` has a letter in its text outside images and links.
fn has_own_words(node: Node<'_>) -> bool {
    node.children().any(|child| {
        let value = &child.data.borrow().value;
        if matches!(value, NodeValue::Link(_) | NodeValue::Image(_)) {
            false
        } else if let NodeValue::Text(text) = value {
            text.chars().any(char::is_alphabetic)
        } else if let NodeValue::Code(code) = value {
            code.literal.chars().any(char::is_alphabetic)
        } else {
            has_own_words(child)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::violation::Fails;

    /// A chunk of a PNG that a test writes.
    struct Chunk<'data> {
        /// Its type, such as `IHDR`.
        kind: &'data [u8; 4],
        /// Its data.
        data: &'data [u8],
    }

    /// The chunk of type `kind` that holds `data`.
    fn chunk<'data>(kind: &'data [u8; 4], data: &'data [u8]) -> Chunk<'data> {
        Chunk { kind, data }
    }

    /// A PNG of `chunks` after the signature, with CRCs of zeros, which the check doesn't read.
    fn png(chunks: &[Chunk<'_>]) -> Vec<u8> {
        let mut bytes = PNG_SIGNATURE.to_vec();
        for Chunk { kind, data } in chunks {
            let length = u32::try_from(data.len()).unwrap();
            bytes.extend(length.to_be_bytes());
            bytes.extend(kind.as_slice());
            bytes.extend(*data);
            bytes.extend([0; 4]);
        }
        bytes
    }

    /// The message of the violation `png_problem` gives `bytes`, or `None` if it gives none.
    fn png_message(bytes: &[u8]) -> Option<String> {
        png_problem("docs/a.png", bytes).map(|violation| violation.message().to_owned())
    }

    #[test]
    fn a_png_of_image_chunks_passes() {
        let mut chunks = vec![chunk(b"IHDR", &[0; 13])];
        let middle = IMAGE_CHUNKS
            .iter()
            .filter(|name| !matches!(**name, "IHDR" | "IEND"));
        for name in middle {
            let kind: &[u8; 4] = name.as_bytes().try_into().unwrap();
            chunks.push(chunk(kind, b"x"));
        }
        chunks.push(chunk(b"IEND", b""));
        assert_eq!(
            png_message(&png(&chunks)),
            None,
            "chunks that draw the image, its colours and its animation hold no metadata"
        );
    }

    #[test]
    fn each_other_chunk_is_named_once() {
        for name in [
            "tEXt", "iTXt", "zTXt", "eXIf", "tIME", "caBX", "zxIf", "prVt",
        ] {
            let other: &[u8; 4] = name.as_bytes().try_into().unwrap();
            let bytes = png(&[
                chunk(b"IHDR", &[0; 13]),
                chunk(other, b"a"),
                chunk(other, b"b"),
                chunk(b"IEND", b""),
            ]);
            let problem = png_message(&bytes).unwrap();
            assert!(
                problem.contains(&format!("carries the `{name}` chunk,"))
                    && problem.contains("oxipng --strip safe docs/a.png"),
                "{name} should be named once, with how to strip it: {problem}"
            );
        }
        let bytes = png(&[
            chunk(b"tIME", b"t"),
            chunk(b"tEXt", b"a"),
            chunk(b"IEND", b""),
        ]);
        assert!(
            png_message(&bytes)
                .unwrap()
                .contains("carries the `tIME` and `tEXt` chunks"),
            "two chunks are named in the order the file holds them"
        );
    }

    #[test]
    fn a_file_that_isnt_a_whole_png_fails() {
        let whole = png(&[chunk(b"IHDR", &[0; 13]), chunk(b"IEND", b"")]);
        for (bytes, why) in [
            (b"GIF89a".to_vec(), "doesn't start with the PNG signature"),
            (
                whole[..whole.len() - 12].to_vec(),
                "ends before its `IEND` chunk",
            ),
            (
                whole[..20].to_vec(),
                "a chunk runs past the end of the file",
            ),
            (PNG_SIGNATURE.to_vec(), "ends before its `IEND` chunk"),
        ] {
            let problem = png_message(&bytes).unwrap();
            assert!(problem.contains(why), "{why}: {problem}");
        }
    }

    #[test]
    fn data_after_the_end_fails() {
        let whole = png(&[chunk(b"IHDR", &[0; 13]), chunk(b"IEND", b"")]);
        let after_end = [whole, b"the uncropped image".to_vec()].concat();
        let problem = png_message(&after_end).unwrap();
        assert!(
            problem.contains("It holds 19 bytes after its `IEND` chunk")
                && problem.contains("CVE-2023-21036")
                && problem.contains("oxipng --strip safe docs/a.png"),
            "bytes after IEND can be the image before it was cropped: {problem}"
        );
    }

    #[test]
    fn a_git_lfs_pointer_is_a_notice() {
        let pointer = b"version https://git-lfs.github.com/spec/v1\noid sha256:abc\nsize 12\n";
        let violation = png_problem("docs/a.png", pointer).unwrap();
        assert_eq!(
            violation.fails(),
            Fails::Never,
            "no check fails on a pointer"
        );
        assert!(
            violation.message().contains("It is a Git LFS pointer"),
            "the notice says why the image isn't checked: {violation}"
        );
    }

    #[test]
    fn sources_resolve_from_their_file() {
        let file = |path: &str, anchor: Option<&str>| Target::File {
            path: path.to_owned(),
            anchor: anchor.map(str::to_owned),
        };
        for (directory, source, expected) in [
            (
                "docs",
                "screenshots/a.png",
                file("docs/screenshots/a.png", None),
            ),
            ("", "./docs/a%20b.png?raw=true", file("docs/a b.png", None)),
            ("docs", "/docs/x.png", file("docs/x.png", None)),
            (
                "",
                "docs/./screenshots/a.png",
                file("docs/screenshots/a.png", None),
            ),
            (
                "",
                "docs/usage.md#screen%2Dshots",
                file("docs/usage.md", Some("screen-shots")),
            ),
            ("docs", "../../x.png", Target::Nowhere),
            ("", "https://example.com/a.png", Target::Elsewhere),
            ("", "data:image/png;base64,AAAA", Target::Elsewhere),
            ("", "//example.com/a.png", Target::Elsewhere),
            ("", "#screenshots", Target::Elsewhere),
        ] {
            assert_eq!(
                target(directory, source),
                expected,
                "{source} in {directory:?}"
            );
        }
    }

    #[test]
    fn markdown_and_html_images_are_found_with_their_alt_text() {
        let arena = Arena::new();
        let page = Markdown::parse(
            &arena,
            "# T\n\n![The *inbox*](a.png) and <img src=\"b.png\"> and <img src=c.png alt=\"C\">\n\n\
             <p>\n<img src=\"d.png\" alt=\"\">\n</p>\n\n<!-- <img src=\"e.png\"> -->\n\n\
             `![f](f.png)`\n",
        );
        let found: Vec<(usize, String, Option<String>)> = page
            .blocks()
            .into_iter()
            .flat_map(images)
            .map(|image| (image.line, image.source, image.alt))
            .collect();
        assert_eq!(
            found,
            [
                (3, "a.png".to_owned(), Some("The inbox".to_owned())),
                (3, "b.png".to_owned(), None),
                (3, "c.png".to_owned(), Some("C".to_owned())),
                (6, "d.png".to_owned(), Some(String::new())),
            ],
            "images in comments and code don't count"
        );
    }

    #[test]
    fn anchors_follow_github_and_lychee() {
        let arena = Arena::new();
        let page = Markdown::parse(
            &arena,
            "# Use it\n\n## Screenshots\n\n## Keys & `options`\n\n## Screenshots\n\n\
             ### Über_alles 2\n\n<a id=\"top\"></a>\n",
        );
        let found: Vec<String> = anchors(&page).into_iter().collect();
        assert_eq!(
            found,
            [
                "keys--options",
                "screenshots",
                "screenshots-1",
                "top",
                "use-it",
                "über_alles-2"
            ],
            "every heading and HTML id gives an anchor"
        );
    }

    #[test]
    fn sections_need_quick_start_documentation_and_license_in_order() {
        let sections = |names: &[&str]| {
            names
                .iter()
                .map(|name| (*name).to_owned())
                .collect::<Vec<_>>()
        };
        for passing in [
            sections(&["Quick start", "Documentation", "License"]),
            sections(&[
                "Quick start",
                "Features",
                "Documentation",
                "FAQ",
                "License",
                "Credits",
            ]),
        ] {
            assert!(
                sections_problem(&passing, LicenseSection::Needed).is_none(),
                "{passing:?} should pass"
            );
        }
        for failing in [
            sections(&["Install", "Documentation", "License"]),
            sections(&["Documentation", "Quick start", "License"]),
            sections(&["Quick start", "Documentation"]),
            sections(&[]),
        ] {
            assert!(
                sections_problem(&failing, LicenseSection::Needed).is_some(),
                "{failing:?} should fail"
            );
        }
        assert!(
            sections_problem(
                &sections(&["Quick start", "Documentation"]),
                LicenseSection::NotNeeded
            )
            .is_none(),
            "a library without a license needs no License section"
        );
        let problem = sections_problem(&sections(&["Install"]), LicenseSection::Needed).unwrap();
        assert!(
            problem.message().contains(
                "lists `Install`, but a readme has `Quick start`, `Documentation` and `License`"
            ),
            "the message names what is listed and what is needed: {problem}"
        );
    }

    #[test]
    fn links_wrap_markdown_and_html_images() {
        let arena = Arena::new();
        let page = Markdown::parse(
            &arena,
            "[![A](a.png)](docs/a.md#x) [text](docs/b.md)\n\n\
             <a href=\"docs/c.md#y\"><img src=\"c.png\" alt=\"C\"></a> <a href=d.md>D</a>\n\n\
             <a href=\"docs/e.md#z\">\n\n![E](e.png)\n\n</a>\n",
        );
        let found: Vec<(bool, String)> = links(&page.blocks())
            .into_iter()
            .map(|link| (link.wraps_image(), link.url))
            .collect();
        assert_eq!(
            found,
            [
                (true, "docs/a.md#x".to_owned()),
                (false, "docs/b.md".to_owned()),
                (true, "docs/c.md#y".to_owned()),
                (false, "d.md".to_owned()),
                (true, "docs/e.md#z".to_owned()),
            ],
            "a link wraps an image when one is inside it, in Markdown or HTML"
        );
    }

    #[test]
    fn own_words_leave_out_links_and_images() {
        let arena = Arena::new();
        let page = Markdown::parse(
            &arena,
            "[![CI](b.svg)](x) [Docs](d.md)\n\n![Shot](s.png)\n\n123 [See](d.md)\n\n\
             See [Docs](d.md).\n\nRun `just ci`\n",
        );
        let found: Vec<bool> = page.blocks().into_iter().map(has_own_words).collect();
        assert_eq!(
            found,
            [false, false, false, true, true],
            "only words outside links and images count"
        );
    }
}
