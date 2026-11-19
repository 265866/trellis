//! The block structure of a Markdown file, as far as the docs checks need it. It follows the
//! block rules of the Markdown specification at <https://spec.commonmark.org>, at the top level
//! and one list item or block quote deep: fenced and indented code, ATX and setext headings,
//! thematic breaks, paragraphs, list items, block quotes, HTML blocks and link reference
//! definitions, and GitHub's tables.

/// A Markdown file, as its lines, each with the block it belongs to.
#[derive(Clone, Debug)]
pub(crate) struct Markdown<'text> {
    /// The lines, in order.
    lines: Vec<Line<'text>>,
}

/// A line of a Markdown file, and the block it belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Line<'text> {
    /// The line as written.
    text: &'text str,
    /// The block the line belongs to.
    block: Block<'text>,
}

/// The block a line belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Block<'text> {
    /// A blank line.
    Blank,
    /// A line of a fenced code block, its fences included, or of an indented code block.
    Code,
    /// An ATX heading outside any list item or block quote.
    Heading(Heading<'text>),
    /// A line of a paragraph outside any list item or block quote.
    Paragraph,
    /// A line of `=` or `-` that makes the paragraph above it a setext heading.
    SetextUnderline,
    /// A thematic break, such as `***`.
    ThematicBreak,
    /// The first line of a list item, with its text after the marker.
    ListItem(&'text str),
    /// Another line inside a list item or a block quote, with its text after the quote's `>` or
    /// the item's indentation.
    Contained(&'text str),
    /// A line of an HTML block, where Markdown is text rather than links or headings.
    Html,
    /// A line of a table, or a link reference definition.
    Other,
}

/// An ATX heading, a line such as `## Install`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Heading<'text> {
    /// The number of `#` characters, from 1 to 6.
    level: usize,
    /// The text, without the `#` characters around it or the spaces beside them.
    text: &'text str,
}

/// The line that starts a list item.
#[derive(Clone, Copy, Debug)]
struct ListItem<'text> {
    /// The text after the marker and the spaces after it.
    text: &'text str,
    /// The column the item's content starts at, which its other lines are indented to.
    content_indent: usize,
    /// Whether the item can interrupt a paragraph: it has text, and it is a bullet or a number
    /// that is 1.
    interrupts_paragraph: bool,
}

/// The opening fence of a fenced code block.
#[derive(Clone, Copy, Debug)]
struct Fence {
    /// The fence character, `` ` `` or `~`.
    mark: char,
    /// How many of it the fence has.
    length: usize,
}

/// A list item or block quote that holds the lines after it.
#[derive(Clone, Copy, Debug)]
enum Container {
    /// A list item, whose lines are indented to its content.
    ListItem {
        /// The column the item's content starts at.
        content_indent: usize,
    },
    /// A block quote, whose lines start with `>`.
    Quote,
}

/// The top-level block that the next line may continue.
#[derive(Clone, Copy, Debug)]
enum Open {
    /// None: the next line starts a block.
    Nothing,
    /// A paragraph, which a line continues unless it starts a block that can interrupt it.
    Paragraph,
    /// An HTML block, which ends as `HtmlEnd` says.
    Html(HtmlEnd),
    /// A table, which ends at a blank line or a line that starts another block.
    Table,
}

