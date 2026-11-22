//! Which line each line of a shell script's command starts on, as far as a shellcheck directive
//! above the command reaches.

use std::mem;

/// The index of the line each line of `text` belongs to the command of, by its index in
/// `text.lines()`: the line itself, unless it continues a command from a line above. A line
/// continues one after a line that ends in `\`, `|`, `||` or `&&` or inside a quoted string, as a
/// heredoc's body does the line that opens it, and a `case` statement's pattern line belongs to
/// the `case`, since shellcheck reads no directive between them.
///
/// It reads the script with a small lexer, not shellcheck's parser: a quote inside `$'...'` or a
/// heredoc nested in a command substitution can make it misjudge where a command starts.
pub(crate) fn of(text: &str) -> Vec<usize> {
    let lines: Vec<&str> = text.lines().collect();
    let mut lexer = Lexer {
        at_command: true,
        ..Lexer::default()
    };
    let mut starts = Vec::with_capacity(lines.len());
    let mut continued_from: Option<usize> = None;
    let mut index = 0;
    while let Some(line) = lines.get(index) {
        let start = continued_from
            .or_else(|| lexer.pattern_line_start(line))
            .unwrap_or(index);
        starts.push(start);
        lexer.command_start = start;
        let continues = lexer.read_line(line);
        continued_from = continues.then_some(start);
        for heredoc in lexer.take_heredocs() {
            while let Some(body) = lines.get(index + 1) {
                index += 1;
                starts.push(start);
                if heredoc.ends_at(body) {
                    break;
                }
            }
        }
        index += 1;
    }
    starts
}

/// What the lexer knows of the script so far, between the characters it reads.
#[derive(Default)]
struct Lexer {
    /// The quote character of the string the lexer is in, if it is in one.
    quote: Option<char>,
    /// How many parentheses deep the lexer is in an arithmetic expression, `$((...))` or
    /// `((...))`, where `<<` is a shift.
    arithmetic: usize,
    /// The unquoted word the lexer is reading.
    word: String,
    /// Whether the word has a quoted part, which keeps it from being a reserved word.
    word_quoted: bool,
    /// Whether the next word is the first of a command, where a reserved word counts.
    at_command: bool,
    /// Where the lexer is in a `case` statement's first line.
    case_head: CaseHead,
    /// The line each open `case` statement's command starts on, innermost last.
    cases: Vec<usize>,
    /// Whether the next word starts a `case` pattern.
    in_pattern: bool,
    /// The heredocs the current line opens, whose bodies follow it in order.
    heredocs: Vec<Heredoc>,
    /// The line the current line's command starts on.
    command_start: usize,
}

/// Where the lexer is in the words that open a `case` statement.
#[derive(Default, PartialEq, Eq)]
enum CaseHead {
    /// In no `case` statement's first words.
    #[default]
    None,
    /// After `case`, before the word it matches.
    Subject,
    /// After the word, before `in`.
    In,
}

impl Lexer {
    /// The line the `case` statement starts on, if `line` is one of its pattern lines.
    fn pattern_line_start(&self, line: &str) -> Option<usize> {
        let first = line.split_whitespace().next()?;
        let is_pattern =
            self.in_pattern && self.quote.is_none() && !first.starts_with('#') && first != "esac";
        is_pattern.then(|| self.cases.last().copied()).flatten()
    }

    /// The heredocs the line just read opens, which the lexer forgets.
    fn take_heredocs(&mut self) -> Vec<Heredoc> {
        if self.quote.is_some() {
            return Vec::new();
        }
        mem::take(&mut self.heredocs)
    }

    /// Reads `line`, and returns whether the next line continues its command.
    fn read_line(&mut self, line: &str) -> bool {
        let characters: Vec<char> = line.chars().collect();
        let mut trailing = String::new();
        let mut escaped_end = false;
        let mut at = 0;
        while let Some(&character) = characters.get(at) {
            let next = characters.get(at + 1).copied();
            at += 1;
            if let Some(quote) = self.quote {
                trailing.clear();
                if character == '\\' && quote != '\'' {
                    at += 1;
                } else if character == quote {
                    self.quote = None;
                }
                continue;
            }
            match character {
                '\\' => {
                    if next.is_none() {
                        escaped_end = true;
                    }
                    at += 1;
                    self.word.push(character);
                    trailing.clear();
                }
                '#' if self.word.is_empty() && self.arithmetic == 0 => break,
                '\'' | '"' | '`' => {
                    self.quote = Some(character);
                    self.word_quoted = true;
                    self.word.push(character);
                    trailing.clear();
                }
                '(' if self.arithmetic > 0 => self.arithmetic += 1,
                '(' if next == Some('(') => {
                    self.arithmetic = 2;
                    at += 1;
                    trailing.clear();
                }
                ')' if self.arithmetic > 0 => self.arithmetic -= 1,
                '<' if next == Some('<') && self.arithmetic == 0 => {
                    self.end_word();
                    at = self.read_heredoc(&characters, at + 1);
                    trailing.clear();
                }
                ';' if next == Some(';') || next == Some('&') => {
                    self.end_word();
                    at += 1;
                    if characters.get(at) == Some(&'&') {
                        at += 1;
                    }
                    self.in_pattern = !self.cases.is_empty();
                    self.at_command = true;
                    trailing.clear();
                }
                ')' if self.in_pattern => {
                    self.end_word();
                    self.in_pattern = false;
                    self.at_command = true;
                    trailing.clear();
                }
                '(' if self.in_pattern && self.word.is_empty() => {}
                ';' | '&' | '|' | '(' | ')' | '{' | '}' => {
                    self.end_word();
                    self.at_command = true;
                    trailing.push(character);
                }
                character if character.is_whitespace() => self.end_word(),
                character => {
                    self.word.push(character);
                    trailing.clear();
                }
            }
        }
        if self.quote.is_some() || escaped_end || self.arithmetic > 0 {
            return true;
        }
        self.end_word();
        self.at_command = true;
        trailing.ends_with('|') || trailing.ends_with("&&")
    }

