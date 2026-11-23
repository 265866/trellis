//! HTML in Markdown, read as far as the docs checks need: its tags, comments and heading tags.

use std::collections::BTreeSet;
use std::sync::LazyLock;

/// The text of an HTML block or of inline HTML.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Html<'text> {
    /// The HTML, as the Markdown holds it.
    text: &'text str,
}

/// A tag in HTML.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Tag {
    /// The element's name, in lowercase.
    pub(crate) name: String,
    /// Whether it is a closing tag, `</name>`.
    pub(crate) closing: bool,
    /// Whether it closes itself: a `/` right before its `>`, outside an attribute value.
    pub(crate) self_closing: bool,
    /// Its byte offset in the HTML.
    pub(crate) offset: usize,
    /// Its length in bytes, up to its `>`.
    length: usize,
    /// Its attributes, in order.
    attributes: Vec<Attribute>,
}

/// A piece of HTML.
#[derive(Clone, Debug)]
enum Piece<'text> {
    /// A tag.
    Tag(Tag),
    /// The text between two tags, comments and the like.
    Text(&'text str),
}

/// An attribute of a tag.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Attribute {
    /// Its name, in lowercase.
    name: String,
    /// Its value without quotes and with its entities decoded, or `""` when it has none.
    value: String,
}

/// The HTML elements, current and obsolete, that have content, so that their tags must balance
/// within a section. Void elements such as `<br>`, `<img>` and `<source>` have no closing tag,
/// and a name HTML doesn't define, such as the `F:` of `spawn<F: 'static>`, is no element GitHub
/// keeps.
const ELEMENTS: &str = "a abbr address article aside audio b bdi bdo big blockquote body button \
                        canvas caption center cite code colgroup data datalist dd del details \
                        dfn dialog dir div dl dt em fieldset figcaption figure font footer form \
                        frameset h1 h2 h3 h4 h5 h6 head header hgroup html i iframe ins kbd label \
                        legend li main map mark marquee menu meter nav nobr noscript object ol \
                        optgroup option output p picture pre progress q rp rt ruby s samp script \
                        search section select slot small span strike strong style sub summary \
                        sup table tbody td template textarea tfoot th thead time title tr tt u ul \
                        var video";

/// The elements of [`ELEMENTS`] whose closing tag HTML lets a document leave out, since the
/// next element closes them, such as `<li>` and `<p>`.
const OPTIONAL_END_ELEMENTS: &str = "body caption colgroup dd dt head html li optgroup option p \
                                     rp rt tbody td tfoot th thead tr";

/// [`ELEMENTS`], split into names once.
static ELEMENT_NAMES: LazyLock<BTreeSet<&str>> =
    LazyLock::new(|| ELEMENTS.split_ascii_whitespace().collect());

/// [`OPTIONAL_END_ELEMENTS`], split into names once.
static OPTIONAL_END_NAMES: LazyLock<BTreeSet<&str>> =
    LazyLock::new(|| OPTIONAL_END_ELEMENTS.split_ascii_whitespace().collect());

impl<'text> Html<'text> {
    /// The HTML `text`.
    pub(crate) fn new(text: &'text str) -> Self {
        Self { text }
    }

    /// The tags in the HTML, in order, past its comments, processing instructions, declarations
    /// and CDATA sections. `Err` holds the byte offset of the first of those, or of a tag, that
    /// opens and doesn't close.
    pub(crate) fn tags(self) -> Result<Vec<Tag>, usize> {
        let tags: Vec<Tag> = self
            .pieces()?
            .into_iter()
            .filter_map(|piece| match piece {
                Piece::Tag(tag) => Some(tag),
                Piece::Text(_) => None,
            })
            .collect();
        Ok(tags)
    }