/// What ends an HTML block.
#[derive(Clone, Copy, Debug)]
enum HtmlEnd {
    /// A blank line.
    BlankLine,
    /// The first line that holds this text, in any case.
    Marker(&'static str),
}

/// The state of the parse between one line and the next.
#[derive(Debug)]
struct Parser {
    /// The list item or block quote that holds the lines, if any.
    container: Option<Container>,
    /// The fenced code block the lines are in, inside the container if there is one.
    fence: Option<Fence>,
    /// The top-level block the next line may continue.
    open: Open,
    /// Whether the container's last line was paragraph text, which a line that starts no block
    /// continues even without the container's `>` or indentation.
    lazy: bool,
}

/// The tag names that start an HTML block that ends at a blank line, and may interrupt a
/// paragraph.
const BLOCK_TAGS: [&str; 62] = [
    "address",
    "article",
    "aside",
    "base",
    "basefont",
    "blockquote",
    "body",
    "caption",
    "center",
    "col",
    "colgroup",
    "dd",
    "details",
    "dialog",
    "dir",
    "div",
    "dl",
    "dt",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "frame",
    "frameset",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "head",
    "header",
    "hr",
    "html",
    "iframe",
    "legend",
    "li",
    "link",
    "main",
    "menu",
    "menuitem",
    "nav",
    "noframes",
    "ol",
    "optgroup",
    "option",
    "p",
    "param",
    "search",
    "section",
    "summary",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "title",
    "tr",
    "track",
    "ul",
];

impl<'text> Markdown<'text> {
    /// Splits `text` into lines and finds the block each belongs to.
    pub(crate) fn new(text: &'text str) -> Self {
        let raw: Vec<&str> = text.lines().collect();
        let mut parser = Parser {
            container: None,
            fence: None,
            open: Open::Nothing,
            lazy: false,
        };
        let mut lines = Vec::with_capacity(raw.len());
        for (index, line) in raw.iter().enumerate() {
            let block = parser.block(line, raw.get(index + 1).copied());
            lines.push(Line { text: line, block });
        }
        Self { lines }
    }

    /// The lines, in order.
    pub(crate) fn lines(&self) -> &[Line<'text>] {
        &self.lines
    }

    /// The lines outside code and HTML blocks, as written, in order.
    pub(crate) fn prose(&self) -> impl Iterator<Item = &'text str> {
        self.lines
            .iter()
            .filter(|line| !matches!(line.block, Block::Code | Block::Html))
            .map(|line| line.text)
    }

    /// The ATX headings outside code blocks, list items and block quotes, in order.
    pub(crate) fn headings(&self) -> impl Iterator<Item = Heading<'text>> {
        self.lines.iter().filter_map(|line| match line.block {
            Block::Heading(heading) => Some(heading),
            _ => None,
        })
    }

    /// The lines under the first level-2 heading whose text is `title`, up to the next heading of
    /// level 1 or 2, or `None` if there is no such heading.
    pub(crate) fn section(&self, title: &str) -> Option<Self> {
        let start = self.lines.iter().position(|line| {
            line.heading()
                .is_some_and(|heading| heading.level == 2 && heading.text == title)
        })? + 1;
        let length = self.lines[start..]
            .iter()
            .position(|line| line.heading().is_some_and(|heading| heading.level <= 2))
            .unwrap_or(self.lines.len() - start);
        Some(Self {
            lines: self.lines[start..start + length].to_vec(),
        })
    }

    /// The indexes of the lines that underline the paragraph above them, which makes it a
    /// setext heading.
    pub(crate) fn setext_underlines(&self) -> Vec<usize> {
        let mut underlines = Vec::new();
        for (index, line) in self.lines.iter().enumerate() {
            if line.block == Block::SetextUnderline {
                underlines.push(index);
            }
        }
        underlines
    }

    /// The indexes of the lines inside a list item or block quote that hold an ATX heading.
    pub(crate) fn contained_headings(&self) -> Vec<usize> {
        let mut headings = Vec::new();
        for (index, line) in self.lines.iter().enumerate() {
            if let Block::ListItem(text) | Block::Contained(text) = line.block
                && Heading::parse(text).is_some()
            {
                headings.push(index);
            }
        }
        headings
    }
}

impl<'text> Line<'text> {
    /// The line as written.
    pub(crate) fn text(self) -> &'text str {
        self.text
    }

    /// The block the line belongs to.
    pub(crate) fn block(self) -> Block<'text> {
        self.block
    }

    /// The ATX heading on the line, if it is one outside any list item or block quote.
    fn heading(self) -> Option<Heading<'text>> {
        match self.block {
            Block::Heading(heading) => Some(heading),
            _ => None,
        }
    }
}

impl<'text> Heading<'text> {
    /// The ATX heading on `line`, if it is one: up to three spaces, one to six `#` characters,
    /// then a space, a tab or the end of the line. A closing run of `#` characters after a space
    /// isn't part of the text.
    pub(crate) fn parse(line: &'text str) -> Option<Self> {
        let rest = line.trim_start_matches(' ');
        if line.len() - rest.len() > 3 {
            return None;
        }
        let level = rest.len() - rest.trim_start_matches('#').len();
        let after = &rest[level..];
        if !(1..=6).contains(&level) || !(after.is_empty() || after.starts_with([' ', '\t'])) {
            return None;
        }
        let text = after.trim();
        let open = text.trim_end_matches('#');
        let text = if open.is_empty() {
            open
        } else if open.ends_with([' ', '\t']) {
            open.trim_end()
        } else {
            text
        };
        Some(Self { level, text })
    }

