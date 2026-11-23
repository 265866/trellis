//! The docs keep their layout: the files `docs_files` lists exist, the root `README.md` has one
//! title, the sections `readme_sections` lists and an index for its `Documentation` section, a
//! `Documentation` section in `CONTRIBUTING.md` is an index too, and every page in `docs/` opens
//! with its title and a paragraph.

use std::path::Path;

use comrak::Arena;
use comrak::nodes::{Node, NodeValue};

use crate::docs_index::{self, BYTE_ORDER_MARK, CONTRIBUTING, INDEX, README};
use crate::markdown::{self, Heading, Markdown, Unbalanced};
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

/// Where the readme's title goes, which the title messages end with.
const TITLE_PLACE: &str = "on its first line or after only badges, comments and blank lines";

/// What a page that doesn't open with a paragraph is told.
const NO_PARAGRAPH: &str = "Its title isn't followed by a paragraph: the blank line after it is \
                            followed by another blank line, or by a block other than a paragraph \
                            with words in it, such as a heading, list, table, quote, code block, \
                            HTML, image or link reference definition";

/// What a file with HTML that doesn't close is told, after the line it opens on.
const UNCLOSED_HTML: &str = "opens HTML that doesn't close: a comment, a tag or a quoted \
                             attribute value. It can hide what follows from readers; close it \
                             in the same HTML block, or write a literal `<` as `&lt;`.";

