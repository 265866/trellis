//! The root `.gitattributes` sets only how git converts line endings, so a checkout holds the
//! bytes a reviewer read in the diff.

use std::fs;
use std::path::Path;

use crate::policy_error::PolicyError;
use crate::repository_files;
use crate::violation::Violation;

/// The root `.gitattributes`, relative to the root. It is the only one: a `.gitattributes` in a
/// directory below is a tool's configuration file, which `config_files` refuses.
const ATTRIBUTES: &str = ".gitattributes";

/// The attributes the root `.gitattributes` may set, as a line writes them. A checkout writes the
/// bytes of the blob under each, apart from line endings: `text` and `text=auto` let git store a
/// file's line endings as LF, `eol=lf` keeps them LF when git writes the file, and `-text` turns
/// the conversion off.
const ALLOWED: [&str; 4] = ["text", "-text", "text=auto", "eol=lf"];

/// The characters git splits an attributes line at: space, tab, carriage return and line feed.
const BLANKS: [u8; 4] = *b" \t\r\n";

/// How git marks a line that defines an attribute macro instead of matching paths.
const MACRO_PREFIX: &[u8] = b"[attr]";

/// The UTF-8 byte order mark, which git skips at the start of an attributes file.
const BYTE_ORDER_MARK: &[u8] = b"\xef\xbb\xbf";

/// Checks that the root `.gitattributes` sets only the attributes in [`ALLOWED`] and defines no
/// attribute macro. Any other attribute can make a checkout write other bytes than the blob a
/// reviewer read, as `working-tree-encoding`, `ident` and `filter` do, or keep git from showing
/// the diff, as `-diff` and `binary` do. A line git reads differently than it looks, such as one
/// with `#` after its pattern, fails too, since its words are attributes to git.
///
/// It reads the file in the working tree, which `files`, the repository's files, lists when it is
/// there, and in a git checkout also each copy in git's index, which a checkout reads to decide
/// how to write files. Either must be a regular file: git reads no attributes from a symbolic link,
/// though versions before 2.32 followed one in the working tree.
///
/// Git also reads `.git/info/attributes` and the attributes files in its own configuration, which
/// a repository can't set and a clone doesn't copy, so they aren't checked.
///
/// # Errors
///
/// Fails if a copy of the file can't be read, or git fails.
pub(crate) fn check(root: &Path, files: &[String]) -> Result<Vec<Violation>, PolicyError> {
    let mut messages = Vec::new();
    let mut reported = Vec::new();
    if files.iter().any(|file| file == ATTRIBUTES) {
        let path = root.join(ATTRIBUTES);
        let read_error = |source| PolicyError::Read {
            path: path.clone(),
            source,
        };
        if fs::symlink_metadata(&path)
            .map_err(read_error)?
            .is_symlink()
        {
            messages.push(format!("This is a symbolic link. {LINK_REMEDY}"));
        } else {
            reported = problems(&fs::read(&path).map_err(read_error)?);
            messages.extend(reported.iter().map(|problem| problem.render("Line")));
        }
    }
    for staged in repository_files::staged_contents(root, ATTRIBUTES)? {
        let Some(text) = staged else {
            messages.push(format!(
                "Git's index holds it as a symbolic link or a submodule. {LINK_REMEDY}"
            ));
            continue;
        };
        for problem in problems(&text) {
            if !reported.contains(&problem) {
                messages.push(problem.render(
                    "In the copy in git's index, which a checkout reads and `git add` \
                         replaces, line",
                ));
                reported.push(problem);
            }
        }
    }
    messages.dedup();
    Ok(messages
        .into_iter()
        .map(|message| Violation::new(ATTRIBUTES.to_owned(), message))
        .collect())
}

/// What to do about a root `.gitattributes` that isn't a regular file.
const LINK_REMEDY: &str = "Git reads no attributes from a symbolic link, though versions before \
                           2.32 followed one in the working tree, so which attributes apply \
                           depends on the version. Replace it with the file itself.";

/// A line of the root `.gitattributes` that sets something outside [`ALLOWED`].
#[derive(Debug, PartialEq, Eq)]
struct Problem {
    /// The line's 1-based number.
    line: usize,
    /// What the line does, after `Line <n>`.
    what: String,
}

impl Problem {
    /// The problem as a message that starts with `at` and the line's number.
    fn render(&self, at: &str) -> String {
        format!("{at} {} {}", self.line, self.what)
    }
}

