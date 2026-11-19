//! Markdown read the way GitHub reads it. The docs checks parse it with comrak, which implements
//! cmark-gfm, the parser GitHub renders Markdown with, and turn on the extensions GitHub uses:
//! tables, strikethrough, autolinks, task lists and footnotes.

use std::ops::RangeInclusive;

use comrak::nodes::{Node, NodeHeading, NodeValue};
use comrak::{Arena, Options};

/// A Markdown file, parsed into the tree of blocks and inlines GitHub renders.
#[derive(Clone, Debug)]
pub(crate) struct Markdown<'tree> {
    /// The document node, the root of the tree.
    document: Node<'tree>,
    /// The file's lines, split at `\n`, `\r\n` or a lone `\r` as the parser splits them.
    lines: Vec<&'tree str>,
}

/// A heading, written with `#` characters or underlined with `=` or `-`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Heading {
    /// The level, from 1 to 6.
    pub(crate) level: u8,
    /// The text, as GitHub renders it, without markup.
    pub(crate) text: String,
    /// Whether the heading is a setext one, a line underlined with `=` or `-`.
    pub(crate) setext: bool,
    /// The 1-based line the heading ends on, its underline for a setext heading.
    pub(crate) last_line: usize,
    /// Whether the heading is a top-level block, outside any list, quote or footnote.
    pub(crate) top_level: bool,
}

impl<'tree> Markdown<'tree> {
    /// Parses `text` into a tree whose nodes `arena` holds.
    pub(crate) fn parse(arena: &'tree Arena<'tree>, text: &'tree str) -> Self {
        let mut options = Options::default();
        options.extension.table = true;
        options.extension.strikethrough = true;
        options.extension.autolink = true;
        options.extension.tasklist = true;
        options.extension.footnotes = true;
        Self {
            document: comrak::parse_document(arena, &github_block_tags(text), &options),
            lines: split_lines(text),
        }
    }

    /// The top-level blocks, in order.
    pub(crate) fn blocks(&self) -> Vec<Node<'tree>> {
        self.document.children().collect()
    }

    /// Every heading at any depth, in order.
    pub(crate) fn headings(&self) -> Vec<Heading> {
        let mut headings = Vec::new();
        for node in self.document.descendants() {
            if let Some(heading) = heading(node) {
                headings.push(Heading {
                    level: heading.level,
                    text: text(node),
                    setext: heading.setext,
                    last_line: last_line(node),
                    top_level: node
                        .parent()
                        .is_some_and(|parent| parent.same_node(self.document)),
                });
            }
        }
        headings
    }

    /// The top-level blocks after the first top-level level-2 heading whose text is `title`, up
    /// to the next top-level heading of level 1 or 2, or `None` if there is no such heading.
    pub(crate) fn section(&self, title: &str) -> Option<Vec<Node<'tree>>> {
        let blocks = self.blocks();
        let start = blocks.iter().position(|block| {
            heading(block).is_some_and(|heading| heading.level == 2) && text(block) == title
        })? + 1;
        let length = blocks[start..]
            .iter()
            .position(|block| heading(block).is_some_and(|heading| heading.level <= 2))
            .unwrap_or(blocks.len() - start);
        Some(blocks[start..start + length].to_vec())
    }

    /// The 1-based line `number` as written, without its line break, or `""` past the end.
    pub(crate) fn line(&self, number: usize) -> &'tree str {
        number
            .checked_sub(1)
            .and_then(|index| self.lines.get(index))
            .copied()
            .unwrap_or("")
    }

    /// Whether the file has a non-blank line after the 1-based line `number`.
    pub(crate) fn has_text_after(&self, number: usize) -> bool {
        self.lines
            .iter()
            .skip(number)
            .any(|line| !line.trim().is_empty())
    }

    /// The 1-based lines that start an HTML block or inline HTML holding an HTML heading tag,
    /// `<h1>` to `<h6>` or its closing tag, outside HTML comments.
    pub(crate) fn html_heading_lines(&self) -> Vec<usize> {
        let mut lines = Vec::new();
        for node in self.document.descendants() {
            let value = &node.data.borrow().value;
            let has_heading = if let NodeValue::HtmlBlock(block) = value {
                has_html_heading(&block.literal)
            } else if let NodeValue::HtmlInline(html) = value {
                has_html_heading(html)
            } else {
                false
            };
            if has_heading {
                lines.push(first_line(node));
            }
        }
        lines
    }
}