    /// Whether the HTML has a letter in its text outside tags, comments and the like, entities
    /// such as `&nbsp;`, and the elements named `element`, such as `a`. HTML that doesn't close
    /// has none.
    pub(crate) fn has_words_outside(self, element: &str) -> bool {
        let Ok(pieces) = self.pieces() else {
            return false;
        };
        let mut depth = 0_usize;
        for piece in pieces {
            match piece {
                Piece::Tag(tag) if tag.name == element && !tag.self_closing => {
                    depth = if tag.closing {
                        depth.saturating_sub(1)
                    } else {
                        depth + 1
                    };
                }
                Piece::Tag(_) => {}
                Piece::Text(text) => {
                    if depth == 0 && decoded(text).chars().any(char::is_alphabetic) {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// The tags in the HTML and the text between them, in order, as [`Self::tags`] reads them.
    fn pieces(self) -> Result<Vec<Piece<'text>>, usize> {
        let html = self.text;
        let mut pieces = Vec::new();
        let mut position = 0;
        let mut text_start = 0;
        while let Some(found) = html[position..].find('<') {
            let start = position + found;
            let rest = &html[start..];
            let after = &rest[1..];
            let is_cdata = rest
                .get(..9)
                .is_some_and(|opener| opener.eq_ignore_ascii_case("<![cdata["));
            let starts_name = |text: &str| text.starts_with(|c: char| c.is_ascii_alphabetic());
            let is_declaration = after.strip_prefix('!').is_some_and(starts_name);
            let is_tag = starts_name(after) || after.strip_prefix('/').is_some_and(starts_name);
            let is_text = !(rest.starts_with("<!--")
                || rest.starts_with("<?")
                || is_cdata
                || is_declaration
                || is_tag);
            if !is_text && text_start < start {
                pieces.push(Piece::Text(&html[text_start..start]));
            }
            let length = if rest.starts_with("<!--") {
                comment_end(rest)
            } else if rest.starts_with("<?") {
                rest.find("?>").map(|end| end + 2)
            } else if is_cdata {
                rest.find("]]>").map(|end| end + 3)
            } else if is_declaration {
                rest.find('>').map(|end| end + 1)
            } else if is_tag {
                let tag = Tag::parse(rest, start).ok_or(start)?;
                let length = tag.length;
                pieces.push(Piece::Tag(tag));
                Some(length)
            } else {
                Some(1)
            };
            position = start + length.ok_or(start)?;
            if !is_text {
                text_start = position;
            }
        }
        if text_start < html.len() {
            pieces.push(Piece::Text(&html[text_start..]));
        }
        Ok(pieces)
    }

    /// The byte offsets of the HTML's heading tags, `<h1>` to `<h6>` or their closing tags, in
    /// any case, after a comment the HTML starts with, which ends at `-->` or `--!>`. Any byte
    /// but a letter or a digit after `<h1` ends the tag name.
    pub(crate) fn heading_offsets(self) -> Vec<usize> {
        let lower = self.text.to_ascii_lowercase();
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
        offsets.sort_unstable();
        offsets
    }

    /// Whether the HTML holds only comments, each closed.
    pub(crate) fn is_comments(self) -> bool {
        let mut rest = self.text.trim();
        while rest.starts_with("<!--") {
            let Some(end) = comment_end(rest) else {
                return false;
            };
            rest = rest[end..].trim_start();
        }
        rest.is_empty()
    }
}

impl Tag {
    /// The tag `html` starts with, at the byte offset `offset` of the HTML it is in, read with
    /// HTML's grammar for a tag: a name, then attributes, each a name with an optional `=` and a
    /// value, which is quoted with `"` or `'` or runs to the next space or `>`. The name is read
    /// as an attribute's name is, and isn't kept as an attribute. `None` if the tag
    /// doesn't close, or if a `<` comes before its `>` outside a quoted value, since GitHub then
    /// reads the tag as running on into what follows.
    fn parse(html: &str, offset: usize) -> Option<Self> {
        let bytes = html.as_bytes();
        let is_space = |byte: u8| matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | b'\x0c');
        let closing = bytes.get(1) == Some(&b'/');
        let name: String = html[1 + usize::from(closing)..]
            .chars()
            .take_while(|c| !c.is_ascii_whitespace() && *c != '/' && *c != '>')
            .collect();
        let mut index = 1 + usize::from(closing);
        let mut slash = false;
        let mut attributes = Vec::new();
        loop {
            let byte = *bytes.get(index)?;
            if byte == b'>' {
                return Some(Self {
                    name: name.to_ascii_lowercase(),
                    closing,
                    self_closing: slash,
                    offset,
                    length: index + 1,
                    attributes,
                });
            }
            if byte == b'<' {
                return None;
            }
            slash = byte == b'/';
            if is_space(byte) || slash {
                index += 1;
                continue;
            }
            let name_start = index;
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
            let is_tag_name = name_start == 1 + usize::from(closing);
            let attribute = (!is_tag_name).then(|| html[name_start..index].to_ascii_lowercase());
            let mut value = index;
            while bytes.get(value).is_some_and(|&next| is_space(next)) {
                value += 1;
            }
            if bytes.get(value) != Some(&b'=') {
                attributes.extend(attribute.map(|name| Attribute {
                    name,
                    value: String::new(),
                }));
                continue;
            }
            value += 1;
            while bytes.get(value).is_some_and(|&next| is_space(next)) {
                value += 1;
            }
            let quote = *bytes.get(value)?;
            if quote == b'"' || quote == b'\'' {
                let close = html[value + 1..].find(char::from(quote))?;
                let text = &html[value + 1..value + 1 + close];
                attributes.extend(attribute.map(|name| Attribute {
                    name,
                    value: decoded(text),
                }));
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
                let text = &html[value..index];
                attributes.extend(attribute.map(|name| Attribute {
                    name,
                    value: decoded(text),
                }));
            }
        }
    }

    /// The value of the tag's first attribute named `name`, in lowercase, as HTML reads a
    /// repeated attribute, or `None` if it has none. An attribute without a value has `""`.
    pub(crate) fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|attribute| attribute.name == name)
            .map(|attribute| attribute.value.as_str())
    }

    /// Whether the tag's element has content, so its tags must balance, as [`ELEMENTS`] lists.
    pub(crate) fn has_content(&self) -> bool {
        ELEMENT_NAMES.contains(self.name.as_str())
    }

    /// Whether HTML lets a document leave out the element's closing tag, as
    /// [`OPTIONAL_END_ELEMENTS`] lists.
    pub(crate) fn has_optional_end(name: &str) -> bool {
        OPTIONAL_END_NAMES.contains(name)
    }
}

/// `text` with its character references decoded: the named ones `&amp;`, `&lt;`, `&gt;`,
/// `&quot;`, `&apos;` and `&nbsp;`, and numeric ones such as `&#39;` or `&#x27;`. Any other `&`
/// stays as written.
fn decoded(text: &str) -> String {
    let mut decoded = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        decoded.push_str(&rest[..start]);
        rest = &rest[start..];
        let reference = rest.find(';').map(|end| (&rest[1..end], end + 1));
        let character = reference.and_then(|(name, _)| match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some('\u{a0}'),
            _ => {
                let number = name.strip_prefix('#')?;
                let code = match number.strip_prefix(['x', 'X']) {
                    Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                    None => number.parse().ok()?,
                };
                char::from_u32(code)
            }
        });
        if let (Some(character), Some((_, length))) = (character, reference) {
            decoded.push(character);
            rest = &rest[length..];
        } else {
            decoded.push('&');
            rest = &rest[1..];
        }
    }
    decoded.push_str(rest);
    decoded
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

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(html: &str) -> Vec<String> {
        let mut found = Vec::new();
        for tag in Html::new(html).tags().unwrap() {
            let slash = if tag.closing { "/" } else { "" };
            let closes = if tag.self_closing { "/" } else { "" };
            found.push(format!("<{slash}{}{closes}>", tag.name));
        }
        found
    }

    #[test]
    fn tags_are_read_with_the_tag_grammar() {
        assert_eq!(
            tags("<a href=https://example.com/>the site</a>"),
            ["<a>", "</a>"],
            "a `/` at the end of an unquoted value doesn't close the tag"
        );
        assert_eq!(
            tags("<details open=x/><br/><img src=\"a.png\" /><p title='a/>'>"),
            ["<details>", "<br/>", "<img/>", "<p>"],
            "only a `/` outside a value, right before `>`, closes the tag"
        );
        assert_eq!(
            tags("<!-- <div> --><?x <div> ?><![CDATA[ <div> ]]><!DOCTYPE html><DIV ID=a>"),
            ["<div>"],
            "comments, instructions, CDATA and declarations hold no tags, and names are lowercase"
        );
    }

    #[test]
    fn attributes_are_read_with_their_values() {
        let tags = Html::new(
            "<img SRC=\"a b.png\" width=480 alt='it\"s' hidden><a href=docs/x.md#y>x</a>",
        )
        .tags()
        .unwrap();
        let image = &tags[0];
        assert_eq!(
            image.attribute("src"),
            Some("a b.png"),
            "a quoted value keeps its spaces"
        );
        assert_eq!(
            image.attribute("width"),
            Some("480"),
            "an unquoted value runs to a space"
        );
        assert_eq!(
            image.attribute("alt"),
            Some("it\"s"),
            "the other quote is part of a value"
        );
        assert_eq!(
            image.attribute("hidden"),
            Some(""),
            "an attribute without a value has \"\""
        );
        assert_eq!(
            image.attribute("title"),
            None,
            "a missing attribute has no value"
        );
        assert_eq!(
            tags[1].attribute("href"),
            Some("docs/x.md#y"),
            "an unquoted value runs to the `>`"
        );
        let repeated = Html::new("<img alt=\"\" alt=\"b\">").tags().unwrap();
        assert_eq!(
            repeated[0].attribute("alt"),
            Some(""),
            "the first of a repeated attribute counts"
        );
    }

    #[test]
    fn attribute_values_and_text_decode_character_references() {
        let tags = Html::new("<img alt=\"&nbsp;\" src=\"a&amp;b.png\" title=&#x41;&#66;&unknown;>")
            .tags()
            .unwrap();
        assert_eq!(
            tags[0].attribute("alt"),
            Some("\u{a0}"),
            "`&nbsp;` is a space"
        );
        assert_eq!(tags[0].attribute("src"), Some("a&b.png"), "`&amp;` is `&`");
        assert_eq!(
            tags[0].attribute("title"),
            Some("AB&unknown;"),
            "numeric references decode, and an unknown one stays as written"
        );
    }

    #[test]
    fn words_outside_an_element_are_found() {
        for (html, words) in [
            ("<p align=\"center\">A tool for people.</p>", true),
            (
                "<p align=\"center\"><a href=x><img src=a.png alt=CI></a></p>",
                false,
            ),
            ("<p><a href=x>Docs</a> &nbsp; <!-- a note --></p>", false),
            ("<p><a href=x>Docs</a> and more</p>", true),
            ("<div>\n<!-- unclosed\n</div>", false),
        ] {
            assert_eq!(
                Html::new(html).has_words_outside("a"),
                words,
                "{html:?} has words outside links: {words}"
            );
        }
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
            assert!(Html::new(html).tags().is_err(), "{html:?} doesn't close");
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
            assert!(Html::new(html).tags().is_ok(), "{html:?} closes");
        }
    }

    #[test]
    fn heading_tags_are_found_after_a_leading_comment() {
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
                !Html::new(html).heading_offsets().is_empty(),
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
                Html::new(html).heading_offsets().is_empty(),
                "{html:?} has no heading tag"
            );
        }
    }

    #[test]
    fn comments_are_recognized() {
        for html in [
            "<!-- SPDX-License-Identifier: MIT -->",
            "<!-- a -->\n<!-- b -->\n",
            "<!-->",
        ] {
            assert!(Html::new(html).is_comments(), "{html:?} is only comments");
        }
        for html in ["<!-- a", "<!-- a --> x", "<div>"] {
            assert!(
                !Html::new(html).is_comments(),
                "{html:?} isn't only comments"
            );
        }
    }

    #[test]
    fn elements_with_content_and_optional_ends_are_listed() {
        let tag = |name: &str| Tag {
            name: name.to_owned(),
            closing: false,
            self_closing: false,
            offset: 0,
            length: 0,
            attributes: Vec::new(),
        };
        for name in ["details", "div", "select", "search", "li", "marquee"] {
            assert!(tag(name).has_content(), "<{name}> has content");
        }
        for name in ["br", "img", "source", "f:", "file", "header-dark"] {
            assert!(!tag(name).has_content(), "<{name}> is void or unknown");
        }
        assert!(Tag::has_optional_end("li"), "<li> may leave out its end");
        assert!(!Tag::has_optional_end("div"), "<div> may not");
    }
}
