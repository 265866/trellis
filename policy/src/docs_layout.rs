//! The docs keep their layout: the files `docs_files` lists exist, the root `README.md` has one
//! title, the sections `readme_sections` lists and an index for its `Documentation` section, and
//! every page in `docs/` opens with its title and a paragraph.

use std::path::Path;

use crate::docs_index::{self, BYTE_ORDER_MARK, INDEX, README};
use crate::markdown::{self, Block, Heading, Line, Markdown};
use crate::policy_error::PolicyError;
use crate::repository_files;
use crate::violation::Violation;
use crate::workspace::Workspace;

/// The manifest that holds the settings, relative to the root.
const MANIFEST: &str = "Cargo.toml";

/// How an entry of the `Documentation` index is written.
const ENTRY_FORM: &str = "`- [Title](path): what the reader does with it.`";

/// The extensions, besides `.md`, that GitHub renders as Markdown, which a page may not use.
const OTHER_MARKDOWN_EXTENSIONS: [&str; 6] = ["markdown", "mdown", "mdwn", "mkd", "mkdn", "mkdown"];

/// What a file that starts with a byte order mark is told.
const BYTE_ORDER_MARK_MESSAGE: &str = "It starts with a byte order mark. Save it as UTF-8 without \
                                       one; the other checks read it as if the mark weren't there.";

/// Checks the layout of the docs, as `docs_files` and `readme_sections` in
/// `[workspace.metadata.policy]` set it:
///
/// - every file `docs_files` lists exists;
/// - the root `README.md` has exactly one `#` heading, its title, on its first line or after
///   only badges and blank lines;
/// - its level-2 headings, outside code, list items and block quotes, are exactly the ones
///   `readme_sections` lists, in order, and it has no heading the comparison would miss: no
///   setext heading, a paragraph underlined with `=` or `-`, no `#` heading in a list item or
///   block quote, and no HTML heading tag such as `<h2>`;
/// - its `## Documentation` section holds only list items of one line each, and each is a
///   relative Markdown link, `: ` and a description with a word in it that ends in `.`;
/// - every page, a `.md` file at any depth under `docs/`, starts with a level-1 heading, a blank
///   line and a paragraph with a word in it, not a heading, list, table, quote, code block,
///   HTML, image or link reference definition;
/// - no file under `docs/` has another Markdown extension, such as `.markdown`, which the checks
///   would skip;
/// - neither the readme nor a page starts with a byte order mark. The other checks read past
///   it, so one file gets one message for it.
///
/// A missing setting is a violation rather than a default, since no default could say which
/// files and sections a project keeps.
///
/// It reads the files git tracks and the untracked ones `.gitignore` doesn't exclude.
///
/// # Errors
///
/// Fails if the repository's files can't be listed, or the readme or a page can't be read.
pub fn check(workspace: &Workspace) -> Result<Vec<Violation>, PolicyError> {
    let config = workspace.config();
    let files = repository_files::list(workspace.root())?;
    let readme = docs_index::read_readme(workspace.root())?;
    let mut violations = Vec::new();
    match config.docs_files() {
        Some(required) => {
            for file in required {
                if !files.contains(file) {
                    violations.push(Violation::new(
                        file.clone(),
                        "`docs_files` in `[workspace.metadata.policy]` lists this file, but it \
                         doesn't exist. Restore it, or remove it from `docs_files` in a rule \
                         change if the project no longer keeps it."
                            .to_owned(),
                    ));
                }
            }
        }
        None => violations.push(unset(
            "docs_files",
            "which docs files the project keeps. Set it to the files that must exist, such as \
             `docs_files = [\"README.md\", \"CONTRIBUTING.md\", \"docs/design.md\"]`.",
        )),
    }
    let sections = config.readme_sections();
    if sections.is_none() {
        violations.push(unset(
            "readme_sections",
            "which sections the root `README.md` has. Set it to the readme's `##` headings in \
             order, such as `readme_sections = [\"Install\", \"Documentation\", \"License\"]`.",
        ));
    }
    match &readme {
        Some(text) => {
            let violation = |message: String| Violation::new(README.to_owned(), message);
            if text.starts_with(BYTE_ORDER_MARK) {
                violations.push(violation(BYTE_ORDER_MARK_MESSAGE.to_owned()));
            }
            let readme = Markdown::new(text.trim_start_matches(BYTE_ORDER_MARK));
            violations.extend(
                readme_problems(&readme, sections)
                    .into_iter()
                    .map(violation),
            );
        }
        None if !config
            .docs_files()
            .is_some_and(|required| required.iter().any(|file| file == README)) =>
        {
            violations.push(Violation::new(
                README.to_owned(),
                format!(
                    "There is no root `{README}`. The docs start with a short readme whose `##` \
                     sections `readme_sections` lists; add one."
                ),
            ));
        }
        None => {}
    }
    let docs = files.iter().filter(|file| docs_index::is_under_docs(file));
    for file in docs {
        if docs_index::is_page(file) {
            let text = docs_index::read_page(workspace.root(), file)?;
            if text.starts_with(BYTE_ORDER_MARK) {
                violations.push(Violation::new(
                    file.clone(),
                    BYTE_ORDER_MARK_MESSAGE.to_owned(),
                ));
            }
            let page = Markdown::new(text.trim_start_matches(BYTE_ORDER_MARK));
            if let Some(problem) = page_problem(&page) {
                violations.push(Violation::new(
                    file.clone(),
                    format!(
                        "{problem}. A page starts with its title as a `# Title` line, then a \
                         blank line and a paragraph that says who the page is for and what it \
                         gets them, before any other heading, list, table or code block."
                    ),
                ));
            }
        } else if let Some(renamed) = renamed_page(file) {
            violations.push(Violation::new(
                file.clone(),
                format!(
                    "Pages in `docs/` use the `.md` extension, which the docs checks read. \
                     Rename it to `{renamed}`."
                ),
            ));
        }
    }
    Ok(violations)
}