    /// Reads the heredoc that opens at `at` in `characters`, after its `<<`, and returns where
    /// its delimiter ends. A `<<<` here-string opens none.
    fn read_heredoc(&mut self, characters: &[char], mut at: usize) -> usize {
        if characters.get(at) == Some(&'<') {
            return at + 1;
        }
        let strips_tabs = characters.get(at) == Some(&'-');
        if strips_tabs {
            at += 1;
        }
        while characters
            .get(at)
            .is_some_and(|character| matches!(character, ' ' | '\t'))
        {
            at += 1;
        }
        let mut delimiter = String::new();
        while let Some(&character) = characters.get(at) {
            if character.is_whitespace() || ";|&<>()".contains(character) {
                break;
            }
            if !matches!(character, '\'' | '"' | '\\') {
                delimiter.push(character);
            }
            at += 1;
        }
        if !delimiter.is_empty() {
            self.heredocs.push(Heredoc {
                delimiter,
                strips_tabs,
            });
        }
        at
    }

    /// Ends the word being read, and follows the reserved words that open and close a `case`
    /// statement and that a command follows.
    fn end_word(&mut self) {
        if self.word.is_empty() {
            return;
        }
        let word = mem::take(&mut self.word);
        let reserved = !mem::take(&mut self.word_quoted);
        let at_command = mem::replace(&mut self.at_command, false);
        if !reserved {
            if self.case_head == CaseHead::Subject {
                self.case_head = CaseHead::In;
            }
            return;
        }
        match (&self.case_head, word.as_str()) {
            (CaseHead::Subject, _) => self.case_head = CaseHead::In,
            (CaseHead::In, "in") => {
                self.case_head = CaseHead::None;
                self.cases.push(self.command_start);
                self.in_pattern = true;
            }
            _ if (at_command || self.in_pattern) && word == "esac" => {
                self.cases.pop();
                self.in_pattern = false;
            }
            _ if self.in_pattern => {}
            _ if at_command && word == "case" => self.case_head = CaseHead::Subject,
            _ if at_command => {
                self.at_command = matches!(
                    word.as_str(),
                    "then" | "do" | "else" | "elif" | "if" | "while" | "until" | "!" | "time"
                );
            }
            _ => {}
        }
    }
}

/// A heredoc a line of a script opens.
struct Heredoc {
    /// The word that ends it, unquoted.
    delimiter: String,
    /// Whether it opened with `<<-`, which strips the tabs that start its lines.
    strips_tabs: bool,
}

impl Heredoc {
    /// Whether `line` ends the heredoc.
    fn ends_at(&self, line: &str) -> bool {
        let line = if self.strips_tabs {
            line.trim_start_matches('\t')
        } else {
            line
        };
        line == self.delimiter
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_continued_line_belongs_to_the_command_it_continues() {
        let script = "set -eu\nrm \\\n  $a\ncurl $u |\n  tar -x\nfalse ||\n  rm $b\n\
                      true &&\n  rm $c\necho \"x\n$d\"\nrm $e\n";
        assert_eq!(of(script), [0, 1, 1, 3, 3, 5, 5, 7, 7, 9, 9, 11]);
    }

    #[test]
    fn a_heredoc_body_belongs_to_the_line_that_opens_it() {
        let script = "cat <<- EOF\n\t$port\n\tEOF\ncat <<'X' | sed s/a/b/\n$y\nX\nrm $z\n";
        assert_eq!(of(script), [0, 0, 0, 3, 3, 3, 6]);
    }

    #[test]
    fn a_shift_or_a_comment_opens_no_heredoc() {
        let script = "x=$((1<<4))\n# cat <<EOF\nf() { # <<EOF\n  rm $a\n}\nrm $b\n";
        assert_eq!(of(script), [0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn a_case_pattern_line_belongs_to_the_case() {
        let script = "case $1 in\n  $2) echo $3 ;;\n  $4)\n    echo $5\n    ;;\n  (*) : ;;\n\
                      esac\necho \"#\" 'it''s' $6\n";
        assert_eq!(of(script), [0, 0, 0, 3, 4, 0, 6, 7]);
    }

    #[test]
    fn a_block_or_a_loop_starts_new_commands_inside() {
        let script = "true && {\n  rm $1\n}\nls | while read -r f; do\n  rm $f\ndone\n";
        assert_eq!(of(script), [0, 1, 2, 3, 4, 5]);
    }
}
