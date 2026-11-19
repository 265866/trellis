//! The parts of a Markdown file the docs checks read: which lines are prose and which are code,
//! and the headings among the prose.

/// A Markdown file, as its lines, each marked as prose or as part of a fenced code block.
#[derive(Clone, Debug)]
pub(crate) struct Markdown<'text> {
    /// The lines, in order.
    lines: Vec<Line<'text>>,
}

/// A line of a Markdown file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Line<'text> {
    /// A line outside fenced code blocks, with its text.
    Prose(&'text str),
    /// A line of a fenced code block, its fences included.
    Code,
}

/// An ATX heading, a line such as `## Install`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Heading<'text> {
    /// The number of `#` characters, from 1 to 6.
    level: usize,
    /// The text, without the `#` characters around it or the spaces beside them.
    text: &'text str,
}

impl<'text> Markdown<'text> {
    /// Splits `text` into lines and marks the ones inside fenced code blocks, which open with
    /// ```` ``` ```` or `~~~` and close at the next line that starts with the same fence.
    pub(crate) fn new(text: &'text str) -> Self {
        let mut lines = Vec::new();
        let mut fence: Option<&str> = None;
        for line in text.lines() {
            let trimmed = line.trim_start();
            if let Some(open) = fence {
                if trimmed.starts_with(open) {
                    fence = None;
                }
                lines.push(Line::Code);
                continue;
            }
            if let Some(open) = ["```", "~~~"]
                .into_iter()
                .find(|open| trimmed.starts_with(open))
            {
                fence = Some(open);
                lines.push(Line::Code);
                continue;
            }
            lines.push(Line::Prose(line));
        }
        Self { lines }
    }

    /// The lines, in order.
    pub(crate) fn lines(&self) -> &[Line<'text>] {
        &self.lines
    }