/// The violation for the setting `key`, which isn't set, where `fix` says what it decides and
/// how to set it.
fn unset(key: &str, fix: &str) -> Violation {
    Violation::new(
        MANIFEST.to_owned(),
        format!("`{key}` isn't set in `[workspace.metadata.policy]`, so nothing says {fix}"),
    )
}

/// `file` with the `.md` extension, if it has another Markdown extension.
fn renamed_page(file: &str) -> Option<String> {
    let extension = Path::new(file).extension()?.to_str()?;
    OTHER_MARKDOWN_EXTENSIONS
        .iter()
        .any(|other| extension.eq_ignore_ascii_case(other))
        .then(|| format!("{}md", &file[..file.len() - extension.len()]))
}

/// What is wrong with the root readme `readme`: its title, sections other than `sections` if
/// that is set, headings the section check can't see, and an index that isn't one.
fn readme_problems(readme: &Markdown<'_>, sections: Option<&[String]>) -> Vec<String> {
    let mut problems = Vec::new();
    problems.extend(title_problem(readme));
    if let Some(required) = sections {
        let found: Vec<&str> = readme
            .headings()
            .filter(|heading| heading.level() == 2)
            .map(Heading::text)
            .collect();
        if !found.iter().eq(required.iter()) {
            problems.push(format!(
                "The readme's `##` sections are {}, but `readme_sections` in \
                 `[workspace.metadata.policy]` lists {}. Give the readme exactly those sections, \
                 in that order, and move anything longer than a few commands to a page in \
                 `docs/`, or change `readme_sections` in a rule change if the project's sections \
                 change.",
                heading_list(&found),
                heading_list(required)
            ));
        }
    }
    for index in readme.setext_underlines() {
        problems.push(format!(
            "Line {} underlines the line above it, which makes that line a heading the section \
             check doesn't read. Write the heading as a `#` line, such as `## Install`, or \
             leave a blank line above the underline to keep it a thematic break.",
            index + 1
        ));
    }
    for index in readme.contained_headings() {
        problems.push(format!(
            "Line {} puts a heading inside a block quote or list item, where the section check \
             doesn't read it. Make it a `#` line of its own, outside the quote or list, or plain \
             text.",
            index + 1
        ));
    }
    for (index, line) in readme.lines().iter().enumerate() {
        if line.block() != Block::Code
            && has_html_heading(&markdown::without_code_spans(line.text()))
        {
            problems.push(format!(
                "Line {} has an HTML heading tag such as `<h2>`, which the section check doesn't \
                 read. Write the heading as a `#` line.",
                index + 1
            ));
        }
    }
    if let Some(section) = readme.section(INDEX) {
        problems.extend(index_problems(&section));
    }
    problems
}