    /// The number of `#` characters, from 1 to 6.
    pub(crate) fn level(self) -> usize {
        self.level
    }

    /// The text, without the `#` characters around it or the spaces beside them.
    pub(crate) fn text(self) -> &'text str {
        self.text
    }
}

impl Parser {
    /// The block of `text`, the next line, given `next`, the line after it.
    fn block<'text>(&mut self, text: &'text str, next: Option<&str>) -> Block<'text> {
        if let Some(container) = self.container {
            if let Some(inner) = container.inner(text) {
                return self.contained(text, inner);
            }
            if self.fence.is_none() && self.lazy && !is_blank(text) && !starts_block(text) {
                return Block::Contained(text.trim());
            }
            self.container = None;
            self.fence = None;
            self.lazy = false;
        }
        if let Some(fence) = self.fence {
            if fence.closes(text) {
                self.fence = None;
            }
            return Block::Code;
        }
        if is_blank(text) {
            self.open = Open::Nothing;
            return Block::Blank;
        }
        match self.open {
            Open::Html(end) => {
                if let HtmlEnd::Marker(marker) = end
                    && text.to_ascii_lowercase().contains(marker)
                {
                    self.open = Open::Nothing;
                }
                return Block::Html;
            }
            Open::Table if !starts_block(text) => return Block::Other,
            Open::Table => self.open = Open::Nothing,
            Open::Nothing | Open::Paragraph => {}
        }
        self.start(text, next)
    }

    /// The block of `text`, a top-level line that isn't blank and isn't in code, HTML or a
    /// table, given `next`, the line after it.
    fn start<'text>(&mut self, text: &'text str, next: Option<&str>) -> Block<'text> {
        let paragraph = matches!(self.open, Open::Paragraph);
        if indent(text) >= 4 {
            return if paragraph {
                Block::Paragraph
            } else {
                Block::Code
            };
        }
        if let Some(fence) = Fence::open(text) {
            self.fence = Some(fence);
            self.open = Open::Nothing;
            return Block::Code;
        }
        if let Some(heading) = Heading::parse(text) {
            self.open = Open::Nothing;
            return Block::Heading(heading);
        }
        if paragraph && is_setext_underline(text) {
            self.open = Open::Nothing;
            return Block::SetextUnderline;
        }
        if is_thematic_break(text) {
            self.open = Open::Nothing;
            return Block::ThematicBreak;
        }
        if let Some(inner) = quote_content(text) {
            self.open = Open::Nothing;
            self.container = Some(Container::Quote);
            return self.contained(text, inner);
        }
        if let Some(item) =
            ListItem::parse(text).filter(|item| !paragraph || item.interrupts_paragraph)
        {
            self.open = Open::Nothing;
            self.container = Some(Container::ListItem {
                content_indent: item.content_indent,
            });
            return match self.contained(text, item.text) {
                Block::Code => Block::Code,
                _ => Block::ListItem(item.text),
            };
        }
        if let Some(end) = html_start(text, self.open) {
            self.open = match end {
                HtmlEnd::Marker(marker)
                    if text
                        .to_ascii_lowercase()
                        .get(2..)
                        .is_some_and(|after| after.contains(marker)) =>
                {
                    Open::Nothing
                }
                HtmlEnd::Marker(_) | HtmlEnd::BlankLine => Open::Html(end),
            };
            return Block::Html;
        }
        if paragraph {
            return Block::Paragraph;
        }
        if text.contains('|') && next.is_some_and(is_table_delimiter) {
            self.open = Open::Table;
            return Block::Other;
        }
        if is_reference_definition(text) {
            return Block::Other;
        }
        self.open = Open::Paragraph;
        Block::Paragraph
    }

    /// The block of `text`, a line inside the container whose content on it is `inner`.
    fn contained<'text>(&mut self, text: &'text str, inner: &'text str) -> Block<'text> {
        if let Some(fence) = self.fence {
            if fence.closes(inner) {
                self.fence = None;
            }
            return Block::Code;
        }
        if is_blank(inner) {
            self.lazy = false;
            return if is_blank(text) {
                Block::Blank
            } else {
                Block::Contained(inner)
            };
        }
        if let Some(fence) = Fence::open(inner) {
            self.fence = Some(fence);
            self.lazy = false;
            return Block::Code;
        }
        self.lazy = indent(inner) < 4 && !starts_block(inner);
        Block::Contained(inner)
    }
}