/// The lines of `text`, an attributes file, that set an attribute outside [`ALLOWED`], define an
/// attribute macro, or hold a NUL byte. Each line is read as git 2.47 reads it: past a byte order
/// mark at the start, split at line feeds, with blanks before the pattern skipped, a line that
/// then starts with `#` a comment, and a pattern that starts with `"` read as a C string when it
/// is one; every word after the pattern is an attribute.
fn problems(text: &[u8]) -> Vec<Problem> {
    let text = text.strip_prefix(BYTE_ORDER_MARK).unwrap_or(text);
    let mut problems = Vec::new();
    for (index, line) in text.split(|&byte| byte == b'\n').enumerate() {
        let number = index + 1;
        if line.contains(&0) {
            problems.push(Problem {
                line: number,
                what: "holds a NUL byte, after which git reads no more of the line, or of the \
                       file. Delete it."
                    .to_owned(),
            });
            continue;
        }
        let line = trim_blanks(line);
        if line.is_empty() || line.starts_with(b"#") {
            continue;
        }
        let Split {
            pattern,
            attributes,
        } = split_pattern(line);
        if pattern.starts_with(MACRO_PREFIX) {
            let name = shown(&pattern[MACRO_PREFIX.len()..]);
            problems.push(Problem {
                line: number,
                what: format!(
                    "defines the attribute macro `{name}`, which can set any attribute under a \
                     name allowed here, as `[attr]text ident` does. Delete it, and set {} \
                     directly.",
                    allowed_list("or")
                ),
            });
            continue;
        }
        let words = attributes
            .split(|byte| BLANKS.contains(byte))
            .filter(|word| !word.is_empty());
        for attribute in words {
            if ALLOWED
                .iter()
                .any(|allowed| allowed.as_bytes() == attribute)
            {
                continue;
            }
            let comment = if attribute.starts_with(b"#") {
                " Git doesn't read `#` after a pattern as a comment, so it ignores the whole \
                 line; put a comment on a line of its own."
            } else {
                ""
            };
            problems.push(Problem {
                line: number,
                what: format!(
                    "sets `{}`, but only {} may be set here. Another attribute can make a \
                     checkout write other bytes than the diff a reviewer read, as \
                     `working-tree-encoding`, `ident` and `filter` do, or hide the diff, as \
                     `-diff` and `binary` do. Delete it.{comment}",
                    shown(attribute),
                    allowed_list("and")
                ),
            });
        }
    }
    problems
}

/// [`ALLOWED`] as a list in prose, its last two joined by `conjunction`.
fn allowed_list(conjunction: &str) -> String {
    let quoted: Vec<String> = ALLOWED.iter().map(|word| format!("`{word}`")).collect();
    match quoted.split_last() {
        Some((last, rest)) if !rest.is_empty() => {
            format!("{} {conjunction} {last}", rest.join(", "))
        }
        _ => quoted.concat(),
    }
}

/// `bytes`, part of an attributes line, as text a message can quote, with control characters
/// escaped.
fn shown(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .chars()
        .map(|character| {
            if character.is_control() {
                character.escape_unicode().to_string()
            } else {
                character.to_string()
            }
        })
        .collect()
}

/// `line` without the blanks at its start.
fn trim_blanks(line: &[u8]) -> &[u8] {
    let start = line
        .iter()
        .position(|byte| !BLANKS.contains(byte))
        .unwrap_or(line.len());
    &line[start..]
}

/// An attributes line split into its pattern and the rest.
#[derive(Debug, PartialEq, Eq)]
struct Split<'line> {
    /// The pattern, unquoted when it was quoted.
    pattern: Vec<u8>,
    /// The rest of the line, which holds the attributes.
    attributes: &'line [u8],
}

/// `line`, an attributes line that starts with its pattern, split into the pattern and the rest.
/// A pattern that starts with `"` and is a C string, as git quotes one, ends at its closing quote,
/// with or without a blank after it; any other ends at the first blank.
fn split_pattern(line: &[u8]) -> Split<'_> {
    if let Some(unquoted) = unquote(line) {
        return unquoted;
    }
    let end = line
        .iter()
        .position(|byte| BLANKS.contains(byte))
        .unwrap_or(line.len());
    Split {
        pattern: line[..end].to_vec(),
        attributes: &line[end..],
    }
}