/// What is wrong with the readme's title, or `None` if it has exactly one `#` heading, on its
/// first line or after only badges and blank lines.
fn title_problem(readme: &Markdown<'_>) -> Option<String> {
    let lines = readme.lines();
    let mut titles = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if let Block::Heading(heading) = line.block()
            && heading.level() == 1
        {
            titles.push(index);
        }
    }
    let [title] = titles[..] else {
        return Some(format!(
            "The readme has {} `#` headings. It has exactly one, its title, on its first line \
             or after only badges and blank lines. Make the others `##` sections or move them \
             to a page in `docs/`.",
            titles.len()
        ));
    };
    let preamble = &lines[..title];
    let only_badges = preamble.iter().all(|line| match line.block() {
        Block::Blank => true,
        Block::Paragraph => markdown::without_images(line.text()).trim().is_empty(),
        _ => false,
    });
    (!only_badges).then(|| {
        "Its `#` title comes after text other than badges and blank lines. Put the title first, \
         with only badges and blank lines before it."
            .to_owned()
    })
}

/// Whether `text` holds an HTML heading tag, `<h1>` to `<h6>` or its closing tag, in any case.
fn has_html_heading(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    ["<h", "</h"].into_iter().any(|opener| {
        lower.match_indices(opener).any(|(start, _)| {
            let after = &lower.as_bytes()[start + opener.len()..];
            matches!(after.first(), Some(b'1'..=b'6'))
                && after
                    .get(1)
                    .is_none_or(|next| matches!(next, b'>' | b' ' | b'\t' | b'/'))
        })
    })
}

/// `headings` as a list of code spans, or `none`.
fn heading_list(headings: &[impl AsRef<str>]) -> String {
    if headings.is_empty() {
        return "none".to_owned();
    }
    let quoted: Vec<String> = headings
        .iter()
        .map(|heading| format!("`{}`", heading.as_ref()))
        .collect();
    quoted.join(", ")
}

/// What is wrong with the `Documentation` section `section`: one message for each list item
/// that isn't an index entry, and one for the first thing in it that isn't a one-line list item.
fn index_problems(section: &Markdown<'_>) -> Vec<String> {
    let mut problems = Vec::new();
    let mut stray: Option<String> = None;
    for line in section.lines() {
        match line.block() {
            Block::Blank => {}
            Block::ListItem(entry) => {
                if let Some(problem) = entry_problem(entry) {
                    problems.push(format!(
                        "The `## {INDEX}` entry `{entry}` isn't an index entry: {problem}. Write \
                         each entry as {ENTRY_FORM}, with a relative path."
                    ));
                }
            }
            Block::Code => {
                stray.get_or_insert_with(|| "a code block".to_owned());
            }
            Block::Heading(_)
            | Block::Paragraph
            | Block::SetextUnderline
            | Block::ThematicBreak
            | Block::Contained(_)
            | Block::Html
            | Block::Other => {
                stray.get_or_insert_with(|| format!("`{}`", line.text().trim()));
            }
        }
    }
    if let Some(stray) = stray {
        problems.push(format!(
            "The `## {INDEX}` section holds {stray}, but it holds only its index, one \
             single-line list item per page or file, written as {ENTRY_FORM}. Move the text to \
             a page in `docs/` and link the page from an entry."
        ));
    }
    problems
}

