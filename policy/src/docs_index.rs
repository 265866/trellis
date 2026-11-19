//! Every page in `docs/` is linked: a page directly under `docs/` from the `Documentation`
//! section of the root `README.md`, and a page in a subdirectory from a page in the directory
//! above it.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use crate::markdown::{self, Markdown};
use crate::policy_error::PolicyError;
use crate::repository_files;
use crate::violation::Violation;
use crate::workspace::Workspace;

/// The file that links the docs, relative to the root.
pub(crate) const README: &str = "README.md";

/// The directory whose pages the readme links, relative to the root, without a trailing `/`.
pub(crate) const DOCS: &str = "docs";

/// The title of the readme's level-2 section that indexes the docs.
pub(crate) const INDEX: &str = "Documentation";

/// The byte order mark, which the checks report and then read past.
pub(crate) const BYTE_ORDER_MARK: char = '\u{feff}';

/// Checks that every page, a `.md` file at any depth under `docs/`, is linked with an inline
/// Markdown link, `[text](path)`, optionally with a `#anchor`. A page directly under `docs/` is
/// linked from the `## Documentation` section of the root `README.md`, with a path relative to
/// the root; links elsewhere in the readme don't count. A page in a subdirectory, such as
/// `docs/guides/setup.md`, is linked from a page in the directory above it, `docs/`, with a path
/// relative to that page. A page no one links is a page readers don't find. Links in code blocks
/// and code spans don't count, and neither do reference links, since the index holds only
/// inline ones.
///
/// It reads the files git tracks and the untracked ones `.gitignore` doesn't exclude.
///
/// # Errors
///
/// Fails if the repository's files can't be listed, or `README.md` or a page exists but can't be
/// read.
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
    let readme = read_readme(workspace.root())?;
    let index = readme.as_deref().and_then(index_targets);
    let mut page_links: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
    for page in &pages {
        let text = read_page(workspace.root(), page)?;
        let links = linked_targets(&Markdown::new(&text), directory(page));
        page_links.insert(page, links);
    }
    let mut violations = Vec::new();
    for page in &pages {
        let parent = directory(directory(page));
        if directory(page) == DOCS {
            if let Some(violation) = unindexed(page, readme.as_deref(), index.as_ref()) {
                violations.push(violation);
            }
            continue;
        }
        let linked = page_links
            .iter()
            .any(|(linker, links)| directory(linker) == parent && links.contains(*page));
        if !linked {
            let relative = &page[parent.len() + 1..];
            violations.push(Violation::new(
                (*page).to_owned(),
                format!(
                    "No page in `{parent}/` links this page, so readers don't find it. Add a \
                     link such as `[{title}]({relative})` to a page in `{parent}/`, or delete \
                     the page if nothing needs it.",
                    title = page_title(page)
                ),
            ));
        }
    }
    Ok(violations)
}

/// The violation of `page`, directly under `docs/`, if the readme's index doesn't link it.
/// `readme` is the readme, if there is one, and `index` the targets its index links, if it has
/// one.
fn unindexed(
    page: &str,
    readme: Option<&str>,
    index: Option<&BTreeSet<String>>,
) -> Option<Violation> {
    if index.is_some_and(|linked| linked.contains(page)) {
        return None;
    }
    let problem = match (readme, index) {
        (None, _) => format!("there is no root `{README}` to link the page `{page}`"),
        (Some(_), None) => {
            format!("`{README}` has no `## {INDEX}` section to link the page `{page}`")
        }
        (Some(_), Some(_)) => {
            format!("the `## {INDEX}` section of `{README}` doesn't link the page `{page}`")
        }
    };
    Some(Violation::new(
        README.to_owned(),
        format!(
            "{problem}, so readers don't find it. Add an entry such as \
             `- [{title}]({page}): what the reader does with it.` under `## {INDEX}` in \
             `{README}`, or delete the page if nothing needs it.",
            title = page_title(page)
        ),
    ))
}

/// The root `README.md` of the workspace at `root`, or `None` if there is none.
pub(crate) fn read_readme(root: &Path) -> Result<Option<String>, PolicyError> {
    let path = root.join(README);
    match fs::read_to_string(&path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(source) => Err(PolicyError::Read { path, source }),
    }
}

/// The text of `page`, a file relative to `root`.
pub(crate) fn read_page(root: &Path, page: &str) -> Result<String, PolicyError> {
    let path = root.join(page);
    fs::read_to_string(&path).map_err(|source| PolicyError::Read { path, source })
}

/// Whether `relative`, a file relative to the root, is a page: a `.md` file, in any case, at any
/// depth under `docs/`.
pub(crate) fn is_page(relative: &str) -> bool {
    is_under_docs(relative)
        && Path::new(relative)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
}

/// Whether `relative`, a file relative to the root, is under `docs/`.
pub(crate) fn is_under_docs(relative: &str) -> bool {
    relative
        .strip_prefix(DOCS)
        .is_some_and(|rest| rest.starts_with('/'))
}

/// The directory of `path`, relative to the root and without a trailing `/`, or `""` for a file
/// at the root.
fn directory(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(directory, _)| directory)
}