/// Checks the layout of the docs, as `docs_files` and `readme_sections` in
/// `[workspace.metadata.policy]` set it. It reads Markdown with comrak, set up to parse as
/// GitHub renders it:
///
/// - every file `docs_files` lists exists;
/// - the root `README.md` has exactly one level-1 heading, its title, written as a `#` line at
///   the top level with text in it, with only badges, comments and blank lines before it;
/// - its top-level `##` headings are exactly the ones `readme_sections` lists, in order, each
///   written as plain text; it has no `##` heading inside a list item, quote or footnote, no
///   setext heading, a line underlined with `=` or `-`, and no HTML heading tag such as `<h2>`;
/// - its `## Documentation` section holds only list items of one line each, and comments, and
///   each item is a link written exactly `[Title](path)` with a relative path, without entities
///   or escapes, then `: ` and a description whose rendered text has a letter in it, on a line
///   that ends in `.`;
/// - a `## Documentation` section in `CONTRIBUTING.md`, which may index the pages only
///   contributors need, holds an index written the same way, and a `CONTRIBUTING.md` with one is
///   held to the HTML and byte order mark checks below, like the readme;
/// - every page, a `.md` file at any depth under `docs/`, starts with a `#` line with text in
///   it, after any comments, then a blank line and a paragraph with words in it outside images;
/// - neither the readme, a page nor a `CONTRIBUTING.md` with an index has an HTML block with a comment, a tag or a quoted
///   attribute value that doesn't close in it, or an HTML element whose tags don't balance
///   between one top-level heading of level 1 or 2 and the next, which can hide what follows or
///   render it inside the element;
/// - no file under `docs/` has another Markdown extension, such as `.markdown`, which the checks
///   would skip;
/// - neither the readme, a page nor a `CONTRIBUTING.md` with an index starts with a byte order
///   mark. The other checks read past
///   it, so a file gets one message for it.
///
/// A missing setting is a violation rather than a default, since no default could say which
/// files and sections a project keeps.
///
/// It reads the files git tracks and the untracked ones `.gitignore` doesn't exclude.
///
/// # Errors
///
/// Fails if the repository's files can't be listed, or the readme, `CONTRIBUTING.md` or a page
/// can't be read.
pub fn check(workspace: &Workspace) -> Result<Vec<Violation>, PolicyError> {
    let config = workspace.config();
    let docs_files = config.docs_files();
    let sections = config.readme_sections();
    let files = repository_files::list(workspace.root())?;
    let mut violations = Vec::new();
    match docs_files {
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
    if sections.is_none() {
        violations.push(unset(
            "readme_sections",
            "which sections the root `README.md` has. Set it to the readme's `##` headings in \
             order, such as `readme_sections = [\"Install\", \"Documentation\", \"License\"]`.",
        ));
    }
    match docs_index::read_readme(workspace.root())? {
        Some(text) => {
            violations.extend(byte_order_mark(README, &text));
            let arena = Arena::new();
            let readme = Markdown::parse(&arena, text.trim_start_matches(BYTE_ORDER_MARK));
            for problem in readme_problems(&readme, sections) {
                violations.push(Violation::new(README.to_owned(), problem));
            }
        }
        None if !docs_files.is_some_and(|required| required.iter().any(|file| file == README)) => {
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
    if files.iter().any(|file| file == CONTRIBUTING)
        && let Some(text) = docs_index::read_root_file(workspace.root(), CONTRIBUTING)?
    {
        let arena = Arena::new();
        let contributing = Markdown::parse(&arena, text.trim_start_matches(BYTE_ORDER_MARK));
        if let Some(section) = contributing.section(INDEX) {
            violations.extend(byte_order_mark(CONTRIBUTING, &text));
            for problem in html_problems(&contributing)
                .into_iter()
                .chain(index_problems(&contributing, &section))
            {
                violations.push(Violation::new(CONTRIBUTING.to_owned(), problem));
            }
        }
    }
    let docs = files.iter().filter(|file| docs_index::is_under_docs(file));
    for file in docs {
        if docs_index::is_page(file) {
            let text = docs_index::read_page(workspace.root(), file)?;
            violations.extend(byte_order_mark(file, &text));
            let arena = Arena::new();
            let page = Markdown::parse(&arena, text.trim_start_matches(BYTE_ORDER_MARK));
            for problem in html_problems(&page) {
                violations.push(Violation::new(file.clone(), problem));
            }
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

/// The violation of `file` if its text, `text`, starts with a byte order mark.
fn byte_order_mark(file: &str, text: &str) -> Option<Violation> {
    text.starts_with(BYTE_ORDER_MARK).then(|| {
        Violation::new(
            file.to_owned(),
            "It starts with a byte order mark. Save it as UTF-8 without one; the other checks \
             read it as if the mark weren't there."
                .to_owned(),
        )
    })
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
/// that is set, headings the section check can't read as `#` lines, and an index that isn't
/// one.
fn readme_problems(readme: &Markdown<'_>, sections: Option<&[String]>) -> Vec<String> {
    let headings = readme.headings();
    let mut problems = Vec::new();
    problems.extend(title_problem(readme, &headings));
    if let Some(required) = sections {
        let found: Vec<&str> = headings
            .iter()
            .filter(|heading| heading.level == 2 && heading.top_level)
            .map(|heading| heading.text.as_str())
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
    for heading in &headings {
        if heading.level == 2 && !heading.top_level {
            problems.push(format!(
                "Line {} has a `##` heading inside a list item, quote or footnote. A section \
                 heading must be a top-level `##` line.",
                heading.last_line
            ));
        } else if heading.level == 2
            && !heading.setext
            && heading_source(readme, heading).is_empty()
        {
            problems.push(format!(
                "Line {} has an empty `##` heading. Give the section its name, such as \
                 `## Install`.",
                heading.last_line
            ));
        } else if heading.level == 2 && !heading.setext && !is_plain(readme, heading) {
            problems.push(format!(
                "Line {} has a `##` heading with markup, `{}`. Write the heading as plain text, \
                 such as `## Install`.",
                heading.last_line,
                readme.line(heading.last_line).trim()
            ));
        }
        if heading.setext {
            problems.push(format!(
                "Line {} underlines the line above it, which makes that line a heading. Write \
                 every heading as a `#` line, such as `## Install`, or leave a blank line above \
                 the underline to keep it a thematic break.",
                heading.last_line
            ));
        }
    }
    for line in readme.html_heading_lines() {
        problems.push(format!(
            "Line {line} has an HTML heading tag such as `<h2>`, which the section check doesn't \
             read. Write the heading as a `#` line."
        ));
    }
    problems.extend(html_problems(readme));
    if let Some(section) = readme.section(INDEX) {
        problems.extend(index_problems(readme, &section));
    }
    problems
}

/// What is wrong with the HTML in `markdown`: HTML that doesn't close, and elements whose tags
/// don't balance within a section.
fn html_problems(markdown: &Markdown<'_>) -> Vec<String> {
    let mut problems = Vec::new();
    for line in markdown.unclosed_html_lines() {
        problems.push(format!("Line {line} {UNCLOSED_HTML}"));
    }
    for element in markdown.unbalanced_elements() {
        let Unbalanced {
            line,
            name,
            closing,
            optional_end,
        } = element;
        problems.push(if closing {
            format!(
                "Line {line} closes `</{name}>`, which no `<{name}>` open in its section \
                 matches. Remove it, or open the element in the same section."
            )
        } else if optional_end {
            format!(
                "Line {line} opens `<{name}>`, which doesn't close before the next section. \
                 Close it; trellis requires every element closed in its section. Or write a \
                 literal `<` as `&lt;`."
            )
        } else {
            format!(
                "Line {line} opens `<{name}>`, which doesn't close before the next section, so \
                 GitHub renders what follows inside it. Close it in the same section, or write \
                 a literal `<` as `&lt;`."
            )
        });
    }
    problems
}

/// Whether `heading`, an ATX heading of `readme`, is written as plain text: its line is its
/// text, with only the `#` characters and spaces around it.
fn is_plain(readme: &Markdown<'_>, heading: &Heading) -> bool {
    let source = heading_source(readme, heading);
    !source.is_empty() && source == heading.text
}

/// The source of `heading`, an ATX heading of `readme`: its line without the `#` characters
/// and the spaces around it.
fn heading_source<'text>(readme: &Markdown<'text>, heading: &Heading) -> &'text str {
    let source = readme
        .line(heading.last_line)
        .trim()
        .trim_start_matches('#')
        .trim();
    let open = source.trim_end_matches('#');
    if open.is_empty() {
        open
    } else if open.ends_with([' ', '\t']) {
        open.trim_end()
    } else {
        source
    }
}

/// What is wrong with the title of `readme`, whose headings are `headings`, or `None` if it has
/// exactly one level-1 heading, at the top level, with only badges, comments and blank lines
/// before it.
fn title_problem(readme: &Markdown<'_>, headings: &[Heading]) -> Option<String> {
    let titles: Vec<&Heading> = headings
        .iter()
        .filter(|heading| heading.level == 1)
        .collect();
    match titles[..] {
        [] => Some(format!(
            "The readme has no `#` heading. It needs exactly one, its title, {TITLE_PLACE}."
        )),
        [title] if !title.top_level => Some(format!(
            "Its `#` title is inside a list, quote or footnote. Make it a top-level `#` line, \
             {TITLE_PLACE}."
        )),
        [title] if title.text.trim().is_empty() => Some(
            "Its `#` title has no text. Write the project's name after the `#`, as plain text."
                .to_owned(),
        ),
        [_] => {
            let blocks = readme.blocks();
            let only_badges = blocks
                .iter()
                .take_while(|block| {
                    markdown::heading(block).is_none_or(|heading| heading.level != 1)
                })
                .all(|block| {
                    markdown::is_comment(block)
                        || (matches!(block.data.borrow().value, NodeValue::Paragraph)
                            && markdown::is_badges(block))
                });
            (!only_badges).then(|| {
                "Its `#` title comes after text other than badges, comments and blank lines. Put \
                 the title first, with only those before it."
                    .to_owned()
            })
        }
        _ => Some(format!(
            "The readme has {} `#` headings. It needs exactly one, its title, {TITLE_PLACE}. \
             Make the others `##` sections or move them to a page in `docs/`.",
            titles.len()
        )),
    }
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

/// What is wrong with `section`, the blocks of the `Documentation` section of `readme`: one
/// message for each list item that isn't an index entry, and one for the first thing in it that
/// isn't a one-line list item.
fn index_problems(readme: &Markdown<'_>, section: &[Node<'_>]) -> Vec<String> {
    let mut problems = Vec::new();
    let mut stray: Option<String> = None;
    for block in section {
        if markdown::is_comment(block) {
            continue;
        }
        let is_list = matches!(block.data.borrow().value, NodeValue::List(_));
        if !is_list {
            stray.get_or_insert_with(|| stray_name(readme, block));
            continue;
        }
        for item in block.children() {
            if let Some(name) = stray_in_item(readme, item) {
                stray.get_or_insert(name);
            } else if let Some(paragraph) = item.first_child() {
                let line = readme.line(markdown::first_line(paragraph));
                let problem = if markdown::is_task_item(item) {
                    Some("it is a task list item; leave out the `[ ]`")
                } else {
                    link_problem(line, paragraph)
                };
                if let Some(problem) = problem {
                    let start = markdown::columns(paragraph).start().saturating_sub(1);
                    let entry = line.get(start..).unwrap_or(line).trim_end();
                    problems.push(format!(
                        "The `## {INDEX}` entry `{entry}` isn't an index entry: {problem}. \
                         Write each entry as {ENTRY_FORM}, with a relative path."
                    ));
                }
            }
        }
    }
    if let Some(stray) = stray {
        problems.push(format!(
            "The `## {INDEX}` section holds {stray}, but it may hold only its index, one \
             single-line list item per page or file, written as {ENTRY_FORM}. Move the text to \
             a page in `docs/` and link the page from an entry."
        ));
    }
    problems
}

/// How a message names `block`, a block that isn't an index entry: its first line.
fn stray_name(readme: &Markdown<'_>, block: Node<'_>) -> String {
    if matches!(block.data.borrow().value, NodeValue::CodeBlock(_)) {
        return "a code block".to_owned();
    }
    format!("`{}`", readme.line(markdown::first_line(block)).trim())
}

/// How a message names what `item`, a list item of the index, holds besides one line with one
/// paragraph, or `None` if it holds only that.
fn stray_in_item(readme: &Markdown<'_>, item: Node<'_>) -> Option<String> {
    let children: Vec<Node<'_>> = item.children().collect();
    match children[..] {
        [paragraph]
            if matches!(paragraph.data.borrow().value, NodeValue::Paragraph)
                && markdown::first_line(item) == markdown::last_line(item) =>
        {
            None
        }
        [_, second, ..] => Some(stray_name(readme, second)),
        [_] if markdown::last_line(item) > markdown::first_line(item) => Some(format!(
            "`{}`",
            readme.line(markdown::first_line(item) + 1).trim()
        )),
        _ => Some(stray_name(readme, item)),
    }
}

/// What keeps `paragraph`, a one-line list item's paragraph on `source_line`, from being an index
/// entry: a link written exactly `[Title](path)` with a relative path, then `: ` and a
/// description, whose rendered text has a letter in it and ends in `.`. `None` if it is one.
fn link_problem(source_line: &str, paragraph: Node<'_>) -> Option<&'static str> {
    let link = paragraph.first_child().and_then(|child| {
        if let NodeValue::Link(link) = &child.data.borrow().value {
            Some((child, link.url.clone(), link.title.is_empty()))
        } else {
            None
        }
    });
    let Some((link, url, untitled)) = link else {
        return Some("it doesn't start with a link such as `[Install](docs/install.md)`");
    };
    if markdown::text(link).trim().is_empty() {
        return Some("its link has no text");
    }
    let columns = markdown::columns(link);
    let source = source_line
        .get(columns.start().saturating_sub(1)..*columns.end())
        .unwrap_or("");
    let target = source
        .rfind("](")
        .filter(|_| source.starts_with('['))
        .and_then(|start| source[start + 2..].strip_suffix(')'));
    let Some(target) = target else {
        return Some("its link isn't written inline as `[Title](path)`");
    };
    if !untitled || target.starts_with('<') || target.contains(char::is_whitespace) {
        return Some(
            "its link has a title, angle brackets or spaces, which the index doesn't allow; \
             write it as `[Title](path)`",
        );
    }
    if target.contains(['&', '\\']) || !is_relative(target) || !is_relative(&url) {
        return Some(
            "its link isn't a relative path to a file in the repository, written without \
             entities or escapes",
        );
    }
    let rendered = markdown::text_after(link);
    let Some(description) = rendered.strip_prefix(": ") else {
        return Some("the link isn't followed by `: ` and what the reader does with the page");
    };
    if !description.chars().any(char::is_alphabetic) {
        return Some("its description has no words");
    }
    if !description.trim_end().ends_with('.') || !source_line.trim_end().ends_with('.') {
        return Some("its description doesn't end in `.`, with nothing after it");
    }
    None
}

/// Whether the link target `target` is a relative path to a file, as [`docs_index::resolve`]
/// reads one from the root, with no `..` part at all in its path. The `#` fragment isn't part of
/// the path.
fn is_relative(target: &str) -> bool {
    let path = target.split_once('#').map_or(target, |(path, _)| path);
    !path.split('/').any(|part| part == "..") && docs_index::resolve("", target).is_some()
}

/// What keeps `page` from opening with a `# Title` line, a blank line and a paragraph, or `None`
/// if it does. Comments may come before the title.
fn page_problem(page: &Markdown<'_>) -> Option<&'static str> {
    let mut blocks = page.blocks();
    let comments = blocks
        .iter()
        .take_while(|block| markdown::is_comment(block))
        .count();
    blocks.drain(..comments);
    let title = blocks.first().filter(|title| {
        (comments > 0 || markdown::first_line(title) == 1)
            && markdown::heading(title).is_some_and(|heading| heading.level == 1 && !heading.setext)
            && !markdown::text(title).trim().is_empty()
    });
    let Some(title) = title else {
        return Some("It doesn't start with a `# Title` line");
    };
    let title_end = markdown::last_line(title);
    let Some(next) = blocks.get(1) else {
        return Some(if page.has_text_after(title_end) {
            NO_PARAGRAPH
        } else {
            "Nothing follows its title"
        });
    };
    if markdown::first_line(next) == title_end + 1 {
        return Some("The line after its title isn't blank");
    }
    let opens = markdown::first_line(next) == title_end + 2
        && matches!(next.data.borrow().value, NodeValue::Paragraph)
        && markdown::has_words(next);
    (!opens).then_some(NO_PARAGRAPH)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index(text: &str) -> Vec<String> {
        let readme = format!("# Demo\n\n## Documentation\n\n{text}\n## License\n");
        let arena = Arena::new();
        let markdown = Markdown::parse(&arena, &readme);
        index_problems(&markdown, &markdown.section(INDEX).unwrap())
    }

    fn page(text: &str) -> Option<&'static str> {
        let arena = Arena::new();
        page_problem(&Markdown::parse(&arena, text))
    }

    fn readme(text: &str) -> Vec<String> {
        let arena = Arena::new();
        readme_problems(&Markdown::parse(&arena, text), None)
    }

    #[test]
    fn index_entries_pass() {
        assert_eq!(
            index(
                "- [Install](docs/install.md): every way to install it.\n\
                 * [Design](./docs/design.md#goals): the decisions behind it.\n\n\
                 1. [Contributing](CONTRIBUTING.md): the checks, and how a change lands.\n\
                 2. [Security](SECURITY.md): how to report a `vulnerability`.\n"
            ),
            Vec::<String>::new(),
            "bullet and ordered entries with a description or an anchor pass"
        );
    }

    #[test]
    fn each_malformed_entry_is_reported() {
        let cases = [
            ("- Install", "doesn't start with a link"),
            ("- [Install\\](docs/a.md): a.", "doesn't start with a link"),
            ("- [A](docs/a.md: a.", "doesn't start with a link"),
            ("- [](docs/a.md): a.", "its link has no text"),
            ("- [A](https://example.com/a): a.", "isn't a relative path"),
            ("- [A](/docs/a.md): a.", "isn't a relative path"),
            ("- [A](#usage): a.", "isn't a relative path"),
            ("- [A](mailto:a@example.com): a.", "isn't a relative path"),
            ("- [A](../a.md): a.", "isn't a relative path"),
            ("- <https://example.com>: a.", "isn't written inline"),
            ("- [A][a]: a.\n\n[a]: docs/a.md", "isn't written inline"),
            (
                "- [A](docs/a.md \"A\"): a.",
                "has a title, angle brackets or spaces",
            ),
            (
                "- [A](<docs/a.md>): a.",
                "has a title, angle brackets or spaces",
            ),
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
            "Some prose.\n\n- [A](docs/a.md): a.\n\n  More about a.\n\n```sh\nrun\n```\n\n\
             - [B](docs/b.md): b.\n  - [C](docs/c.md): nested.\n",
        );
        assert_eq!(
            problems,
            [format!(
                "The `## Documentation` section holds `Some prose.`, but it may hold only its \
                 index, one single-line list item per page or file, written as {ENTRY_FORM}. \
                 Move the text to a page in `docs/` and link the page from an entry."
            )],
            "the first stray block is named, and the entries themselves pass"
        );
        for stray in [
            "### Pages\n",
            "```sh\nrun\n```\n",
            "- [A](a.md): a.\n  - [C](c.md): c.\n",
            "- [A](a.md): the end.\n  More.\n",
            "- [A](a.md): the end.\nMore.\n",
            "| a |\n| - |\n",
            "    - [A](a.md): a.\n",
            "- - [A](a.md): a.\n",
            "-\n",
            "- <!-- [A](docs/a.md) --> a.\n",
        ] {
            let problems = index(stray);
            assert!(
                problems
                    .iter()
                    .any(|problem| problem.contains("may hold only its index")),
                "{stray:?} isn't a one-line entry: {problems:?}"
            );
        }
    }

    #[test]
    fn a_page_with_a_title_and_a_paragraph_passes() {
        for text in [
            "# Install\n\nFor people who run it: how to install it.\n\n## Steps\n",
            "# Install\n\n[Releases](x.md) have the archives.\n",
            "# Install\n\n![Logo](logo.png) For people who run it.\n",
            "# Install\n\n<https://example.com> has the releases.\n",
            "# Install\n\nFor people\nwho run it.\n\n---\n",
            "# Install\r\n\r\nFor people who run it.\r\n",
        ] {
            assert_eq!(page(text), None, "{text:?} opens with a paragraph");
        }
    }

    #[test]
    fn each_page_problem_is_reported() {
        let title = "doesn't start with a `# Title` line";
        let cases = [
            ("", title),
            ("Install\n=======\n\nText.\n", title),
            ("## Install\n\nText.\n", title),
            ("#\n\nText.\n", title),
            ("\n# Install\n\nText.\n", title),
            ("- # Install\n\nText.\n", title),
            ("# Install\n", "Nothing follows its title"),
            ("# Install\n\n\n", "Nothing follows its title"),
            ("# Install\n```sh\nx\n```\n", "isn't blank"),
            ("# Install\nText.\n", "isn't blank"),
            ("# Install\n\n\nText.\n", NO_PARAGRAPH),
            ("# Install\n\n## Steps\n", NO_PARAGRAPH),
            ("# Install\n\n- a\n", NO_PARAGRAPH),
            ("# Install\n\n| a |\n| - |\n", NO_PARAGRAPH),
            ("# Install\n\n> Note.\n", NO_PARAGRAPH),
            ("# Install\n\n```sh\nx\n```\n", NO_PARAGRAPH),
            ("# Install\n\n    code\n", NO_PARAGRAPH),
            ("# Install\n\n\tcode\n", NO_PARAGRAPH),
            ("# Install\n\n<!-- For people. -->\n", NO_PARAGRAPH),
            ("# Install\n\n<div>For people.</div>\n", NO_PARAGRAPH),
            ("# Install\n\n![The demo](demo.png)\n", NO_PARAGRAPH),
            ("# Install\n\n[install]: install.md\n", NO_PARAGRAPH),
            (
                "# Install\n\n[install]: install.md\nFor people.\n",
                NO_PARAGRAPH,
            ),
            ("# Install\n\nSteps\n-----\n", NO_PARAGRAPH),
            ("# Install\n\nFor people\nwho run it\n=====\n", NO_PARAGRAPH),
            ("# Install\n\n***\n", NO_PARAGRAPH),
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
            "[![CI][ci-badge]][ci]\n\n# Demo\n\n[ci]: ci\n[ci-badge]: ci.svg\n",
            "# Demo\n\n[![CI](ci.svg)](ci)\n[![Release](r.svg)](r)\n",
        ] {
            assert!(
                readme(text).is_empty(),
                "{text:?} passes: {:?}",
                readme(text)
            );
        }
        for (text, problem) in [
            (
                "Demo\n\n## A\n",
                "The readme has no `#` heading. It needs exactly one",
            ),
            (
                "# Demo\n\n# Again\n",
                "has 2 `#` headings. It needs exactly one",
            ),
            ("Intro.\n\n# Demo\n", "title comes after text"),
            ("## A\n\n# Demo\n", "title comes after text"),
            ("[CI](ci)\n\n# Demo\n", "title comes after text"),
            ("- # Demo\n", "title is inside a list, quote or footnote"),
            ("Demo\n====\n", "Line 2 underlines the line above it"),
        ] {
            let problems = readme(text);
            assert!(
                problems.iter().any(|found| found.contains(problem)),
                "{text:?} should be reported as {problem:?}: {problems:?}"
            );
        }
        assert!(
            !readme("Demo\n\n## A\n")
                .iter()
                .any(|found| found.contains("Make the others")),
            "with no title, there are no others"
        );
    }

    #[test]
    fn html_headings_in_the_readme_are_reported() {
        for (text, problem) in [
            ("# Demo\n\n<h2>A</h2>\n", "Line 3 has an HTML heading tag"),
            (
                "# Demo\n\nSee <H3 id=a>A</H3>.\n",
                "Line 3 has an HTML heading tag",
            ),
            ("# Demo\n\nText </h4>\n", "Line 3 has an HTML heading tag"),
            (
                "# Demo\n\n\\`<h2>A</h2>\\`\n",
                "Line 3 has an HTML heading tag",
            ),
            (
                "# Demo\n\n<div>`<h2>A</h2>`</div>\n",
                "Line 3 has an HTML heading tag",
            ),
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
            "# Demo\n\n<header></header>, <hr> and <i>a</i>.\n",
            "# Demo\n\n<!--\n\n<h2>Old</h2>\n\n-->\n",
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
        for target in [
            "docs/a.md",
            "./docs/a.md#b",
            "CONTRIBUTING.md",
            "a.md#k:v",
            "a.md#b/../c",
        ] {
            assert!(is_relative(target), "{target} is relative");
        }
        for target in [
            "",
            "https://x",
            "/a.md",
            "#a",
            "mailto:a",
            "../x.md",
            "a/..#b",
            "docs/install.md/x/..#a",
        ] {
            assert!(!is_relative(target), "{target:?} isn't relative");
        }
    }
}