/// What keeps `entry`, the text of a list item, from being an index entry: a relative Markdown
/// link, `: ` and a description with a word in it that ends in `.`. `None` if it is one.
fn entry_problem(entry: &str) -> Option<&'static str> {
    let Some((title, rest)) = entry
        .strip_prefix('[')
        .and_then(|rest| rest.split_once("]("))
    else {
        return Some("it doesn't start with a link such as `[Install](docs/install.md)`");
    };
    if title.trim().is_empty() {
        return Some("its link has no text");
    }
    let Some((target, rest)) = rest.split_once(')') else {
        return Some("its link has no closing `)`");
    };
    if target.starts_with('<') || target.contains(char::is_whitespace) {
        return Some(
            "its link has a title or angle brackets, which the index doesn't allow; write it as \
             `[Title](path)`",
        );
    }
    if !is_relative(target) {
        return Some("its link isn't a relative path to a file in the repository");
    }
    let Some(description) = rest.strip_prefix(": ") else {
        return Some("the link isn't followed by `: ` and what the reader does with the page");
    };
    if !description.chars().any(char::is_alphabetic) {
        return Some("its description has no words");
    }
    if !description.ends_with('.') {
        return Some("its description doesn't end in `.`");
    }
    None
}

/// Whether the link target `target` is a relative path: no scheme such as `https:`, no leading
/// `/` or `#`, and no `..` part that leaves the repository.
fn is_relative(target: &str) -> bool {
    let before_path = target.split(['/', '#']).next().unwrap_or(target);
    !target.is_empty()
        && !target.split('/').any(|part| part == "..")
        && !target.starts_with(['/', '#'])
        && !before_path.contains(':')
}