/// The heading `node` is, if it is one.
pub(crate) fn heading(node: Node<'_>) -> Option<NodeHeading> {
    if let NodeValue::Heading(heading) = &node.data.borrow().value {
        Some(heading.clone())
    } else {
        None
    }
}

/// The 1-based line `node` starts on.
pub(crate) fn first_line(node: Node<'_>) -> usize {
    node.data.borrow().sourcepos.start.line
}

/// The 1-based line `node` ends on.
pub(crate) fn last_line(node: Node<'_>) -> usize {
    node.data.borrow().sourcepos.end.line
}

/// The 1-based columns of the first and the last byte of `node` on their lines.
pub(crate) fn columns(node: Node<'_>) -> RangeInclusive<usize> {
    let sourcepos = node.data.borrow().sourcepos;
    sourcepos.start.column..=sourcepos.end.column
}

/// The text of `node` and the nodes inside it, as GitHub renders it, without markup.
pub(crate) fn text(node: Node<'_>) -> String {
    let mut text = String::new();
    for descendant in node.descendants() {
        if let Some(literal) = literal(descendant) {
            text.push_str(&literal);
        } else if is_break(descendant) {
            text.push(' ');
        }
    }
    text
}

/// Whether `node` has a letter in its text outside images, so it says something in words.
pub(crate) fn has_words(node: Node<'_>) -> bool {
    node.children().any(|child| {
        let words = literal(child).is_some_and(|literal| literal.chars().any(char::is_alphabetic));
        !is_image(child) && (words || has_words(child))
    })
}

/// Whether `node`, a paragraph, holds only badges: images, links around them and spaces.
pub(crate) fn is_badges(node: Node<'_>) -> bool {
    let mut images = 0_usize;
    let mut other = false;
    for child in node.children() {
        let is_link = matches!(child.data.borrow().value, NodeValue::Link(_));
        let is_space =
            matches!(&child.data.borrow().value, NodeValue::Text(text) if text.trim().is_empty());
        let inside: Vec<Node<'_>> = child.children().collect();
        if is_image(child)
            || (is_link && !inside.is_empty() && inside.iter().all(|inner| is_image(inner)))
        {
            images += 1;
        } else if !(is_space || is_break(child)) {
            other = true;
        }
    }
    images > 0 && !other
}

/// The text `node` holds itself, if it is text or a code span.
fn literal(node: Node<'_>) -> Option<String> {
    let value = &node.data.borrow().value;
    if let NodeValue::Text(text) = value {
        Some(text.to_string())
    } else if let NodeValue::Code(code) = value {
        Some(code.literal.clone())
    } else {
        None
    }
}

/// Whether `node` is a line break, soft or hard.
fn is_break(node: Node<'_>) -> bool {
    matches!(
        node.data.borrow().value,
        NodeValue::SoftBreak | NodeValue::LineBreak
    )
}

/// Whether `node` is an image.
fn is_image(node: Node<'_>) -> bool {
    matches!(node.data.borrow().value, NodeValue::Image(_))
}

/// The destinations of the links in `node` and the nodes inside it, in order, as GitHub
/// renders them: inline and reference links and autolinks, but not images.
pub(crate) fn link_urls(node: Node<'_>) -> Vec<String> {
    let mut urls = Vec::new();
    for descendant in node.descendants() {
        if let NodeValue::Link(link) = &descendant.data.borrow().value {
            urls.push(link.url.clone());
        }
    }
    urls
}