impl Container {
    /// The content of `line` if it belongs to the container: after a quote's `>`, or after a
    /// list item's indentation, which a blank line belongs to as well.
    fn inner(self, line: &str) -> Option<&str> {
        match self {
            Self::Quote => quote_content(line),
            Self::ListItem { content_indent } => {
                if is_blank(line) {
                    Some("")
                } else if indent(line) >= content_indent {
                    Some(strip_indent(line, content_indent))
                } else {
                    None
                }
            }
        }
    }
}

impl<'text> ListItem<'text> {
    /// The list item that `line` starts, if it starts one: up to three spaces, then `-`, `*` or
    /// `+`, or one to nine digits and `.` or `)`, then a space, a tab or the end of the line.
    fn parse(line: &'text str) -> Option<Self> {
        let marker_indent = indent(line);
        if marker_indent > 3 {
            return None;
        }
        let rest = line.trim_start();
        let digits = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        let (marker_length, is_one) = if (1..=9).contains(&digits) {
            if !rest[digits..].starts_with(['.', ')']) {
                return None;
            }
            (digits + 1, rest[..digits].trim_start_matches('0') == "1")
        } else if digits == 0 && rest.starts_with(['-', '*', '+']) {
            (1, true)
        } else {
            return None;
        };
        let after = &rest[marker_length..];
        if !after.is_empty() && !after.starts_with([' ', '\t']) {
            return None;
        }
        let text = after.trim();
        let marker_end = marker_indent + marker_length;
        let spaces = indent(after);
        let content_indent = if text.is_empty() || spaces >= 5 {
            marker_end + 1
        } else {
            marker_end + spaces
        };
        Some(Self {
            text,
            content_indent,
            interrupts_paragraph: !text.is_empty() && is_one,
        })
    }
}

impl Fence {
    /// The opening fence on `line`, if it is one: up to three spaces, then three or more
    /// backticks or tildes. A backtick fence's info string holds no backtick.
    fn open(line: &str) -> Option<Self> {
        if indent(line) > 3 {
            return None;
        }
        let rest = line.trim_start();
        let mark = rest.chars().next().filter(|&c| c == '`' || c == '~')?;
        let length = rest.len() - rest.trim_start_matches(mark).len();
        let info = &rest[length..];
        (length >= 3 && !(mark == '`' && info.contains('`'))).then_some(Self { mark, length })
    }

    /// Whether `line` closes the block: up to three spaces, then at least as many of the same
    /// character as the opening fence, then only spaces or tabs.
    fn closes(self, line: &str) -> bool {
        let rest = line.trim_start();
        let length = rest.len() - rest.trim_start_matches(self.mark).len();
        indent(line) <= 3 && length >= self.length && rest[length..].trim().is_empty()
    }
}

/// Whether `line` holds only spaces and tabs.
fn is_blank(line: &str) -> bool {
    line.trim().is_empty()
}

/// The column the text of `line` starts at, with a tab moving to the next multiple of 4.
fn indent(line: &str) -> usize {
    let mut column = 0;
    for character in line.chars() {
        match character {
            ' ' => column += 1,
            '\t' => column += 4 - column % 4,
            _ => break,
        }
    }
    column
}

/// `line` without the spaces and tabs that indent it up to `columns`.
fn strip_indent(line: &str, columns: usize) -> &str {
    let mut column = 0;
    for (position, character) in line.char_indices() {
        if column >= columns {
            return &line[position..];
        }
        match character {
            ' ' => column += 1,
            '\t' => column += 4 - column % 4,
            _ => return &line[position..],
        }
    }
    ""
}