/// What keeps `page` from opening with a `# Title` line, a blank line and a paragraph, or `None`
/// if it does.
fn page_problem(page: &Markdown<'_>) -> Option<&'static str> {
    let lines = page.lines();
    let title = lines.first().and_then(|line| match line.block() {
        Block::Heading(heading) => Some(heading),
        _ => None,
    });
    if !title.is_some_and(|title| title.level() == 1 && !title.text().is_empty()) {
        return Some("It doesn't start with a `# Title` line");
    }
    match lines.get(1).copied().map(Line::block) {
        Some(Block::Blank) => {}
        Some(_) => return Some("The line after its title isn't blank"),
        None => return Some("Nothing follows its title"),
    }
    let rest = &lines[2..];
    let length = rest
        .iter()
        .position(|line| line.block() != Block::Paragraph)
        .unwrap_or(rest.len());
    let paragraph = &rest[..length];
    let underlined = rest
        .get(length)
        .is_some_and(|line| line.block() == Block::SetextUnderline);
    let has_words = paragraph.iter().any(|line| has_words(*line));
    if paragraph.is_empty() || underlined || !has_words {
        return Some(
            "Its title isn't followed by a paragraph: the blank line after it is followed by \
             nothing, another blank line, a heading, list, table, quote, code block, HTML, image \
             or link reference definition",
        );
    }
    None
}

/// Whether `line` has a word in it once its images are gone.
fn has_words(line: Line<'_>) -> bool {
    markdown::without_images(line.text())
        .chars()
        .any(char::is_alphabetic)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index(text: &str) -> Vec<String> {
        let readme = format!("# Demo\n\n## Documentation\n\n{text}\n## License\n");
        index_problems(&Markdown::new(&readme).section(INDEX).unwrap())
    }

    fn page(text: &str) -> Option<&'static str> {
        page_problem(&Markdown::new(text))
    }

    fn readme(text: &str) -> Vec<String> {
        readme_problems(&Markdown::new(text), None)
    }

    #[test]
    fn index_entries_pass() {
        assert_eq!(
            index(
                "- [Install](docs/install.md): every way to install it.\n\
                 * [Design](./docs/design.md#goals): the decisions behind it.\n\
                 1. [Contributing](CONTRIBUTING.md): the checks, and how a change lands.\n"
            ),
            Vec::<String>::new(),
            "relative links with a description or an anchor pass"
        );
    }

    #[test]
    fn each_malformed_entry_is_reported() {
        let cases = [
            ("- Install", "doesn't start with a link"),
            ("- [](docs/a.md): a.", "its link has no text"),
            ("- [A](docs/a.md: a.", "has no closing `)`"),
            ("- [A](https://example.com/a): a.", "isn't a relative path"),
            ("- [A](/docs/a.md): a.", "isn't a relative path"),
            ("- [A](#usage): a.", "isn't a relative path"),
            ("- [A](mailto:a@example.com): a.", "isn't a relative path"),
            ("- [A](../a.md): a.", "isn't a relative path"),
            (
                "- [A](docs/a.md \"A\"): a.",
                "has a title or angle brackets",
            ),
            ("- [A](<docs/a.md>): a.", "has a title or angle brackets"),
            ("- [A](docs/a.md)", "isn't followed by `: `"),
            ("- [A](docs/a.md) - a.", "isn't followed by `: `"),
            ("- [A](docs/a.md): ", "isn't followed by `: `"),
            ("- [A](docs/a.md): .", "has no words"),
            ("- [A](docs/a.md): 1.", "has no words"),
            ("- [A](docs/a.md): a", "doesn't end in `.`"),
        ];
        for (entry, problem) in cases {
            let problems = index(&format!("{entry}\n"));
            assert_eq!(problems.len(), 1, "{entry:?} is one problem: {problems:?}");
            assert!(
                problems[0].contains(problem),
                "{entry:?} should be reported as {problem:?}: {problems:?}"
            );
        }
    }

    #[test]
    fn anything_but_one_line_entries_in_the_index_is_reported_once() {
        let problems = index(
            "Some prose.\n\n- [A](docs/a.md): a.\n\n  More about a.\n\n```sh\nrun\n```\n\
             - [B](docs/b.md): b.\n  - [C](docs/c.md): nested.\n",
        );
        assert_eq!(
            problems,
            [format!(
                "The `## Documentation` section holds `Some prose.`, but it holds only its \
                 index, one single-line list item per page or file, written as {ENTRY_FORM}. \
                 Move the text to a page in `docs/` and link the page from an entry."
            )],
            "the first stray line is named, and the entries themselves pass"
        );
        for stray in [
            "### Pages\n",
            "```sh\nrun\n```\n",
            "- [A](a.md): a.\n  - [C](c.md): c.\n",
            "- [A](a.md): the end.\n  More.\n",
            "- [A](a.md): the end.\nMore.\n",
            "| a |\n| - |\n",
            "    - [A](a.md): a.\n",
        ] {
            assert_eq!(index(stray).len(), 1, "{stray:?} isn't a one-line entry");
        }
    }

    #[test]
    fn a_page_with_a_title_and_a_paragraph_passes() {
        for text in [
            "# Install\n\nFor people who run it: how to install it.\n\n## Steps\n",
            "# Install\n\n[Releases](x.md) have the archives.\n",
            "# Install\n\n![Logo](logo.png) For people who run it.\n",
            "# Install\n\nFor people\nwho run it.\n\n---\n",
        ] {
            assert_eq!(page(text), None, "{text:?} opens with a paragraph");
        }
    }

    #[test]
    fn each_page_problem_is_reported() {
        let title = "doesn't start with a `# Title` line";
        let paragraph = "isn't followed by a paragraph";
        let cases = [
            ("", title),
            ("Install\n=======\n\nText.\n", title),
            ("## Install\n\nText.\n", title),
            ("#\n\nText.\n", title),
            ("\n# Install\n\nText.\n", title),
            ("- # Install\n\nText.\n", title),
            ("# Install\n", "Nothing follows its title"),
            ("# Install\n```sh\nx\n```\n", "isn't blank"),
            ("# Install\nText.\n", "isn't blank"),
            ("# Install\n\n", paragraph),
            ("# Install\n\n\nText.\n", paragraph),
            ("# Install\n\n## Steps\n", paragraph),
            ("# Install\n\n- a\n", paragraph),
            ("# Install\n\n| a |\n| - |\n", paragraph),
            ("# Install\n\na | b\n--- | ---\n", paragraph),
            ("# Install\n\n> Note.\n", paragraph),
            ("# Install\n\n```sh\nx\n```\n", paragraph),
            ("# Install\n\n    code\n", paragraph),
            ("# Install\n\n\tcode\n", paragraph),
            ("# Install\n\n<!-- For people. -->\n", paragraph),
            ("# Install\n\n<div>For people.</div>\n", paragraph),
            ("# Install\n\n![The demo](demo.png)\n", paragraph),
            ("# Install\n\n[install]: install.md\n", paragraph),
            ("# Install\n\nSteps\n-----\n", paragraph),
            ("# Install\n\nFor people\nwho run it\n=====\n", paragraph),
            ("# Install\n\n***\n", paragraph),
        ];
        for (text, problem) in cases {
            let found = page(text);
            assert!(
                found.is_some_and(|found| found.contains(problem)),
                "{text:?} should be reported as {problem:?}, not {found:?}"
            );
        }
    }

    #[test]
    fn the_readme_has_one_title_first() {
        for text in [
            "# Demo\n\nText.\n",
            "[![CI](ci.svg)](ci)\n\n# Demo\n",
            "# Demo\n\n[![CI](ci.svg)](ci)\n[![Release](r.svg)](r)\n",
        ] {
            assert!(
                readme(text).is_empty(),
                "{text:?} passes: {:?}",
                readme(text)
            );
        }
        for (text, problem) in [
            ("Demo\n\n## A\n", "has 0 `#` headings"),
            ("# Demo\n\n# Again\n", "has 2 `#` headings"),
            ("Intro.\n\n# Demo\n", "title comes after text"),
            ("## A\n\n# Demo\n", "title comes after text"),
        ] {
            let problems = readme(text);
            assert!(
                problems.iter().any(|found| found.contains(problem)),
                "{text:?} should be reported as {problem:?}: {problems:?}"
            );
        }
    }

    #[test]
    fn hidden_headings_in_the_readme_are_reported() {
        for (text, problem) in [
            (
                "# Demo\n\n> ## A\n",
                "Line 3 puts a heading inside a block quote",
            ),
            (
                "# Demo\n\n- ## A\n",
                "Line 3 puts a heading inside a block quote",
            ),
            (
                "# Demo\n\nText\n----\n",
                "Line 4 underlines the line above it",
            ),
            ("# Demo\n\n<h2>A</h2>\n", "Line 3 has an HTML heading tag"),
            (
                "# Demo\n\nSee <H3 id=a>A</H3>.\n",
                "Line 3 has an HTML heading tag",
            ),
            ("# Demo\n\nText </h4>\n", "Line 3 has an HTML heading tag"),
        ] {
            let problems = readme(text);
            assert!(
                problems.iter().any(|found| found.contains(problem)),
                "{text:?} should be reported as {problem:?}: {problems:?}"
            );
        }
        for text in [
            "# Demo\n\nWrite `<h2>` as `##`.\n",
            "# Demo\n\n```html\n<h2>A</h2>\n```\n",
            "# Demo\n\n<header>, <hr> and <html>.\n",
        ] {
            assert!(
                readme(text).is_empty(),
                "{text:?} passes: {:?}",
                readme(text)
            );
        }
    }

    #[test]
    fn other_markdown_extensions_are_renamed() {
        assert_eq!(
            renamed_page("docs/extra.markdown"),
            Some("docs/extra.md".to_owned()),
            "a `.markdown` page"
        );
        assert_eq!(
            renamed_page("docs/a/b.MKD"),
            Some("docs/a/b.md".to_owned()),
            "in any case and directory"
        );
        for file in ["docs/a.md", "docs/a.png", "docs/markdown"] {
            assert_eq!(renamed_page(file), None, "{file} keeps its name");
        }
    }

    #[test]
    fn relative_targets_are_recognized() {
        for target in ["docs/a.md", "./docs/a.md#b", "CONTRIBUTING.md", "a.md#k:v"] {
            assert!(is_relative(target), "{target} is relative");
        }
        for target in ["", "https://x", "/a.md", "#a", "mailto:a", "../x.md"] {
            assert!(!is_relative(target), "{target:?} isn't relative");
        }
    }
}