    /// The prose lines, in order.
    pub(crate) fn prose(&self) -> impl Iterator<Item = &'text str> {
        self.lines.iter().filter_map(|line| match line {
            Line::Prose(text) => Some(*text),
            Line::Code => None,
        })
    }

    /// The ATX headings outside code blocks, in order.
    pub(crate) fn headings(&self) -> impl Iterator<Item = Heading<'text>> {
        self.prose().filter_map(Heading::parse)
    }

    /// The lines under the first level-2 heading whose text is `title`, up to the next heading of
    /// level 1 or 2, or `None` if there is no such heading.
    pub(crate) fn section(&self, title: &str) -> Option<Self> {
        let start = self.lines.iter().position(|line| {
            prose_heading(*line).is_some_and(|heading| heading.level == 2 && heading.text == title)
        })? + 1;
        let length = self.lines[start..]
            .iter()
            .position(|line| prose_heading(*line).is_some_and(|heading| heading.level <= 2))
            .unwrap_or(self.lines.len() - start);
        Some(Self {
            lines: self.lines[start..start + length].to_vec(),
        })
    }

    /// The indexes of the prose lines that underline the paragraph above them with `=` or `-`,
    /// which makes that paragraph a setext heading rather than text. A paragraph is a run of
    /// prose lines that starts after a blank line, a heading, a code block or the start of the
    /// file, with a line that doesn't start a list item, a block quote, a table row, an indented
    /// code block or a thematic break, and ends where one of those interrupts it. A list item's
    /// continuation lines are no paragraph, since an underline can't follow them.
    pub(crate) fn setext_underlines(&self) -> Vec<usize> {
        let mut underlines = Vec::new();
        let mut paragraph = false;
        let mut block_start = true;
        for (index, line) in self.lines.iter().enumerate() {
            let Line::Prose(text) = *line else {
                paragraph = false;
                block_start = true;
                continue;
            };
            if text.trim().is_empty() || Heading::parse(text).is_some() {
                paragraph = false;
                block_start = true;
                continue;
            }
            if paragraph && is_setext_underline(text) {
                underlines.push(index);
                paragraph = false;
                block_start = true;
                continue;
            }
            if block_start {
                paragraph = starts_paragraph(text);
            } else if is_list_item(text)
                || is_thematic_break(text)
                || text.trim_start().starts_with('>')
            {
                paragraph = false;
            }
            block_start = false;
        }
        underlines
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

/// The heading on `line`, if it is a prose line that holds one.
fn prose_heading(line: Line<'_>) -> Option<Heading<'_>> {
    match line {
        Line::Prose(text) => Heading::parse(text),
        Line::Code => None,
    }
}

/// Whether `line` starts a list item: `-`, `*` or `+`, or a number and `.` or `)`, then a space
/// or the end of the line, after any indentation.
pub(crate) fn is_list_item(line: &str) -> bool {
    list_item_text(line).is_some()
}

/// The text of the list item that `line` starts, after its marker and the spaces after it, or
/// `None` if `line` doesn't start one.
pub(crate) fn list_item_text(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    let digits = trimmed.len()
        - trimmed
            .trim_start_matches(|c: char| c.is_ascii_digit())
            .len();
    let rest = if digits > 0 {
        trimmed[digits..].strip_prefix(['.', ')'])
    } else {
        trimmed.strip_prefix(['-', '*', '+'])
    }?;
    (rest.is_empty() || rest.starts_with([' ', '\t'])).then(|| rest.trim())
}

/// Whether `line` is the delimiter row under a table's header, such as `| --- | :-: |`: only
/// `|`, `:`, `-` and spaces, with at least one `|` and one `-`.
pub(crate) fn is_table_delimiter(line: &str) -> bool {
    line.contains('|')
        && line.contains('-')
        && line
            .chars()
            .all(|c| matches!(c, '|' | ':' | '-' | ' ' | '\t'))
}

/// Whether `line` is a thematic break: three or more `-`, `*` or `_` of one kind, with spaces
/// allowed between them, after up to three spaces.
pub(crate) fn is_thematic_break(line: &str) -> bool {
    let trimmed = line.trim_start_matches(' ');
    if line.len() - trimmed.len() > 3 {
        return false;
    }
    ['-', '*', '_'].into_iter().any(|mark| {
        let marks = trimmed.chars().filter(|&c| c == mark).count();
        marks >= 3 && trimmed.chars().all(|c| c == mark || c == ' ' || c == '\t')
    })
}

/// Whether `line` is a setext underline: a run of `=` or of `-` after up to three spaces, with
/// trailing spaces allowed.
fn is_setext_underline(line: &str) -> bool {
    let trimmed = line.trim_start_matches(' ');
    if line.len() - trimmed.len() > 3 {
        return false;
    }
    let trimmed = trimmed.trim_end();
    ['=', '-']
        .into_iter()
        .any(|mark| !trimmed.is_empty() && trimmed.chars().all(|c| c == mark))
}

/// Whether the prose line `line`, the first of a block, starts a paragraph rather than a list
/// item, a block quote, a table row, an indented code block or a thematic break.
pub(crate) fn starts_paragraph(line: &str) -> bool {
    let trimmed = line.trim_start();
    let indent = line.len() - trimmed.len();
    indent < 4
        && !trimmed.is_empty()
        && Heading::parse(line).is_none()
        && !is_list_item(line)
        && !is_thematic_break(line)
        && !trimmed.starts_with(['>', '|'])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heading(level: usize, text: &str) -> Heading<'_> {
        Heading { level, text }
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
    }

    #[test]
    fn other_lines_are_not_headings() {
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
    fn fenced_code_is_not_prose() {
        let markdown = Markdown::new("# A\n```sh\n# not a heading\n```\n~~~\n## B\n~~~\n## C\n");
        let headings: Vec<&str> = markdown.headings().map(Heading::text).collect();
        assert_eq!(headings, ["A", "C"], "headings inside fences don't count");
        assert_eq!(
            markdown.lines()[1..4],
            [Line::Code, Line::Code, Line::Code],
            "the fences and what they hold are code"
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
        let last = Markdown::new("## Docs\n- a\n").section("Docs").unwrap();
        assert_eq!(
            last.prose().collect::<Vec<_>>(),
            ["- a"],
            "a section at the end runs to the end of the file"
        );
    }

    #[test]
    fn setext_underlines_after_a_paragraph_are_found() {
        let text = "Title\n===\n\nSection\n-------\n\n- item\n---\n\ntext\n\n---\n\n| a |\n---\n\
                    \n> quote\n---\n\n## Heading\n---\n\ntext\n- item\n---\n\ntext\n> quote\n---\n";
        let markdown = Markdown::new(text);
        assert_eq!(
            markdown.setext_underlines(),
            [1, 4],
            "only the underlines below a paragraph make it a heading"
        );
    }

    #[test]
    fn list_items_and_breaks_are_recognized() {
        for line in ["- a", "* a", "+ a", "1. a", "12) a", "-", "  - nested"] {
            assert!(is_list_item(line), "{line:?} is a list item");
        }
        for line in ["-a", "1.a", "a. b", "text", ""] {
            assert!(!is_list_item(line), "{line:?} isn't a list item");
        }
        assert_eq!(
            list_item_text("-   [A](a.md): b. "),
            Some("[A](a.md): b."),
            "the text follows the marker"
        );
        assert_eq!(list_item_text("2) x"), Some("x"), "an ordered item");
        for line in ["| --- | :-: |", "--- | ---", "|-|"] {
            assert!(is_table_delimiter(line), "{line:?} is a delimiter row");
        }
        for line in ["---", "| a |", "a | -"] {
            assert!(!is_table_delimiter(line), "{line:?} isn't a delimiter row");
        }
        for line in ["---", "* * *", "___", "   ----"] {
            assert!(is_thematic_break(line), "{line:?} is a thematic break");
        }
        for line in ["--", "-- x", "    ---", "=-="] {
            assert!(!is_thematic_break(line), "{line:?} isn't a thematic break");
        }
    }

    #[test]
    fn only_plain_text_starts_a_paragraph() {
        assert!(starts_paragraph("Who it is for."), "plain text");
        assert!(starts_paragraph("[A link](x.md) first."), "a link");
        for line in ["", "# H", "- a", "1. a", "| a |", "> q", "    code", "---"] {
            assert!(
                !starts_paragraph(line),
                "{line:?} doesn't start a paragraph"
            );
        }
    }
}