/// The content of the block quote line `line`, after its `>` markers and the space after each,
/// if it is one.
fn quote_content(line: &str) -> Option<&str> {
    if indent(line) > 3 {
        return None;
    }
    let mut rest = line.trim_start().strip_prefix('>')?;
    loop {
        rest = rest.strip_prefix([' ', '\t']).unwrap_or(rest);
        match rest.trim_start().strip_prefix('>') {
            Some(inner) if indent(rest) <= 3 => rest = inner,
            Some(_) | None => return Some(rest),
        }
    }
}

/// Whether `line` starts a block that ends a paragraph: a fence, an ATX heading, a thematic
/// break, a block quote, a list item that can interrupt a paragraph, or an HTML block that can.
fn starts_block(line: &str) -> bool {
    Fence::open(line).is_some()
        || Heading::parse(line).is_some()
        || is_thematic_break(line)
        || quote_content(line).is_some()
        || ListItem::parse(line).is_some_and(|item| item.interrupts_paragraph)
        || html_start(line, Open::Paragraph).is_some()
}

/// Whether `line` is a thematic break: three or more `-`, `*` or `_` of one kind, with spaces
/// allowed between them, after up to three spaces.
fn is_thematic_break(line: &str) -> bool {
    if indent(line) > 3 {
        return false;
    }
    let trimmed = line.trim();
    ['-', '*', '_'].into_iter().any(|mark| {
        let marks = trimmed.chars().filter(|&c| c == mark).count();
        marks >= 3 && trimmed.chars().all(|c| c == mark || c == ' ' || c == '\t')
    })
}

/// Whether `line` is a setext underline: a run of `=` or of `-` after up to three spaces, with
/// trailing spaces allowed.
fn is_setext_underline(line: &str) -> bool {
    if indent(line) > 3 {
        return false;
    }
    let trimmed = line.trim();
    ['=', '-']
        .into_iter()
        .any(|mark| !trimmed.is_empty() && trimmed.chars().all(|c| c == mark))
}

/// Whether `line` is the delimiter row under a table's header, such as `| --- | :-: |`: only
/// `|`, `:`, `-` and spaces, with at least one `|` and one `-`.
fn is_table_delimiter(line: &str) -> bool {
    line.contains('|')
        && line.contains('-')
        && line
            .chars()
            .all(|c| matches!(c, '|' | ':' | '-' | ' ' | '\t'))
}

/// Whether `line` is a link reference definition, such as `[install]: docs/install.md`.
fn is_reference_definition(line: &str) -> bool {
    indent(line) <= 3
        && line
            .trim_start()
            .strip_prefix('[')
            .and_then(|rest| rest.split_once("]:"))
            .is_some_and(|(label, _)| !label.trim().is_empty() && !label.contains(['[', ']']))
}

/// What ends the HTML block that `line` starts, if it starts one, as the seven kinds of HTML
/// block in the specification do. The last kind, a lone complete tag, can't interrupt a
/// paragraph, so it counts only when `open` isn't one.
fn html_start(line: &str, open: Open) -> Option<HtmlEnd> {
    if indent(line) > 3 {
        return None;
    }
    let lower = line.trim().to_ascii_lowercase();
    for (opener, end) in [("<!--", "-->"), ("<?", "?>"), ("<![cdata[", "]]>")] {
        if lower.starts_with(opener) {
            return Some(HtmlEnd::Marker(end));
        }
    }
    if lower
        .strip_prefix("<!")
        .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_alphabetic()))
    {
        return Some(HtmlEnd::Marker(">"));
    }
    let closing = lower.starts_with("</");
    let name_start = lower
        .strip_prefix("</")
        .or_else(|| lower.strip_prefix('<'))?;
    let name_length = name_start
        .find(|c: char| !c.is_ascii_alphanumeric() && c != '-')
        .unwrap_or(name_start.len());
    let name = &name_start[..name_length];
    let after = &name_start[name_length..];
    let ends_name =
        after.is_empty() || after.starts_with(['>', ' ', '\t']) || after.starts_with("/>");
    if !closing && ends_name {
        let raw_end = match name {
            "script" => Some("</script>"),
            "pre" => Some("</pre>"),
            "style" => Some("</style>"),
            "textarea" => Some("</textarea>"),
            _ => None,
        };
        if let Some(end) = raw_end {
            return Some(HtmlEnd::Marker(end));
        }
    }
    if ends_name && BLOCK_TAGS.contains(&name) {
        return Some(HtmlEnd::BlankLine);
    }
    let lone_tag = !name.is_empty()
        && name.starts_with(|c: char| c.is_ascii_alphabetic())
        && lower.ends_with('>')
        && !lower[1..lower.len() - 1].contains(['<', '>']);
    (lone_tag && !matches!(open, Open::Paragraph)).then_some(HtmlEnd::BlankLine)
}

