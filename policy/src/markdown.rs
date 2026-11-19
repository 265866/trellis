//! Markdown parsed to match how GitHub renders it. The docs checks parse it with comrak, which
//! implements cmark-gfm, the parser GitHub renders Markdown with, turn on the extensions GitHub
//! uses (tables, strikethrough, autolinks, task lists and footnotes), and adjust the HTML block
//! starts where comrak follows a newer version of the specification than GitHub does.

use std::borrow::Cow;
use std::collections::BTreeSet;
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

/// An HTML element whose tags don't balance within a section.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Unbalanced {
    /// The 1-based line of the tag.
    pub(crate) line: usize,
    /// The element's name, in lowercase.
    pub(crate) name: String,
    /// Whether the tag is a closing tag that no open element matches, rather than an opening
    /// tag that doesn't close.
    pub(crate) closing: bool,
}

/// A tag in HTML.
#[derive(Clone, Debug)]
struct Tag {
    /// The element's name, in lowercase.
    name: String,
    /// Whether it is a closing tag, `</name>`.
    closing: bool,
    /// Whether it closes itself, `<name/>`.
    self_closing: bool,
    /// Its byte offset in the HTML.
    offset: usize,
}

/// The HTML elements whose tags must balance within a section: the standard elements that have
/// content. Void elements such as `<br>`, `<img>` and `<source>` have no closing tag, and a name
/// HTML doesn't define, such as the `F:` of `spawn<F: 'static>`, is no element GitHub keeps.
const ELEMENTS: [&str; 108] = [
    "a",
    "abbr",
    "address",
    "article",
    "aside",
    "audio",
    "b",
    "bdi",
    "bdo",
    "blockquote",
    "body",
    "button",
    "canvas",
    "caption",
    "center",
    "cite",
    "code",
    "colgroup",
    "data",
    "datalist",
    "dd",
    "del",
    "details",
    "dfn",
    "dialog",
    "dir",
    "div",
    "dl",
    "dt",
    "em",
    "fieldset",
    "figcaption",
    "figure",
    "font",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "head",
    "header",
    "hgroup",
    "html",
    "i",
    "iframe",
    "ins",
    "kbd",
    "label",
    "legend",
    "li",
    "main",
    "map",
    "mark",
    "menu",
    "meter",
    "nav",
    "noscript",
    "object",
    "ol",
    "optgroup",
    "option",
    "output",
    "p",
    "picture",
    "pre",
    "progress",
    "q",
    "rp",
    "rt",
    "ruby",
    "s",
    "samp",
    "script",
    "search",
    "section",
    "select",
    "slot",
    "small",
    "span",
    "strike",
    "strong",
    "style",
    "sub",
    "summary",
    "sup",
    "table",
    "tbody",
    "td",
    "template",
    "textarea",
    "tfoot",
    "th",
    "thead",
    "time",
    "title",
    "tr",
    "tt",
    "u",
    "ul",
    "var",
    "video",
    "big",
    "nobr",
    "marquee",
    "frameset",
];

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
    /// Footnote definitions aren't in any section: the parser moves them to the end.
    pub(crate) fn section(&self, title: &str) -> Option<Vec<Node<'tree>>> {
        let mut blocks = self.blocks();
        blocks
            .retain(|block| !matches!(block.data.borrow().value, NodeValue::FootnoteDefinition(_)));
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

    /// The 1-based lines with an HTML heading tag, `<h1>` to `<h6>` or its closing tag, in an
    /// HTML block or inline HTML, each line once.
    pub(crate) fn html_heading_lines(&self) -> Vec<usize> {
        let mut lines = BTreeSet::new();
        for node in self.document.descendants() {
            if let Some(html) = html(node) {
                for offset in html_heading_offsets(&html) {
                    lines.insert(first_line(node) + line_breaks(&html[..offset]));
                }
            }
        }
        lines.into_iter().collect()
    }

    /// The 1-based lines where an HTML block opens a comment, a quoted attribute value or a tag
    /// that doesn't close inside the block, which hides what follows from readers.
    pub(crate) fn unclosed_html_lines(&self) -> Vec<usize> {
        let mut lines = Vec::new();
        for node in self.document.descendants() {
            if let NodeValue::HtmlBlock(block) = &node.data.borrow().value
                && let Err(offset) = html_tags(&block.literal)
            {
                lines.push(first_line(node) + line_breaks(&block.literal[..offset]));
            }
        }
        lines
    }

    /// The HTML elements, of those [`ELEMENTS`] lists, whose tags don't balance within a
    /// section: between the start of the file, each top-level heading of level 1 or 2, and the
    /// end. An element left open makes GitHub render what follows inside it. Tags in code, in
    /// comments and in HTML that doesn't close don't count; [`Self::unclosed_html_lines`] reports
    /// the last. Neither does a `header` tag that [`github_block_tags`] wrote for a `<source>`
    /// tag, an element with no content.
    pub(crate) fn unbalanced_elements(&self) -> Vec<Unbalanced> {
        let mut problems = Vec::new();
        let mut open: Vec<Unbalanced> = Vec::new();
        for block in self.document.children() {
            if heading(block).is_some_and(|heading| heading.level <= 2) {
                problems.append(&mut open);
                continue;
            }
            for node in block.descendants() {
                let Some(html) = html(node) else {
                    continue;
                };
                let Ok(tags) = html_tags(&html) else {
                    continue;
                };
                for tag in tags {
                    let line = first_line(node) + line_breaks(&html[..tag.offset]);
                    let shimmed = tag.name == "header"
                        && !self.line(line).to_ascii_lowercase().contains("header");
                    if tag.self_closing || shimmed || !ELEMENTS.contains(&tag.name.as_str()) {
                        continue;
                    }
                    let element = Unbalanced {
                        line,
                        name: tag.name,
                        closing: tag.closing,
                    };
                    if !element.closing {
                        open.push(element);
                    } else if let Some(index) =
                        open.iter().rposition(|opened| opened.name == element.name)
                    {
                        problems.extend(open.drain(index + 1..));
                        open.pop();
                    } else {
                        problems.push(element);
                    }
                }
            }
        }
        problems.append(&mut open);
        problems.sort_by_key(|problem| problem.line);
        problems
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

/// The text of `node` and the nodes inside it, as GitHub renders it, without markup, images or
/// HTML.
pub(crate) fn text(node: Node<'_>) -> String {
    let mut text = String::new();
    if let Some(literal) = literal(node) {
        text.push_str(&literal);
    } else if is_break(node) {
        text.push(' ');
    } else if !is_image(node) {
        for child in node.children() {
            text.push_str(&self::text(child));
        }
    }
    text
}

/// The text of the inlines after `node`, its siblings that follow it, as [`text`] reads them.
pub(crate) fn text_after(node: Node<'_>) -> String {
    let mut text = String::new();
    let mut next = node.next_sibling();
    while let Some(sibling) = next {
        text.push_str(&self::text(sibling));
        next = sibling.next_sibling();
    }
    text
}

/// Whether `node` is an HTML block that holds only comments, each closed.
pub(crate) fn is_comment(node: Node<'_>) -> bool {
    let NodeValue::HtmlBlock(block) = &node.data.borrow().value else {
        return false;
    };
    let mut rest = block.literal.trim();
    while rest.starts_with("<!--") {
        let Some(end) = comment_end(rest) else {
            return false;
        };
        rest = rest[end..].trim_start();
    }
    rest.is_empty()
}

/// Whether `node` is a task list item, `- [ ] ...`.
pub(crate) fn is_task_item(node: Node<'_>) -> bool {
    matches!(node.data.borrow().value, NodeValue::TaskItem(_))
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

/// The HTML `node` holds, if it is an HTML block or inline HTML.
fn html(node: Node<'_>) -> Option<String> {
    let value = &node.data.borrow().value;
    if let NodeValue::HtmlBlock(block) = value {
        Some(block.literal.clone())
    } else if let NodeValue::HtmlInline(html) = value {
        Some(html.clone())
    } else {
        None
    }
}

/// How many line breaks `text` holds: `\n`, `\r\n` or a lone `\r`.
fn line_breaks(text: &str) -> usize {
    text.replace("\r\n", "\n").matches(['\n', '\r']).count()
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

/// `text` with the HTML block starts that comrak and GitHub read differently changed to read as
/// GitHub reads them, where a block can start: at the start of a line, or after its quote
/// markers, list markers or footnote label. comrak follows version 0.31 of the specification at
/// <https://spec.commonmark.org>, and GitHub's cmark-gfm version 0.29. Their HTML blocks differ
/// in three ways: `<search>` starts one only in 0.31, `<source>` only in 0.29, and `<!` starts
/// one before any letter in 0.31 but only before an uppercase letter in 0.29. So `search`
/// becomes `searcx`, which starts no block, `source` becomes `header`, which does, and the `<` of
/// `<!` before a lowercase letter becomes `&`, which makes the line text. Each change keeps the
/// length, so every line and column stays where it was.
fn github_block_tags(text: &str) -> String {
    let mut adjusted = String::with_capacity(text.len());
    for line in text.split_inclusive(['\n', '\r']) {
        let start = block_start(line);
        adjusted.push_str(&line[..start]);
        adjusted.push_str(&github_block_start(&line[start..]));
    }
    adjusted
}

/// `rest`, the text where a block can start, with an HTML block start changed to read as GitHub
/// reads it, as [`github_block_tags`] says.
fn github_block_start(rest: &str) -> Cow<'_, str> {
    let Some(after) = rest.strip_prefix('<') else {
        return Cow::Borrowed(rest);
    };
    if after
        .strip_prefix('!')
        .is_some_and(|declaration| declaration.starts_with(|c: char| c.is_ascii_lowercase()))
    {
        return Cow::Owned(format!("&{after}"));
    }
    let slash = usize::from(after.starts_with('/'));
    let name = &after[slash..];
    for (tag, replacement) in [("search", "searcx"), ("source", "header")] {
        let is_tag = name
            .get(..tag.len())
            .is_some_and(|candidate| candidate.eq_ignore_ascii_case(tag))
            && {
                let after = &name[tag.len()..];
                after.is_empty()
                    || after.starts_with([' ', '\t', '\n', '\r', '\x0b', '\x0c', '>'])
                    || after.starts_with("/>")
            };
        if is_tag {
            return Cow::Owned(format!(
                "<{}{replacement}{}",
                &after[..slash],
                &name[tag.len()..]
            ));
        }
    }
    Cow::Borrowed(rest)
}

/// Where a block could start on `line`: after its indentation, quote markers, list markers and
/// footnote label, such as `[^1]: `.
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
        } else if rest.starts_with("[^") {
            rest.find("]:").map_or(0, |end| end + 2)
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

/// The byte offsets in `html` of its HTML heading tags, `<h1>` to `<h6>` or their closing tags,
/// in any case, after a comment the HTML starts with, which ends at `-->` or `--!>`. Any byte but
/// a letter or a digit after `<h1` ends the tag name.
fn html_heading_offsets(html: &str) -> Vec<usize> {
    let lower = html.to_ascii_lowercase();
    let skipped = if lower.trim_start().starts_with("<!--") {
        let start = lower.len() - lower.trim_start().len();
        comment_end(&lower[start..]).map_or(lower.len(), |end| start + end)
    } else {
        0
    };
    let mut offsets = Vec::new();
    for opener in ["<h", "</h"] {
        for (start, _) in lower.match_indices(opener) {
            let after = &lower.as_bytes()[start + opener.len()..];
            let is_heading = start >= skipped
                && matches!(after.first(), Some(b'1'..=b'6'))
                && after
                    .get(1)
                    .is_none_or(|next| !next.is_ascii_alphanumeric());
            if is_heading {
                offsets.push(start);
            }
        }
    }
    offsets
}

/// The length of the comment `html` starts with, up to the end of its `-->` or `--!>`, or `None`
/// if it doesn't close. The search starts right after `<!`, so the `--` that opens the comment
/// can also close it, which makes `<!-->` and `<!--->` empty comments, as HTML has them.
fn comment_end(html: &str) -> Option<usize> {
    let body = html.get(2..)?;
    let ends = [
        body.find("-->").map(|end| end + 3),
        body.find("--!>").map(|end| end + 4),
    ];
    ends.into_iter().flatten().min().map(|end| end + 2)
}

/// The tags in `html`, an HTML block or inline HTML, in order, past its comments, processing
/// instructions, declarations and CDATA sections. `Err` holds the byte offset of the first of
/// those, or of a tag, that opens and doesn't close.
fn html_tags(html: &str) -> Result<Vec<Tag>, usize> {
    let mut tags = Vec::new();
    let mut position = 0;
    while let Some(found) = html[position..].find('<') {
        let start = position + found;
        let rest = &html[start..];
        let after = &rest[1..];
        let is_cdata = rest
            .get(..9)
            .is_some_and(|opener| opener.eq_ignore_ascii_case("<![cdata["));
        let starts_name = |text: &str| text.starts_with(|c: char| c.is_ascii_alphabetic());
        let length = if rest.starts_with("<!--") {
            comment_end(rest)
        } else if rest.starts_with("<?") {
            rest.find("?>").map(|end| end + 2)
        } else if is_cdata {
            rest.find("]]>").map(|end| end + 3)
        } else if after.strip_prefix('!').is_some_and(starts_name) {
            rest.find('>').map(|end| end + 1)
        } else if starts_name(after) || after.strip_prefix('/').is_some_and(starts_name) {
            let length = tag_end(rest).ok_or(start)?;
            let closing = after.starts_with('/');
            let name: String = after[usize::from(closing)..]
                .chars()
                .take_while(|c| !c.is_ascii_whitespace() && *c != '/' && *c != '>')
                .collect();
            tags.push(Tag {
                name: name.to_ascii_lowercase(),
                closing,
                self_closing: rest[..length].ends_with("/>"),
                offset: start,
            });
            Some(length)
        } else {
            Some(1)
        };
        position = start + length.ok_or(start)?;
    }
    Ok(tags)
}

/// The length of the tag `html` starts with, up to its closing `>`, read with HTML's grammar
/// for a tag: a name, then attributes, each a name with an optional `=` and a value, which is
/// quoted with `"` or `'` or runs to the next space or `>`. `None` if the tag doesn't close, or
/// if a `<` comes before its `>` outside a quoted value, since GitHub then reads the tag as
/// running on into what follows.
fn tag_end(html: &str) -> Option<usize> {
    let bytes = html.as_bytes();
    let is_space = |byte: u8| matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | b'\x0c');
    let mut index = 1 + usize::from(bytes.get(1) == Some(&b'/'));
    loop {
        let byte = *bytes.get(index)?;
        if byte == b'>' {
            return Some(index + 1);
        }
        if byte == b'<' {
            return None;
        }
        if is_space(byte) || byte == b'/' {
            index += 1;
            continue;
        }
        index += 1;
        while let Some(&next) = bytes.get(index) {
            if next == b'<' {
                return None;
            }
            if is_space(next) || matches!(next, b'=' | b'>' | b'/') {
                break;
            }
            index += 1;
        }
        let mut value = index;
        while bytes.get(value).is_some_and(|&next| is_space(next)) {
            value += 1;
        }
        if bytes.get(value) != Some(&b'=') {
            continue;
        }
        value += 1;
        while bytes.get(value).is_some_and(|&next| is_space(next)) {
            value += 1;
        }
        let quote = *bytes.get(value)?;
        if quote == b'"' || quote == b'\'' {
            let close = html[value + 1..].find(char::from(quote))?;
            index = value + 1 + close + 1;
        } else {
            index = value;
            while let Some(&next) = bytes.get(index) {
                if next == b'<' {
                    return None;
                }
                if is_space(next) || next == b'>' {
                    break;
                }
                index += 1;
            }
        }
    }
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
                "<search>x\n> - </SEARCH>\r\n<source src=a>\n`<search>` <searching>\n\
                 <search\x0cx\n[^1]: <search>x\n<!x\n- <!doctype html\n<!X\n<!--\n"
            ),
            "<searcx>x\n> - </searcx>\r\n<header src=a>\n`<search>` <searching>\n\
             <searcx\x0cx\n[^1]: <searcx>x\n&!x\n- &!doctype html\n<!X\n<!--\n",
            "the tags and lowercase declarations change where a block starts, and nowhere else"
        );
        assert_eq!(
            headings("<search>x\n## A\n\n<source>x\n## B\n\n<!x\n\n## C\n"),
            ["2 A", "2 C"],
            "a search tag and a lowercase declaration start no HTML block on GitHub, and a \
             source tag does"
        );
    }

    #[test]
    fn html_heading_tags_are_found_after_a_leading_comment() {
        for html in [
            "<h2>A</h2>",
            "<H1 id=a>",
            "text </h6>",
            "<h3\nclass=x>",
            "<div><!-- a --!> <h2>Extra</h2> --></div>",
            "<div><a b=\"<!--\"><h2>Extra</h2><i c=\"-->\"></i></div>",
            "<div><h2\x0cid=x>Extra</div>",
            "<div><h2\"id=x>Extra</div>",
            "<!-- a --> <h2>b</h2>",
        ] {
            assert!(
                !html_heading_offsets(html).is_empty(),
                "{html:?} has a heading tag"
            );
        }
        for html in [
            "<header>",
            "<hr>",
            "<!-- <h2>old</h2> -->",
            "<!-- <h2>old</h2> --!>",
            "<html>",
            "<h7>",
            "<h2x>",
        ] {
            assert!(
                html_heading_offsets(html).is_empty(),
                "{html:?} has no heading tag"
            );
        }
    }

    #[test]
    fn html_headings_are_reported_once_on_their_own_line() {
        let arena = Arena::new();
        let markdown = Markdown::parse(
            &arena,
            "# T\n\nText <h2>x</h2>.\n\n<div>\n\n</div>\n\n<div>\na\n<h3>b</h3>\n</div>\n",
        );
        assert_eq!(
            markdown.html_heading_lines(),
            [3, 11],
            "inline tags once per line, and a block's tag on its own line"
        );
    }

    #[test]
    fn unclosed_html_is_found() {
        for html in [
            "<!--\n",
            "<div><!-- a\n</div>\n",
            "<div title=\"\n</div>\n",
            "<div\n## Extra\n",
            "<?php echo\n",
            "<![CDATA[ x\n",
            "<div><![cdata[ x\n</div>\n",
            "<!DOCTYPE html\n",
            "<div it's title=\"it's>\nx\n</div>\n",
            "<pre>\ncat <<EOF\nhi\nEOF\n</pre>\n",
            "<pre>\nUsage: demo <file\n</pre>\n",
        ] {
            assert!(html_tags(html).is_err(), "{html:?} doesn't close");
        }
        for html in [
            "<details>\n<summary>More</summary>\n",
            "<pre>\n\n# a comment\n</pre>\n",
            "<!--\n\n## Old\n\n-->\n",
            "<!-- a --!>\n",
            "<!-->\n",
            "<div title='a > b'>1 < 2 <3</div>\n",
            "<div title=it's>\nx\n</div>\n",
            "<pre>\nfn spawn<F: 'static>(f: F)\n</pre>\n",
            "<img src=a.png alt=\"<b>\"/>\n",
            "<?php x ?>\n",
        ] {
            assert!(html_tags(html).is_ok(), "{html:?} closes");
        }
        let arena = Arena::new();
        let markdown = Markdown::parse(&arena, "# T\n\n<div>\na\n<!-- b\n\n## C\n");
        assert_eq!(
            markdown.unclosed_html_lines(),
            [5],
            "the line the comment opens on"
        );
    }

    #[test]
    fn elements_balance_within_a_section() {
        let unbalanced = |text: &str| {
            let arena = Arena::new();
            let markdown = Markdown::parse(&arena, text);
            let mut found = Vec::new();
            for element in markdown.unbalanced_elements() {
                let slash = if element.closing { "/" } else { "" };
                found.push(format!("{} <{slash}{}>", element.line, element.name));
            }
            found
        };
        for text in [
            "# T\n\n<details>\n<summary>More</summary>\n\n```sh\nrun\n```\n\n</details>\n",
            "# T\n\n<div align=\"center\">\n\n[![CI](a.svg)](b)\n\n</div>\n",
            "# T\n\n<picture>\n<source srcset=a.png>\n<img src=b.png>\n</picture>\n\nA<br>b.\n",
            "# T\n\nPress <kbd>Ctrl</kbd>. <!-- <div> -->\n\n`<div>`\n\n```\n<div>\n```\n",
            "# T\n\n<pre>\nfn spawn<F: 'static>(f: F)\n</pre>\n",
        ] {
            assert!(
                unbalanced(text).is_empty(),
                "{text:?} balances: {:?}",
                unbalanced(text)
            );
        }
        assert_eq!(
            unbalanced("# T\n\n<details>\n<summary>S</summary>\n\n```\nx\n```\n\n## A\n"),
            ["3 <details>"],
            "an element left open at the next section"
        );
        assert_eq!(
            unbalanced("# T\n\n<details>\n\n## Install\n\n</details>\n"),
            ["3 <details>", "7 </details>"],
            "a heading inside an element splits it"
        );
        assert_eq!(
            unbalanced("# T\n\n<div align=\"center\">\n\nText.\n\n</detail>\n"),
            ["3 <div>"],
            "a misspelled closing tag closes nothing"
        );
        assert_eq!(
            unbalanced("# T\n\n<div><b>x</div>\n\n<select>\n"),
            ["3 <b>", "5 <select>"],
            "an element closed by its parent's tag, and one left open at the end"
        );
    }

    #[test]
    fn comment_blocks_are_found() {
        let arena = Arena::new();
        let markdown = Markdown::parse(
            &arena,
            "<!-- SPDX-License-Identifier: MIT -->\n\n<!-- a -->\n<!-- b -->\n\n<!-- c\n\n\
             <div>\n\n<!-- d --> x\n",
        );
        let comments: Vec<bool> = markdown.blocks().into_iter().map(is_comment).collect();
        assert_eq!(
            comments,
            [true, true, true, false],
            "closed comments only, and an unclosed one runs to the end"
        );
    }

    #[test]
    fn text_leaves_out_images_and_html() {
        let arena = Arena::new();
        let markdown = Markdown::parse(&arena, "## ![I](i.png) A <kbd>B</kbd> `c`\n");
        assert_eq!(
            text(markdown.blocks()[0]),
            " A B c",
            "the image and the tags go"
        );
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
