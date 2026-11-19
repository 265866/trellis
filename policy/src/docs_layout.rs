//! The docs keep their layout: the files `docs_files` lists exist, the root `README.md` has the
//! sections `readme_sections` lists and an index for its `Documentation` section, and every page
//! in `docs/` opens with its title and a paragraph.

use std::fs;

use crate::docs_index::{self, INDEX, README};
use crate::markdown::{self, Heading, Line, Markdown};
use crate::policy_error::PolicyError;
use crate::repository_files;
use crate::violation::Violation;
use crate::workspace::Workspace;

/// The manifest that holds the settings, relative to the root.
const MANIFEST: &str = "Cargo.toml";

/// How an entry of the `Documentation` index is written.
const ENTRY_FORM: &str = "`- [Title](path): what the reader does with it.`";

/// Checks the layout of the docs, as `docs_files` and `readme_sections` in
/// `[workspace.metadata.policy]` set it:
///
/// - every file `docs_files` lists exists;
/// - the level-2 ATX headings of the root `README.md`, outside code blocks, are exactly the ones
///   `readme_sections` lists, in order, and it has no setext heading, a line underlined with `=`
///   or `-`, that the comparison would miss;
/// - every list item in its `## Documentation` section is a relative Markdown link followed by
///   `: ` and a description that ends in `.`, and the section holds nothing but those items;
/// - every Markdown file directly under `docs/` starts with a level-1 heading, a blank line and
///   a paragraph, not a heading, list, table, quote or code block.
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
        Some(text) => violations.extend(readme_violations(&Markdown::new(text), sections)),
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
    let pages = files.iter().filter(|file| docs_index::is_page(file));
    for page in pages {
        let path = workspace.root().join(page);
        let text =
            fs::read_to_string(&path).map_err(|source| PolicyError::Read { path, source })?;
        if let Some(problem) = page_problem(&Markdown::new(&text)) {
            violations.push(Violation::new(
                page.clone(),
                format!(
                    "{problem}. A page starts with its title as a `# Title` line, then a blank \
                     line and a paragraph that says who the page is for and what it gets them, \
                     before any other heading, list, table or code block."
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

/// The violations of the root readme `readme`: sections other than `sections`, if that is set,
/// setext headings, and `Documentation` entries that aren't index entries.
fn readme_violations(readme: &Markdown<'_>, sections: Option<&[String]>) -> Vec<Violation> {
    let mut violations = Vec::new();
    let violation = |message: String| Violation::new(README.to_owned(), message);
    if let Some(required) = sections {
        let found: Vec<&str> = readme
            .headings()
            .filter(|heading| heading.level() == 2)
            .map(Heading::text)
            .collect();
        if !found.iter().eq(required.iter()) {
            violations.push(violation(format!(
                "The readme's `##` sections are {}, but `readme_sections` in \
                 `[workspace.metadata.policy]` lists {}. Give the readme exactly those sections, \
                 in that order, and move anything longer than a few commands to a page in \
                 `docs/`.",
                heading_list(&found),
                heading_list(required)
            )));
        }
    }
    for index in readme.setext_underlines() {
        violations.push(violation(format!(
            "Line {} underlines the line above it, which makes that line a heading the section \
             check doesn't read. Write the heading as a `#` line, such as `## Install`, or \
             leave a blank line above the underline to keep it a thematic break.",
            index + 1
        )));
    }
    if let Some(section) = readme.section(INDEX) {
        violations.extend(index_problems(&section).into_iter().map(violation));
    }
    violations
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
/// that isn't an index entry, and one for the first thing in it that isn't a list item.
fn index_problems(section: &Markdown<'_>) -> Vec<String> {
    let mut entries: Vec<String> = Vec::new();
    let mut stray: Option<String> = None;
    let mut in_entry = false;
    for line in section.lines() {
        let Line::Prose(text) = *line else {
            stray.get_or_insert_with(|| "a code block".to_owned());
            in_entry = false;
            continue;
        };
        let item = markdown::list_item_text(text).filter(|_| !text.starts_with([' ', '\t']));
        if text.trim().is_empty() {
            in_entry = false;
        } else if let Some(item) = item {
            entries.push(item.to_owned());
            in_entry = true;
        } else if let Some(entry) = entries
            .last_mut()
            .filter(|_| in_entry && Heading::parse(text).is_none() && !markdown::is_list_item(text))
        {
            entry.push(' ');
            entry.push_str(text.trim());
        } else {
            stray.get_or_insert_with(|| format!("`{}`", text.trim()));
            in_entry = false;
        }
    }
    let mut problems: Vec<String> = entries
        .iter()
        .filter_map(|entry| {
            entry_problem(entry).map(|problem| {
                format!(
                    "The `## {INDEX}` entry `{entry}` isn't an index entry: {problem}. Write \
                     each entry as {ENTRY_FORM}, with a relative path."
                )
            })
        })
        .collect();
    if let Some(stray) = stray {
        problems.push(format!(
            "The `## {INDEX}` section holds {stray}, but it holds only its index, one list item \
             per page or file, written as {ENTRY_FORM}. Move the text to a page in `docs/` and \
             link the page from an entry."
        ));
    }
    problems
}

/// What keeps `entry`, the text of a list item, from being an index entry: a relative Markdown
/// link, `: ` and a description that ends in `.`. `None` if it is one.
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
    if !is_relative(target) {
        return Some("its link isn't a relative path to a file in the repository");
    }
    let Some(description) = rest.strip_prefix(": ") else {
        return Some("the link isn't followed by `: ` and what the reader does with the page");
    };
    if !description.ends_with('.') {
        return Some("its description doesn't end in `.`");
    }
    None
}

/// Whether the link target `target` is a relative path: no scheme such as `https:`, no leading
/// `/` or `#`, no `..` part that leaves the repository, and no spaces or angle brackets.
fn is_relative(target: &str) -> bool {
    let before_path = target.split(['/', '#']).next().unwrap_or(target);
    !target.is_empty()
        && !target.split('/').any(|part| part == "..")
        && !target.starts_with(['/', '#'])
        && !before_path.contains(':')
        && !target.contains(|c: char| c.is_whitespace() || c == '<' || c == '>')
}

/// What keeps `page` from opening with a `# Title` line, a blank line and a paragraph, or `None`
/// if it does.
fn page_problem(page: &Markdown<'_>) -> Option<&'static str> {
    let lines = page.lines();
    let title = match lines.first() {
        Some(Line::Prose(text)) => Heading::parse(text),
        Some(Line::Code) | None => None,
    };
    if !title.is_some_and(|title| title.level() == 1 && !title.text().is_empty()) {
        return Some("It doesn't start with a `# Title` line");
    }
    let Some(Line::Prose(blank)) = lines.get(1) else {
        return Some("Nothing but a code block or the end of the file follows its title");
    };
    if !blank.trim().is_empty() {
        return Some("The line after its title isn't blank");
    }
    let paragraph = match lines.get(2) {
        Some(Line::Prose(text)) => markdown::starts_paragraph(text),
        Some(Line::Code) | None => false,
    };
    let underlined = page.setext_underlines().contains(&3);
    let table =
        matches!(lines.get(3), Some(Line::Prose(text)) if markdown::is_table_delimiter(text));
    if !paragraph || underlined || table {
        return Some(
            "Its title isn't followed by a paragraph: the blank line after it is followed by \
             nothing, another blank line, or a heading, list, table, quote or code block",
        );
    }
    None
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

    #[test]
    fn index_entries_pass() {
        assert_eq!(
            index(
                "- [Install](docs/install.md): every way to install it.\n\
                 * [Design](./docs/design.md#goals): the decisions behind it.\n\
                 1. [Contributing](CONTRIBUTING.md): the checks, and how a change lands\n   \
                 on `main`.\n"
            ),
            Vec::<String>::new(),
            "relative links with a description, an anchor or a continuation line pass"
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
            ("- [A](docs/a.md \"A\"): a.", "isn't a relative path"),
            ("- [A](docs/a.md)", "isn't followed by `: `"),
            ("- [A](docs/a.md) - a.", "isn't followed by `: `"),
            ("- [A](docs/a.md): ", "isn't followed by `: `"),
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
    fn anything_but_entries_in_the_index_is_reported_once() {
        let problems = index(
            "Some prose.\n\n- [A](docs/a.md): a.\n\n  More about a.\n\n```sh\nrun\n```\n\
             - [B](docs/b.md): b.\n  - [C](docs/c.md): nested.\n",
        );
        assert_eq!(
            problems,
            [format!(
                "The `## Documentation` section holds `Some prose.`, but it holds only its \
                 index, one list item per page or file, written as {ENTRY_FORM}. Move the text \
                 to a page in `docs/` and link the page from an entry."
            )],
            "the first stray line is named, and the entries themselves pass"
        );
        for stray in [
            "### Pages\n",
            "```sh\nrun\n```\n",
            "- [A](a.md): a.\n  - [C](c.md): c.\n",
        ] {
            assert_eq!(index(stray).len(), 1, "{stray:?} isn't an entry");
        }
    }

    #[test]
    fn a_page_with_a_title_and_a_paragraph_passes() {
        assert_eq!(
            page("# Install\n\nFor people who run it: how to install it.\n\n## Steps\n"),
            None,
            "a title, a blank line and a paragraph"
        );
        assert_eq!(
            page("# Install\n\n[Releases](x.md) have the archives.\n"),
            None,
            "a paragraph may start with a link"
        );
    }

    #[test]
    fn each_page_problem_is_reported() {
        let cases = [
            ("", "doesn't start with a `# Title` line"),
            (
                "Install\n=======\n\nText.\n",
                "doesn't start with a `# Title` line",
            ),
            (
                "## Install\n\nText.\n",
                "doesn't start with a `# Title` line",
            ),
            ("#\n\nText.\n", "doesn't start with a `# Title` line"),
            (
                "\n# Install\n\nText.\n",
                "doesn't start with a `# Title` line",
            ),
            ("# Install\n", "Nothing but a code block"),
            ("# Install\n```sh\nx\n```\n", "Nothing but a code block"),
            ("# Install\nText.\n", "isn't blank"),
            ("# Install\n\n", "isn't followed by a paragraph"),
            ("# Install\n\n\nText.\n", "isn't followed by a paragraph"),
            ("# Install\n\n## Steps\n", "isn't followed by a paragraph"),
            ("# Install\n\n- a\n", "isn't followed by a paragraph"),
            (
                "# Install\n\n| a |\n| - |\n",
                "isn't followed by a paragraph",
            ),
            (
                "# Install\n\na | b\n--- | ---\n",
                "isn't followed by a paragraph",
            ),
            ("# Install\n\n> Note.\n", "isn't followed by a paragraph"),
            (
                "# Install\n\n```sh\nx\n```\n",
                "isn't followed by a paragraph",
            ),
            ("# Install\n\n    code\n", "isn't followed by a paragraph"),
            (
                "# Install\n\nSteps\n-----\n",
                "isn't followed by a paragraph",
            ),
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
    fn relative_targets_are_recognized() {
        for target in ["docs/a.md", "./docs/a.md#b", "CONTRIBUTING.md", "a.md#k:v"] {
            assert!(is_relative(target), "{target} is relative");
        }
        for target in [
            "",
            "https://x",
            "/a.md",
            "#a",
            "mailto:a",
            "<a.md>",
            "a b.md",
            "../x.md",
        ] {
            assert!(!is_relative(target), "{target:?} isn't relative");
        }
    }
}