/// A title for a link to `page`: its file stem.
fn page_title(page: &str) -> &str {
    let name = page.rsplit_once('/').map_or(page, |(_, name)| name);
    name.rsplit_once('.').map_or(name, |(stem, _)| stem)
}

/// The targets the `## Documentation` section of `readme` links, relative to the root, or
/// `None` if it has no such section.
fn index_targets(readme: &str) -> Option<BTreeSet<String>> {
    let readme = readme.trim_start_matches(BYTE_ORDER_MARK);
    Markdown::new(readme)
        .section(INDEX)
        .map(|section| linked_targets(&section, ""))
}

/// The files the inline links in `markdown`, a file in `directory`, point to, relative to the
/// root and without a `#anchor`. Links in code don't count, and neither does a link that leaves
/// the repository or names a URL.
fn linked_targets(markdown: &Markdown<'_>, directory: &str) -> BTreeSet<String> {
    let mut targets = BTreeSet::new();
    for line in markdown.prose() {
        let prose = markdown::without_code_spans(line);
        let mut rest = prose.as_str();
        while let Some(start) = rest.find("](") {
            rest = &rest[start + 2..];
            if let Some(target) = inline_target(rest).and_then(|target| resolve(directory, target))
            {
                targets.insert(target);
            }
        }
    }
    targets
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

/// The file `target`, a link in a file in `directory`, points to, relative to the root and
/// without a `#anchor`, or `None` if it names a URL, starts with `/` or leaves the repository.
fn resolve(directory: &str, target: &str) -> Option<String> {
    let path = target.split_once('#').map_or(target, |(path, _)| path);
    let before_path = path.split('/').next().unwrap_or(path);
    if path.is_empty() || path.starts_with('/') || before_path.contains(':') {
        return None;
    }
    let mut parts: Vec<&str> = directory
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            _ => parts.push(part),
        }
    }
    Some(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn links(text: &str, directory: &str) -> Vec<String> {
        linked_targets(&Markdown::new(text), directory)
            .into_iter()
            .collect()
    }

    #[test]
    fn inline_links_are_found_and_reference_links_are_not() {
        let text = "# Project\n\nSee [install](docs/install.md) and \
                    [usage](./docs/usage.md#flags \"Usage\").\n\
                    Read [the design][design] and [<faq>](<docs/faq.md>).\n\n\
                    [design]: docs/design.md\n";
        assert_eq!(
            links(text, ""),
            ["docs/faq.md", "docs/install.md", "docs/usage.md"],
            "every inline link names its target, without `./` or the anchor"
        );
    }

    #[test]
    fn links_resolve_from_the_linking_page() {
        let text = "[a](guides/a.md), [b](./b.md#x), [c](../README.md), [d](../../x.md), \
                    [e](https://example.com/docs/e.md), [f](/docs/f.md)\n";
        assert_eq!(
            links(text, "docs"),
            ["README.md", "docs/b.md", "docs/guides/a.md"],
            "paths resolve from the page's directory, and URLs and escapes don't count"
        );
    }

    #[test]
    fn links_in_code_dont_count() {
        let text = "```markdown\n[a](docs/a.md)\n```\n\n~~~\n[b](docs/b.md)\n~~~\n\
                    Write `[c](docs/c.md)` or ``[d](docs/d.md)``, then [e](docs/e.md).\n\n\
                    \x20   [f](docs/f.md)\n";
        assert_eq!(
            links(text, ""),
            ["docs/e.md"],
            "only the link outside code blocks and spans counts"
        );
    }

    #[test]
    fn an_unclosed_code_span_is_prose() {
        assert_eq!(
            links("A stray ` and [a](docs/a.md).\n", ""),
            ["docs/a.md"],
            "a backtick with no closing run starts no code span"
        );
    }

    #[test]
    fn pages_are_md_files_at_any_depth_under_docs() {
        for path in ["docs/install.md", "docs/Guide.MD", "docs/guides/setup.md"] {
            assert!(is_page(path), "{path} is a page");
        }
        for path in [
            "docs/notes.txt",
            "docs/extra.markdown",
            "README.md",
            "crates/app/docs/a.md",
            "Docs/a.md",
            "docsx/a.md",
        ] {
            assert!(!is_page(path), "{path} isn't a page");
        }
    }

    #[test]
    fn only_links_in_the_documentation_section_count() {
        let readme = "# Project\n\n[Install](docs/install.md) has the details.\n\n\
                      ## Documentation\n\n- [Design](docs/design.md): the decisions.\n\n\
                      ### Pages\n\n- [Faq](docs/faq.md): answers.\n\n\
                      ## License\n\n[Guide](docs/guide.md).\n";
        let targets: Vec<String> = index_targets(readme).unwrap().into_iter().collect();
        assert_eq!(
            targets,
            ["docs/design.md", "docs/faq.md"],
            "a link before or after the section isn't in the index"
        );
        assert_eq!(
            index_targets("# Project\n\nSee [install](docs/install.md).\n"),
            None,
            "a readme without the section has no index"
        );
    }

    #[test]
    fn the_suggested_link_is_titled_after_the_page() {
        assert_eq!(
            page_title("docs/guides/install.md"),
            "install",
            "the title is the file stem"
        );
    }
}