/// `line` with its code spans, the text between two runs of the same number of backticks,
/// removed.
pub(crate) fn without_code_spans(line: &str) -> String {
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

/// `text` without its images, `![alt](source)`, and without the links left with no text once
/// they are gone, such as a badge's `[![alt](image)](target)`.
pub(crate) fn without_images(text: &str) -> String {
    let mut text = text.to_owned();
    for opener in ["![", "[]("] {
        while let Some(start) = text.find(opener) {
            let Some(close) = text[start..].find("](").map(|close| start + close) else {
                break;
            };
            let Some(end) = text[close..].find(')').map(|end| close + end) else {
                break;
            };
            text.replace_range(start..=end, "");
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heading(level: usize, text: &str) -> Heading<'_> {
        Heading { level, text }
    }

    fn blocks(text: &str) -> Vec<Block<'_>> {
        let markdown = Markdown::new(text);
        let mut blocks = Vec::new();
        for line in markdown.lines() {
            blocks.push(line.block());
        }
        blocks
    }

    fn headings(text: &str) -> Vec<&str> {
        Markdown::new(text).headings().map(Heading::text).collect()
    }

    #[test]
    fn atx_headings_are_parsed() {
        assert_eq!(
            Heading::parse("# Title"),
            Some(heading(1, "Title")),
            "a level-1 heading"
        );
        assert_eq!(
            Heading::parse("## Run it ##"),
            Some(heading(2, "Run it")),
            "a closing run is dropped"
        );
        assert_eq!(
            Heading::parse("   ### C#"),
            Some(heading(3, "C#")),
            "a `#` in the text stays"
        );
        assert_eq!(
            Heading::parse("##"),
            Some(heading(2, "")),
            "an empty heading"
        );
        assert_eq!(
            Heading::parse("## ##"),
            Some(heading(2, "")),
            "only a closing run"
        );
        for line in [
            "#Title",
            "    # Code",
            "####### Seven",
            "Text # not",
            "#\u{2009}x",
        ] {
            assert_eq!(Heading::parse(line), None, "{line:?} isn't an ATX heading");
        }
    }

    #[test]
    fn fences_follow_commonmark() {
        for text in [
            "```just ci``` runs the checks.\n\n## B\n",
            "Text:\n\n    ```\n\n## B\n",
            "````md\n```\n````\n\n## B\n",
            "~~~\n~~\n~~~\n## B\n",
            "```sh\n```  more\n```\n## B\n",
            "~~~\n```\n~~~\n## B\n",
            "```\n    ```\n```\n## B\n",
        ] {
            assert_eq!(headings(text), ["B"], "{text:?} closes or opens no fence");
        }
        for text in [
            "```sh\n## A\n```\n",
            "````\n```\n## A\n````\n",
            "~~~ `info`\n## A\n~~~\n",
            "   ```\n## A\n   ```\n",
            "```\n## A\n",
        ] {
            assert!(headings(text).is_empty(), "{text:?} fences its heading");
        }
        assert_eq!(
            blocks("```sh\nrun\n```\n"),
            [Block::Code, Block::Code, Block::Code],
            "the fences and what they hold are code"
        );
    }

    #[test]
    fn indented_code_is_code_unless_it_continues_a_paragraph() {
        assert_eq!(
            blocks("    ## A\n\n\t## B\nText\n    ## C\n"),
            [
                Block::Code,
                Block::Blank,
                Block::Code,
                Block::Paragraph,
                Block::Paragraph
            ],
            "four spaces or a tab start indented code, which can't interrupt a paragraph"
        );
    }

    #[test]
    fn setext_underlines_follow_any_paragraph() {
        let cases = [
            ("***\nExtra\n-----\n", vec![2]),
            ("    code\nExtra\n-----\n", vec![2]),
            ("Extra\n2. section\n---\n", vec![2]),
            ("Title\n===\n\nSection\n-------\n", vec![1, 4]),
            ("- item\n---\n", vec![]),
            ("Text\n- item\n---\n", vec![]),
            ("Text\n1. item\n---\n", vec![]),
            ("text\n\n---\n", vec![]),
            ("| a |\n| - |\n---\n", vec![]),
            ("> quote\n---\n", vec![]),
            ("## Heading\n---\n", vec![]),
        ];
        for (text, underlines) in cases {
            assert_eq!(
                Markdown::new(text).setext_underlines(),
                underlines,
                "the underlines in {text:?}"
            );
        }
    }

    #[test]
    fn headings_in_lists_and_quotes_are_contained() {
        let markdown = Markdown::new(
            "> ## A\n- ## B\n\n- item\n\n  ## C\n\n> > ## D\n\n## E\n- ```\n  ## F\n  ```\n",
        );
        assert_eq!(
            markdown.contained_headings(),
            [0, 1, 5, 7],
            "A to D are inside"
        );
        let top: Vec<&str> = markdown.headings().map(Heading::text).collect();
        assert_eq!(top, ["E"], "only E is a top-level heading, and F is fenced");
    }

    #[test]
    fn list_items_and_their_lines_are_found() {
        assert_eq!(
            blocks("- [A](a.md): a.\n  more\n* b\n1. c\nlazy\n\nText\n"),
            [
                Block::ListItem("[A](a.md): a."),
                Block::Contained("more"),
                Block::ListItem("b"),
                Block::ListItem("c"),
                Block::Contained("lazy"),
                Block::Blank,
                Block::Paragraph
            ],
            "items, their continuation lines and a lazy line"
        );
        for line in ["-a", "1.a", "a. b", "1234567890. b", "    - code"] {
            assert!(
                ListItem::parse(line).is_none(),
                "{line:?} isn't a list item"
            );
        }
    }

    #[test]
    fn html_tables_and_definitions_are_their_own_blocks() {
        assert_eq!(
            blocks("<!-- a\n## Hidden\n-->\nText\n"),
            [Block::Html, Block::Html, Block::Html, Block::Paragraph],
            "a comment runs to its end marker"
        );
        assert_eq!(
            blocks("<div>\n## Raw\n\n## Heading\n"),
            [
                Block::Html,
                Block::Html,
                Block::Blank,
                Block::Heading(heading(2, "Heading"))
            ],
            "a block tag runs to the blank line"
        );
        assert_eq!(
            blocks("<a href=\"x\">\n\n<b>text</b> more\n"),
            [Block::Html, Block::Blank, Block::Paragraph],
            "a lone tag is HTML, inline HTML in text isn't"
        );
        assert_eq!(
            blocks("a | b\n--- | ---\n1 | 2\n## H\n"),
            [
                Block::Other,
                Block::Other,
                Block::Other,
                Block::Heading(heading(2, "H"))
            ],
            "a table runs until another block starts"
        );
        assert_eq!(
            blocks("[install]: install.md\nText\n"),
            [Block::Other, Block::Paragraph],
            "a reference definition isn't a paragraph"
        );
    }

    #[test]
    fn a_section_runs_to_the_next_heading_of_level_two_or_less() {
        let markdown =
            Markdown::new("# T\n\n## Docs\n\n- a\n### Sub\n- b\n## Next\n- c\n## Docs\n- d\n");
        let section = markdown.section("Docs").unwrap();
        let lines: Vec<&str> = section.prose().collect();
        assert_eq!(
            lines,
            ["", "- a", "### Sub", "- b"],
            "the first matching section, up to the next level-2 heading"
        );
        assert!(
            markdown.section("Missing").is_none(),
            "a heading that isn't there has no section"
        );
    }

    #[test]
    fn images_and_code_spans_are_removed() {
        assert_eq!(
            without_images("[![CI](a.svg)](ci) ![x](y.png) text"),
            "  text",
            "badges and images go"
        );
        assert_eq!(
            without_code_spans("a `<h2>` b ``c`d`` e"),
            "a  b  e",
            "code spans go"
        );
    }
}