/// The C string that starts `line`, unquoted, and the rest of the line after its closing quote,
/// or `None` if `line` doesn't start with one. Like git, it takes the escapes `\a`, `\b`, `\f`,
/// `\n`, `\r`, `\t`, `\v`, `\\`, `\"` and three octal digits whose first is at most 3, and no
/// other.
fn unquote(line: &[u8]) -> Option<Split<'_>> {
    let mut bytes = line.strip_prefix(b"\"")?.iter().enumerate();
    let mut unquoted = Vec::new();
    while let Some((at, &byte)) = bytes.next() {
        match byte {
            b'"' => {
                return Some(Split {
                    pattern: unquoted,
                    attributes: &line[at + 2..],
                });
            }
            b'\\' => {
                let (_, &escaped) = bytes.next()?;
                let byte = match escaped {
                    b'a' => 0x07,
                    b'b' => 0x08,
                    b'f' => 0x0c,
                    b'n' => b'\n',
                    b'r' => b'\r',
                    b't' => b'\t',
                    b'v' => 0x0b,
                    b'\\' | b'"' => escaped,
                    b'0'..=b'3' => {
                        let mut value = escaped - b'0';
                        for _ in 0..2 {
                            let (_, &digit) = bytes.next()?;
                            if !(b'0'..=b'7').contains(&digit) {
                                return None;
                            }
                            value = (value << 3) | (digit - b'0');
                        }
                        value
                    }
                    _ => return None,
                };
                unquoted.push(byte);
            }
            _ => unquoted.push(byte),
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The attributes each line of `text` sets that aren't allowed, and the lines that define a
    /// macro or hold a NUL, each as `<line>: <word>`, with `[attr]` or `NUL` as the word.
    fn refused(text: &[u8]) -> Vec<String> {
        problems(text)
            .into_iter()
            .map(|problem| {
                let word = if problem.what.starts_with("defines") {
                    "[attr]".to_owned()
                } else if problem.what.starts_with("holds a NUL") {
                    "NUL".to_owned()
                } else {
                    problem
                        .what
                        .split('`')
                        .nth(1)
                        .unwrap_or_default()
                        .to_owned()
                };
                format!("{}: {word}", problem.line)
            })
            .collect()
    }

    #[test]
    fn line_ending_attributes_comments_and_bare_patterns_pass() {
        for text in [
            &b"# Comment ident\n* text=auto eol=lf\n"[..],
            b"\xef\xbb\xbf# Comment ident\r\n*.png -text\r\n",
            b"  \t# indented comment ident\n\t*.rs\ttext  eol=lf \n\n",
            b"\"docs/my notes.md\" text\n",
            b"\"docs/a\\\"b c.md\"eol=lf\n",
            b"*.txt\n",
            b"",
        ] {
            assert_eq!(
                refused(text),
                Vec::<String>::new(),
                "{:?} passes",
                shown(text)
            );
        }
    }

    #[test]
    fn words_git_reads_as_attributes_fail_as_git_splits_the_line() {
        for (text, expected) in [
            (&b"*.rs ident\n"[..], vec!["1: ident"]),
            (b"*.rs IDENT\n", vec!["1: IDENT"]),
            (
                b"* text=AUTO eol=crlf\n",
                vec!["1: text=AUTO", "1: eol=crlf"],
            ),
            (
                b"* crlf !eol !text -eol\n",
                vec!["1: crlf", "1: !eol", "1: !text", "1: -eol"],
            ),
            (b"\"build.rs\"ident\n", vec!["1: ident"]),
            (b"\"a b.rs\" text ident\n", vec!["1: ident"]),
            (b"\"a\\x b.rs\" text\n", vec!["1: b.rs\""]),
            (b"\"open text\n", Vec::<&str>::new()),
            (b"a\\ b.rs ident\n", vec!["1: b.rs", "1: ident"]),
            (b"*.rs text # LF only\n", vec!["1: #", "1: LF", "1: only"]),
            (b"* text=auto\r\n*.rs ident\r\n", vec!["2: ident"]),
            (b"\xef\xbb\xbf*.rs ident\n", vec!["1: ident"]),
            (b"* text\x0bident\n", vec!["1: text\\u{b}ident"]),
            (b"* text\0 ident\n* eol=lf\n", vec!["1: NUL"]),
            (b"[attr]lf text\n", vec!["1: [attr]"]),
            (b"\xef\xbb\xbf[attr]m ident\n", vec!["1: [attr]"]),
            (b"\"[attr]m\" ident\n", vec!["1: [attr]"]),
            (b"\"\\133attr]m\" ident\n", vec!["1: [attr]"]),
        ] {
            assert_eq!(refused(text), expected, "{:?}", shown(text));
        }
    }

    #[test]
    fn quoted_patterns_unquote_as_git_does() {
        assert_eq!(
            unquote(b"\"a\\tb\\101\\\\\\\"\" rest"),
            Some(Split {
                pattern: b"a\tbA\\\"".to_vec(),
                attributes: b" rest",
            }),
            "escapes"
        );
        for text in [
            &b"\"a\\x\""[..],
            b"\"a\\4\"",
            b"\"a\\18\"",
            b"\"open",
            b"plain",
        ] {
            assert_eq!(unquote(text), None, "{:?} isn't a C string", shown(text));
        }
    }

    #[test]
    fn allowed_attributes_read_as_a_list() {
        assert_eq!(
            allowed_list("or"),
            "`text`, `-text`, `text=auto` or `eol=lf`"
        );
    }
}