/// `text` with the HTML block tags whose meaning comrak and GitHub disagree on renamed, at the
/// start of a line or after its quote and list markers, where a block can start. comrak follows
/// version 0.31 of the specification at <https://spec.commonmark.org>, which made `<search>`
/// start an HTML block and `<source>` not, while GitHub's cmark-gfm follows version 0.29, which
/// has it the other way round. `search` becomes `searcx`, which no
/// block starts with, and `source` becomes `header`, which one does, so comrak reads each line
/// as GitHub does, and a tag of the same length keeps every line and column where it was.
fn github_block_tags(text: &str) -> String {
    let mut adjusted = String::with_capacity(text.len());
    for line in text.split_inclusive(['\n', '\r']) {
        let start = block_start(line);
        let rest = &line[start..];
        let slash = usize::from(rest.starts_with("</"));
        let name = rest.get(1 + slash..).unwrap_or("");
        let renamed = [("search", "searcx"), ("source", "header")]
            .into_iter()
            .find(|(tag, _)| {
                name.get(..tag.len())
                    .is_some_and(|candidate| candidate.eq_ignore_ascii_case(tag))
                    && name[tag.len()..]
                        .chars()
                        .next()
                        .is_none_or(|next| matches!(next, ' ' | '\t' | '>' | '/' | '\n' | '\r'))
            });
        match renamed {
            Some((tag, replacement)) if rest.starts_with('<') => {
                let tag_start = start + 1 + slash;
                adjusted.push_str(&line[..tag_start]);
                adjusted.push_str(replacement);
                adjusted.push_str(&line[tag_start + tag.len()..]);
            }
            Some(_) | None => adjusted.push_str(line),
        }
    }
    adjusted
}

/// Where a block could start on `line`: after its indentation, quote markers and list markers.
fn block_start(line: &str) -> usize {
    let mut position = 0;
    loop {
        let rest = &line[position..];
        let skipped = rest.len() - rest.trim_start_matches([' ', '\t', '>']).len();
        position += skipped;
        let rest = &line[position..];
        let digits = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        let marker = if rest.starts_with(['-', '*', '+']) {
            1
        } else if (1..=9).contains(&digits) && rest[digits..].starts_with(['.', ')']) {
            digits + 1
        } else {
            0
        };
        if marker > 0 && rest[marker..].starts_with([' ', '\t']) {
            position += marker;
        } else if skipped == 0 {
            return position;
        }
    }
}

/// `text` split into lines at `\n`, `\r\n` or a lone `\r`, without the line breaks.
fn split_lines(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut rest = text;
    while let Some(end) = rest.find(['\n', '\r']) {
        lines.push(&rest[..end]);
        let break_length = if rest[end..].starts_with("\r\n") {
            2
        } else {
            1
        };
        rest = &rest[end + break_length..];
    }
    if !rest.is_empty() {
        lines.push(rest);
    }
    lines
}

