//! The root `README.md` links every page in `docs/`.

use std::collections::BTreeSet;
use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use crate::policy_error::PolicyError;
use crate::repository_files;
use crate::violation::Violation;
use crate::workspace::Workspace;

/// The file that links the docs, relative to the root.
const README: &str = "README.md";

/// The directory whose pages the readme links, relative to the root, with a trailing `/`.
const DOCS: &str = "docs/";

/// Checks that the root `README.md` links each Markdown file directly under `docs/` with a
/// relative Markdown link, `[text](docs/<page>.md)` or a reference definition
/// `[label]: docs/<page>.md`, optionally with a `#anchor`. A page no index links is a page
/// readers don't find. Links in code blocks and code spans don't count, since they don't render
/// as links. Pages in subdirectories of `docs/` may be linked from the pages above them instead.
///
/// It reads the files git tracks and the untracked ones `.gitignore` doesn't exclude.
///
/// # Errors
///
/// Fails if the repository's files can't be listed, or `README.md` exists but can't be read.
pub fn check(workspace: &Workspace) -> Result<Vec<Violation>, PolicyError> {
    let files = repository_files::list(workspace.root())?;
    let pages: Vec<&str> = files
        .iter()
        .map(String::as_str)
        .filter(|file| is_page(file))
        .collect();
    if pages.is_empty() {
        return Ok(Vec::new());
    }
    let path = workspace.root().join(README);
    let readme = match fs::read_to_string(&path) {
        Ok(text) => Some(text),
        Err(error) if error.kind() == ErrorKind::NotFound => None,
        Err(source) => return Err(PolicyError::Read { path, source }),
    };
    let linked = readme.as_deref().map_or_else(BTreeSet::new, linked_targets);
    let mut violations = Vec::new();
    for page in pages {
        if linked.contains(page) {
            continue;
        }
        let problem = if readme.is_some() {
            format!("`{README}` doesn't link the page `{page}`")
        } else {
            format!("there is no root `{README}` to link the page `{page}`")
        };
        violations.push(Violation::new(
            README.to_owned(),
            format!(
                "{problem}, so readers don't find it. Add a relative link such as \
                 `[{title}]({page})` to `{README}`, or delete the page if nothing needs it.",
                title = page_title(page)
            ),
        ));
    }
    Ok(violations)
}

/// Whether `relative`, a file relative to the root, is a Markdown file directly under `docs/`.
fn is_page(relative: &str) -> bool {
    relative.strip_prefix(DOCS).is_some_and(|name| {
        !name.contains('/')
            && Path::new(name)
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
    })
}

/// A title for a link to `page`: its file stem.
fn page_title(page: &str) -> &str {
    let name = page.strip_prefix(DOCS).unwrap_or(page);
    name.rsplit_once('.').map_or(name, |(stem, _)| stem)
}

/// The targets of the Markdown links in `text`, outside code, without a leading `./` or a
/// trailing `#anchor`.
fn linked_targets(text: &str) -> BTreeSet<String> {
    let mut targets = BTreeSet::new();
    let mut fence: Option<&str> = None;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if let Some(open) = fence {
            if trimmed.starts_with(open) {
                fence = None;
            }
            continue;
        }
        if let Some(open) = ["```", "~~~"]
            .into_iter()
            .find(|open| trimmed.starts_with(open))
        {
            fence = Some(open);
            continue;
        }
        let prose = without_code_spans(line);
        if let Some(target) = reference_target(&prose) {
            targets.insert(normalize(target));
        }
        let mut rest = prose.as_str();
        while let Some(start) = rest.find("](") {
            rest = &rest[start + 2..];
            if let Some(target) = inline_target(rest) {
                targets.insert(normalize(target));
            }
        }
    }
    targets
}

/// `line` with its code spans, the text between two runs of the same number of backticks,
/// removed.
fn without_code_spans(line: &str) -> String {
    let mut prose = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(start) = rest.find('`') {
        prose.push_str(&rest[..start]);
        let after = &rest[start..];
        let ticks = after.len() - after.trim_start_matches('`').len();
        let run = &after[..ticks];
        let inside = &after[ticks..];
        let Some(end) = inside.find(run) else {
            prose.push_str(after);
            return prose;
        };
        rest = inside[end + ticks..].trim_start_matches('`');
    }
    prose.push_str(rest);
    prose
}

/// The target of the reference definition `[label]: target` on `line`, if it is one.
fn reference_target(line: &str) -> Option<&str> {
    let rest = line.trim_start().strip_prefix('[')?;
    let (_, after) = rest.split_once("]:")?;
    inline_target(after.trim_start())
}

/// The link target at the start of `rest`, the text after a link's `](`: up to the first space
/// or `)`, or between `<` and `>`.
fn inline_target(rest: &str) -> Option<&str> {
    let rest = rest.trim_start();
    let target = if let Some(bracketed) = rest.strip_prefix('<') {
        bracketed.split_once('>')?.0
    } else {
        let end = rest
            .find(|character: char| character == ')' || character.is_whitespace())
            .unwrap_or(rest.len());
        &rest[..end]
    };
    (!target.is_empty()).then_some(target)
}

/// `target` without a leading `./` or a trailing `#anchor`.
fn normalize(target: &str) -> String {
    let target = target.split_once('#').map_or(target, |(path, _)| path);
    target.strip_prefix("./").unwrap_or(target).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn links(text: &str) -> Vec<String> {
        linked_targets(text).into_iter().collect()
    }

    #[test]
    fn inline_and_reference_links_are_found() {
        let text = "# Project\n\nSee [install](docs/install.md) and \
                    [usage](./docs/usage.md#flags \"Usage\").\n\
                    Read [the design][design] and [<faq>](<docs/faq.md>).\n\n\
                    [design]: docs/design.md\n";
        assert_eq!(
            links(text),
            [
                "docs/design.md",
                "docs/faq.md",
                "docs/install.md",
                "docs/usage.md",
            ],
            "every link form names its target, without `./` or the anchor"
        );
    }

    #[test]
    fn links_in_code_dont_count() {
        let text = "```markdown\n[a](docs/a.md)\n```\n\n~~~\n[b](docs/b.md)\n~~~\n\
                    Write `[c](docs/c.md)` or ``[d](docs/d.md)``, then [e](docs/e.md).\n";
        assert_eq!(
            links(text),
            ["docs/e.md"],
            "only the link outside code blocks and spans counts"
        );
    }

    #[test]
    fn an_unclosed_code_span_is_prose() {
        assert_eq!(
            links("A stray ` and [a](docs/a.md).\n"),
            ["docs/a.md"],
            "a backtick with no closing run starts no code span"
        );
    }

    #[test]
    fn pages_are_markdown_files_directly_under_docs() {
        for path in ["docs/install.md", "docs/Guide.MD"] {
            assert!(is_page(path), "{path} is a page");
        }
        for path in [
            "docs/guides/setup.md",
            "docs/notes.txt",
            "README.md",
            "crates/app/docs/a.md",
            "Docs/a.md",
        ] {
            assert!(!is_page(path), "{path} isn't a page");
        }
    }

    #[test]
    fn the_suggested_link_is_titled_after_the_page() {
        assert_eq!(
            page_title("docs/install.md"),
            "install",
            "the title is the file stem"
        );
    }
}