/// Whether `html` holds an HTML heading tag, `<h1>` to `<h6>` or its closing tag, in any case,
/// outside HTML comments.
fn has_html_heading(html: &str) -> bool {
    let mut lower = html.to_ascii_lowercase();
    while let Some(start) = lower.find("<!--") {
        let end = lower[start..]
            .find("-->")
            .map_or(lower.len(), |end| start + end + 3);
        lower.replace_range(start..end, "");
    }
    ["<h", "</h"].into_iter().any(|opener| {
        lower.match_indices(opener).any(|(start, _)| {
            let after = &lower.as_bytes()[start + opener.len()..];
            matches!(after.first(), Some(b'1'..=b'6'))
                && after
                    .get(1)
                    .is_none_or(|next| matches!(next, b'>' | b' ' | b'\t' | b'\n' | b'/'))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headings(text: &str) -> Vec<String> {
        let arena = Arena::new();
        let markdown = Markdown::parse(&arena, text);
        let mut found = Vec::new();
        for heading in markdown.headings() {
            found.push(format!("{} {}", heading.level, heading.text));
        }
        found
    }

    #[test]
    fn headings_are_found_at_any_depth_and_not_in_code_or_html() {
        assert_eq!(
            headings("# A\n\n- ## B\n\n> ### C\n\n```\n## D\n```\n\n<!--\n\n## E\n\n-->\n"),
            ["1 A", "2 B", "3 C"],
            "headings in lists and quotes count, and those in code and comments don't"
        );
        assert_eq!(
            headings("Text\n---\n\n- Item\n  ===\n"),
            ["2 Text", "1 Item"],
            "setext headings count, in a list item too"
        );
    }

    #[test]
    fn a_section_holds_the_blocks_up_to_the_next_heading_of_level_two() {
        let arena = Arena::new();
        let markdown = Markdown::parse(
            &arena,
            "# T\n\n## Docs\n\n- a\n\n### Sub\n\nText\n\n## Next\n\n- b\n",
        );
        let section = markdown.section("Docs").unwrap();
        let lines: Vec<usize> = section.iter().map(|block| first_line(block)).collect();
        assert_eq!(
            lines,
            [5, 7, 9],
            "the list, the level-3 heading and the paragraph"
        );
        assert!(markdown.section("Missing").is_none(), "no such heading");
    }

    #[test]
    fn lines_split_at_every_line_break() {
        assert_eq!(
            split_lines("a\r\nb\rc\nd"),
            ["a", "b", "c", "d"],
            "CRLF, a lone CR and LF each end a line"
        );
        let arena = Arena::new();
        let markdown = Markdown::parse(&arena, "# A\r\rText.\r## B\r");
        assert_eq!(markdown.line(3), "Text.", "lines count from 1");
        assert_eq!(
            markdown.headings().last().map(|heading| heading.last_line),
            Some(4),
            "the parser's lines match"
        );
    }

    #[test]
    fn block_tags_follow_github() {
        assert_eq!(
            github_block_tags(
                "<search>x\n> - </SEARCH>\r\n<source src=a>\n`<search>` <searching>\n"
            ),
            "<searcx>x\n> - </searcx>\r\n<header src=a>\n`<search>` <searching>\n",
            "the two tags are renamed where a block starts, and nowhere else"
        );
        assert_eq!(
            headings("<search>x\n## A\n\n<source>x\n## B\n"),
            ["2 A"],
            "a search tag starts no HTML block on GitHub, and a source tag does"
        );
    }

    #[test]
    fn html_heading_tags_are_found_outside_comments() {
        for html in ["<h2>A</h2>", "<H1 id=a>", "text </h6>", "<h3\nclass=x>"] {
            assert!(has_html_heading(html), "{html:?} has a heading tag");
        }
        for html in [
            "<header>",
            "<hr>",
            "<!-- <h2>old</h2> -->",
            "<html>",
            "<h7>",
        ] {
            assert!(!has_html_heading(html), "{html:?} has no heading tag");
        }
    }

    #[test]
    fn words_badges_and_links_are_read_from_inlines() {
        let arena = Arena::new();
        let markdown = Markdown::parse(
            &arena,
            "![Logo](a.png)\n\n[![CI](b.svg)](c) [![R][r]][s]\n\n<https://example.com> has it.\n\n\
             See [a](a.md), [b][b] and <https://x.example>.\n\n[r]: r.svg\n[s]: s\n[b]: b.md\n",
        );
        let blocks = markdown.blocks();
        assert!(!has_words(blocks[0]), "a lone image has no words");
        assert!(
            is_badges(blocks[1]),
            "inline and reference badges are badges"
        );
        assert!(has_words(blocks[2]), "an autolink's text is words");
        assert!(!is_badges(blocks[2]), "text isn't a badge");
        assert_eq!(
            link_urls(blocks[3]),
            ["a.md", "b.md", "https://x.example"],
            "inline, reference and autolinks"
        );
    }
}
