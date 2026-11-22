//! The shell scripts of a deploy kit, the files under `deploy/`, pass shellcheck, since they are
//! code that no other check reads.

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs::{self, File};
use std::io::{self, ErrorKind};
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::Value;

use crate::policy_error::PolicyError;
use crate::repository_files;
use crate::violation::Violation;
use crate::workspace::Workspace;

/// The directory that holds a project's deploy kit, relative to the root. Its files are code to
/// `trellis-policy rule-changes`, so a kit change lands with the code change it needs.
pub(crate) const DEPLOY_DIRECTORY: &str = "deploy/";

/// The extensions that make a file under [`DEPLOY_DIRECTORY`] a shell script, in any case.
const SCRIPT_EXTENSIONS: [&str; 5] = ["sh", "bash", "dash", "ksh", "bats"];

/// The shells shellcheck reads, which a script's `#!` line can name.
const SHELLS: [&str; 6] = ["sh", "bash", "dash", "ksh", "ash", "bats"];

/// The directive keys a script may use, which tell shellcheck what it can't find out alone.
/// `disable` is checked apart, and any other key, such as `extended-analysis` or
/// `external-sources`, would turn checks off for the whole script.
const ALLOWED_DIRECTIVES: [&str; 4] = ["enable", "shell", "source", "source-path"];

/// The longest `#!` line read, in bytes. A longer line is cut there, which keeps a large binary
/// file from being read whole.
const SHEBANG_LIMIT: u64 = 256;

/// What to do when shellcheck isn't installed.
const INSTALL_REMEDY: &str = "Install the shellcheck version the Set up section of CONTRIBUTING.md \
                              names; CI installs it too.";

/// Checks every shell script under `deploy/` with shellcheck, and reports each finding at its
/// file. A script is a file whose name ends in `.sh`, `.bash`, `.dash`, `.ksh` or `.bats`, or
/// whose first line is a `#!` line that names `sh`, `bash`, `dash`, `ksh`, `ash` or `bats`.
/// shellcheck runs on a copy of `deploy/` alone, with `--norc` and without `SHELLCHECK_OPTS`, so
/// neither a `.shellcheckrc` nor the environment can turn a check off and a file outside the kit
/// can't be sourced unseen, and with `--check-sourced`, so it reports the files a script sources
/// too, looked up from the script's directory.
///
/// A file under `deploy/` silences one finding with a `# shellcheck disable=SC<code> # <reason>`
/// line right above it, where review sees it. shellcheck applies a directive to the whole command
/// below it, a function or a sourced file included, so the check measures what each directive
/// hides: it runs shellcheck a second time with every `disable` taken out, and each finding that
/// only that run reports must have a directive for its code right above its line, with only
/// comments and blank lines between. [`directive_problems`] says which other directives are
/// violations. A symbolic link under `deploy/` is one too, since the kit's files are what review
/// read.
///
/// A project without `deploy/` has no script, so the check runs nothing.
///
/// # Errors
///
/// Fails if the repository's files or a file under `deploy/` can't be read or copied, or
/// shellcheck doesn't run, such as when it isn't installed.
pub(crate) fn check(workspace: &Workspace) -> Result<Vec<Violation>, PolicyError> {
    let root = workspace.root();
    let mut violations = Vec::new();
    let mut texts = BTreeMap::new();
    let mut scripts = Vec::new();
    for file in repository_files::list(root)? {
        if !file.starts_with(DEPLOY_DIRECTORY) {
            continue;
        }
        let path = root.join(&file);
        let read_error = |source| PolicyError::Read {
            path: path.clone(),
            source,
        };
        if fs::symlink_metadata(&path)
            .map_err(read_error)?
            .is_symlink()
        {
            violations.push(Violation::new(
                file,
                "This is a symbolic link. A deploy kit's files are what review reads and \
                 shellcheck checks, so put the file itself here."
                    .to_owned(),
            ));
            continue;
        }
        let text = String::from_utf8_lossy(&fs::read(&path).map_err(read_error)?).into_owned();
        violations.extend(
            directive_problems(&file, &text)
                .into_iter()
                .map(|problem| Violation::new(file.clone(), problem)),
        );
        if is_script(&path).map_err(read_error)? {
            scripts.push(file.clone());
        }
        texts.insert(file, text);
    }
    if scripts.is_empty() {
        return Ok(violations);
    }
    let reported = shellcheck_copy(&texts, &scripts, str::to_owned)?;
    let disables = texts.values().any(|text| text.contains("disable="));
    let unsilenced = if disables {
        shellcheck_copy(&texts, &scripts, without_disables)?
    } else {
        BTreeSet::new()
    };
    for hidden in unsilenced.difference(&reported) {
        let silenced_above = texts
            .get(&hidden.file)
            .is_some_and(|text| silenced_right_above(text, hidden.line, hidden.code));
        if !silenced_above {
            violations.push(Violation::new(
                hidden.file.clone(),
                format!(
                    "A shellcheck directive hides SC{} at line {}, which isn't the line right \
                     below it, so it hides more than it says: a whole function, block or sourced \
                     file, or the rest of the file. Put `# shellcheck disable=SC{} # <reason>` on \
                     the line above line {}, and take the code out of the directive that hid it.",
                    hidden.code, hidden.line, hidden.code, hidden.line
                ),
            ));
        }
    }
    violations.extend(reported.iter().map(Finding::violation));
    Ok(violations)
}

/// What is wrong with the shellcheck directives in `text`, the text of `file`. shellcheck reads a
/// directive in a comment after any run of spaces and tabs, at the start of a line or after code
/// such as `f() {` or `then`. So each of these is a problem:
///
/// - a directive after code on its line, where review misses it;
/// - a `disable` whose value isn't a list of codes, such as `all`, a range or a quoted value;
/// - a `disable` without a reason after it, as a `#` comment;
/// - `shell` in a file whose `#!` line names its shell already, which it would override;
/// - `source` that names a file outside `deploy/` or an absolute path, such as `/dev/null`, which
///   would keep shellcheck from reading what the script sources;
/// - any key but `disable`, `enable`, `shell`, `source` and `source-path`, which turns checks off
///   for the whole file.
///
/// A line in a heredoc that looks like a directive is read as one too, which shellcheck doesn't.
fn directive_problems(file: &str, text: &str) -> Vec<String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let has_shebang = text.starts_with("#!");
    let directory = file.rsplit_once('/').map_or("", |(directory, _)| directory);
    let mut problems = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let number = index + 1;
        let Some(Directive {
            code,
            text: directive,
        }) = Directive::of(line)
        else {
            continue;
        };
        if Directive::has_unusual_spaces(line) {
            problems.push(format!(
                "Line {number} spells a shellcheck directive with a space that isn't an ASCII \
                 space or tab. Write it with plain spaces."
            ));
        }
        if !code.trim().is_empty() {
            problems.push(format!(
                "Line {number} has a shellcheck directive after code, where it covers the command \
                 that code opens. Put the directive on a line of its own, above the command it's \
                 for."
            ));
            continue;
        }
        let (settings, reason) = directive.split_once('#').unwrap_or((directive, ""));
        for setting in settings.split_whitespace() {
            let (key, value) = setting.split_once('=').unwrap_or((setting, ""));
            match key {
                "disable" if !is_code_list(value) => problems.push(format!(
                    "Line {number} disables `{value}`, which isn't a list of codes such as \
                     `SC2086,SC2046`. Silence each finding by its code, on the line above it."
                )),
                "disable" if disables_a_parse_check(value) => problems.push(format!(
                    "Line {number} disables `{value}`, a check of how shellcheck parses the \
                     script, which can stop it reading the rest of the file. Fix the syntax; \
                     only SC1090 and SC1091, about files it can't follow, may be silenced."
                )),
                "disable" if reason.trim().is_empty() => problems.push(format!(
                    "Line {number} silences shellcheck without a reason. Write it after the \
                     codes, as `# shellcheck disable=SC2086 # <why the finding is wrong>`."
                )),
                "shell" if has_shebang => problems.push(format!(
                    "Line {number} sets the shell, which the file's `#!` line names already. \
                     Leave it to the `#!` line."
                )),
                "source-path" if value != "SCRIPTDIR" && !inside_deploy(directory, value) => {
                    problems.push(format!(
                        "Line {number} points shellcheck at `{value}` to look for sourced files, \
                         which isn't `SCRIPTDIR` or a directory in `deploy/`. Look them up in the \
                         kit."
                    ));
                }
                "source" if !inside_deploy(directory, value) => problems.push(format!(
                    "Line {number} points shellcheck at `{value}`, which isn't a file in \
                     `deploy/`, so it can't read what the script sources. Point it at the file \
                     in the kit, or silence SC1091 with a reason."
                )),
                "disable" | "shell" | "source" | "source-path" => {}
                key if ALLOWED_DIRECTIVES.contains(&key) => {}
                key => problems.push(format!(
                    "Line {number} sets `{key}`, which isn't a directive this check allows: only \
                     `disable` with a reason, `enable`, `shell`, `source` and `source-path`."
                )),
            }
        }
    }
    problems
}

/// A comment that shellcheck reads as a directive, in a line of a script.
struct Directive<'line> {
    /// The code before the comment.
    code: &'line str,
    /// The directive after `shellcheck`, its settings and any `#` comment after them.
    text: &'line str,
}

impl<'line> Directive<'line> {
    /// The directive in `line`, if it holds a comment that shellcheck reads as one: `#`, any spaces
    /// and tabs, `shellcheck`, then at least one space or tab.
    fn of(line: &'line str) -> Option<Self> {
        line.match_indices('#').find_map(|(at, _)| {
            let after = line[at + 1..].trim_start_matches(char::is_whitespace);
            let rest = after.strip_prefix("shellcheck")?;
            let text = rest.trim_start_matches(char::is_whitespace);
            (text.len() < rest.len()).then_some(Self {
                code: &line[..at],
                text,
            })
        })
    }

    /// Whether the directive's line spells it with a space other than an ASCII space or tab, such
    /// as a non-breaking space, which shellcheck reads but a reader may not see.
    fn has_unusual_spaces(line: &str) -> bool {
        line.chars()
            .any(|character| character.is_whitespace() && !matches!(character, ' ' | '\t'))
    }

    /// The codes this directive's `disable` settings name, as numbers.
    fn disabled_codes(&self) -> BTreeSet<u32> {
        let settings = self
            .text
            .split_once('#')
            .map_or(self.text, |(settings, _)| settings);
        settings
            .split_whitespace()
            .filter_map(|setting| setting.strip_prefix("disable="))
            .flat_map(|value| value.split(','))
            .filter_map(|code| code.strip_prefix("SC").unwrap_or(code).parse().ok())
            .collect()
    }
}

/// Whether `value` is a list of shellcheck codes, such as `SC2086,2046`.
fn is_code_list(value: &str) -> bool {
    !value.is_empty()
        && value.split(',').all(|code| {
            let digits = code.strip_prefix("SC").unwrap_or(code);
            !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
        })
}

/// Whether `value`, a list of codes, names one of shellcheck's parse checks, SC1000 to SC1999,
/// other than SC1090 and SC1091, which are about the files a script sources.
fn disables_a_parse_check(value: &str) -> bool {
    value.split(',').any(|code| {
        let digits = code.strip_prefix("SC").unwrap_or(code);
        digits.len() == 4 && digits.starts_with('1') && !matches!(digits, "1090" | "1091")
    })
}

/// Whether `source`, a `source=` value in a file in `directory`, names a file under `deploy/`: a
/// relative path that stays inside the kit, read from the file's directory.
fn inside_deploy(directory: &str, source: &str) -> bool {
    if source.starts_with('/') || source.contains('\\') {
        return false;
    }
    normalized(&format!("{directory}/{source}"))
        .is_some_and(|path| path.starts_with(DEPLOY_DIRECTORY))
}

/// `path` with its `.` parts and each `..` with the part before it taken out, or `None` if a `..`
/// climbs above its start.
fn normalized(path: &str) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part => parts.push(part),
        }
    }
    Some(parts.join("/"))
}

/// `text` with the codes of every shellcheck directive's `disable` settings replaced with SC0000,
/// which shellcheck takes and which names no check, and every other byte kept, so the lines keep
/// their numbers and nothing else in them changes.
fn without_disables(text: &str) -> String {
    let mut edited = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let Some(directive) = Directive::of(line) else {
            edited.push_str(line);
            continue;
        };
        let start = line.len() - directive.text.len();
        let settings_end = directive
            .text
            .find('#')
            .map_or(line.len(), |reason| start + reason);
        edited.push_str(&line[..start]);
        for setting in line[start..settings_end].split_inclusive(char::is_whitespace) {
            if let Some(value) = setting.strip_prefix("disable=") {
                let codes = value.trim_end_matches(char::is_whitespace);
                edited.push_str("disable=SC0000");
                edited.push_str(&value[codes.len()..]);
            } else {
                edited.push_str(setting);
            }
        }
        edited.push_str(&line[settings_end..]);
    }
    edited
}

/// Whether `text` has a directive that disables `code` right above its line `line`: on the lines
/// above the command that line belongs to, before any line that is neither blank nor a comment. A
/// line belongs to the command a `\\` at the end of the line above continues, and a heredoc's body
/// to the line that opens the heredoc, since no comment can go between them.
fn silenced_right_above(text: &str, line: usize, code: u32) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    let mut first = command_start(&lines, line.saturating_sub(1));
    while first > 0 && lines[first - 1].ends_with('\\') {
        first = command_start(&lines, first - 1);
    }
    lines[..first]
        .iter()
        .rev()
        .map(|line| line.trim_start_matches([' ', '\t']))
        .take_while(|line| line.is_empty() || line.starts_with('#'))
        .filter_map(Directive::of)
        .any(|directive| directive.disabled_codes().contains(&code))
}

/// The index in `lines` of the line that opens the heredoc whose body holds the line at `index`,
/// or `index` if that line is in no heredoc's body.
fn command_start(lines: &[&str], index: usize) -> usize {
    let mut open: Option<(usize, Heredoc)> = None;
    for (at, line) in lines.iter().enumerate().take(index + 1) {
        match &open {
            Some((start, heredoc)) => {
                let start = *start;
                if heredoc.ends_at(line) {
                    open = None;
                } else if at == index {
                    return start;
                }
            }
            None => open = Heredoc::opened_by(line).map(|heredoc| (at, heredoc)),
        }
    }
    index
}

/// A heredoc a line of a script opens.
struct Heredoc {
    /// The word that ends it, unquoted.
    delimiter: String,
    /// Whether it opened with `<<-`, which strips the tabs that start its lines.
    strips_tabs: bool,
}

impl Heredoc {
    /// The heredoc `line` opens, or `None` if it opens none.
    fn opened_by(line: &str) -> Option<Self> {
        let (_, rest) = line.split_once("<<")?;
        if rest.starts_with('<') {
            return None;
        }
        let (rest, strips_tabs) = rest
            .strip_prefix('-')
            .map_or((rest, false), |rest| (rest, true));
        let delimiter: String = rest
            .trim_start()
            .chars()
            .take_while(|character| !character.is_whitespace() && !";|&<>)".contains(*character))
            .filter(|character| !matches!(character, '\'' | '"' | '\\'))
            .collect();
        (!delimiter.is_empty()).then_some(Self {
            delimiter,
            strips_tabs,
        })
    }

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

/// Whether the file at `path` is a shell script: by its extension, by the shell its first line
/// names, or by having neither an extension nor a `#!` line for another program, as a library a
/// script sources often has, so a sourced file is checked on its own too. A hidden file, such as
/// `.shellcheckrc`, isn't one by its lack of an extension.
fn is_script(path: &Path) -> io::Result<bool> {
    let extension = path.extension();
    let has_script_extension = extension.is_some_and(|extension| {
        SCRIPT_EXTENSIONS
            .iter()
            .any(|script| extension.eq_ignore_ascii_case(script))
    });
    if has_script_extension {
        return Ok(true);
    }
    let start = file_start(path)?;
    let start = start.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&start);
    let hidden = path
        .file_name()
        .is_some_and(|name| name.as_encoded_bytes().starts_with(b"."));
    Ok(names_a_shell(start) || (extension.is_none() && !hidden && !start.starts_with(b"#!")))
}

/// The first [`SHEBANG_LIMIT`] bytes of the file at `path`, or all of a shorter one.
fn file_start(path: &Path) -> io::Result<Vec<u8>> {
    use std::io::Read as _;

    let mut start = Vec::new();
    File::open(path)?
        .take(SHEBANG_LIMIT)
        .read_to_end(&mut start)?;
    Ok(start)
}

/// Whether `start`, the start of a file, is a `#!` line that names one of the [`SHELLS`], as
/// `#!/bin/sh` and `#!/usr/bin/env bash` do, after a UTF-8 byte order mark if it has one.
fn names_a_shell(start: &[u8]) -> bool {
    let start = start.strip_prefix(b"\xef\xbb\xbf").unwrap_or(start);
    let Some(rest) = start.strip_prefix(b"#!") else {
        return false;
    };
    let line = rest.split(|byte| *byte == b'\n').next().unwrap_or_default();
    line.split(|byte| byte.is_ascii_whitespace() || *byte == b'/')
        .any(|word| SHELLS.iter().any(|shell| word == shell.as_bytes()))
}

/// One finding shellcheck reports.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Finding {
    /// The file, relative to the root, `deploy/` included.
    file: String,
    /// The line.
    line: usize,
    /// The column.
    column: usize,
    /// The code, such as 2086 for SC2086.
    code: u32,
    /// The level, such as `warning`.
    level: String,
    /// What shellcheck says.
    message: String,
}

impl Finding {
    /// The finding as a violation of its file.
    fn violation(&self) -> Violation {
        Violation::new(
            self.file.clone(),
            format!(
                "shellcheck reports, at line:column {}:{}: {}: {} [SC{}]. Fix the script, or \
                 silence the finding with `# shellcheck disable=SC{} # <reason>` on the line \
                 above it.",
                self.line, self.column, self.level, self.message, self.code, self.code
            ),
        )
    }
}

/// The findings shellcheck reports for `scripts` in a copy of `deploy/` alone, made in a
/// temporary directory from `texts`, each file's text, as `edit` changes it.
fn shellcheck_copy(
    texts: &BTreeMap<String, String>,
    scripts: &[String],
    edit: impl Fn(&str) -> String,
) -> Result<BTreeSet<Finding>, PolicyError> {
    let copy = tempfile::tempdir().map_err(|source| PolicyError::Read {
        path: env::temp_dir(),
        source,
    })?;
    for (file, text) in texts {
        let path = copy.path().join(file);
        let write_error = |source| PolicyError::Read {
            path: path.clone(),
            source,
        };
        if let Some(directory) = path.parent() {
            fs::create_dir_all(directory).map_err(write_error)?;
        }
        fs::write(&path, edit(text)).map_err(write_error)?;
    }
    shellcheck(copy.path(), scripts)
}

/// Runs shellcheck on `scripts`, relative to `root`, and reads its findings.
fn shellcheck(root: &Path, scripts: &[String]) -> Result<BTreeSet<Finding>, PolicyError> {
    let arguments = [
        "--norc",
        "--check-sourced",
        "--external-sources",
        "--source-path=SCRIPTDIR",
        "--format=json1",
        "--",
    ];
    let command = format!("shellcheck {} {}", arguments.join(" "), scripts.join(" "));
    let failed = |message: String| PolicyError::Tool {
        command: command.clone(),
        message,
    };
    let output = Command::new("shellcheck")
        .args(arguments)
        .args(scripts)
        .current_dir(root)
        .env_remove("SHELLCHECK_OPTS")
        .stdin(Stdio::null())
        .output()
        .map_err(|error: io::Error| {
            if error.kind() == ErrorKind::NotFound {
                failed(format!("shellcheck isn't installed. {INSTALL_REMEDY}"))
            } else {
                failed(error.to_string())
            }
        })?;
    let report = String::from_utf8_lossy(&output.stdout);
    let found = findings(&report);
    match (output.status.code(), found) {
        (Some(0), Some(found)) if found.is_empty() => Ok(found),
        (Some(1), Some(found)) if !found.is_empty() => Ok(found),
        _ => Err(failed(format!(
            "it exited with {}: {}\n{}",
            output.status,
            report.trim(),
            String::from_utf8_lossy(&output.stderr).trim()
        ))),
    }
}

/// The findings in `report`, shellcheck's output in its `json1` format, each at its file with the
/// path's `.` and `..` parts resolved, or `None` if the report isn't that format.
fn findings(report: &str) -> Option<BTreeSet<Finding>> {
    let report: Value = serde_json::from_str(report).ok()?;
    report
        .get("comments")?
        .as_array()?
        .iter()
        .map(|comment| {
            let number = |key: &str| comment.get(key)?.as_u64()?.try_into().ok();
            let text = |key: &str| Some(comment.get(key)?.as_str()?.to_owned());
            Some(Finding {
                file: normalized(&text("file")?)?,
                line: number("line")?,
                column: number("column")?,
                code: comment.get("code")?.as_u64()?.try_into().ok()?,
                level: text("level")?,
                message: text("message")?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shebang_names_a_shell_by_path_or_through_env() {
        for start in [
            "#!/bin/sh\n",
            "#!/bin/bash -eu\necho",
            "#!/usr/bin/env bash\n",
            "#! /bin/dash",
            "#!/usr/bin/env -S ksh -e\n",
            "#!/bin/ash\n",
            "#!/usr/bin/env bats\n",
            "\u{feff}#!/bin/sh\n",
        ] {
            assert!(names_a_shell(start.as_bytes()), "{start:?}");
        }
    }

    #[test]
    fn other_first_lines_name_no_shell() {
        for start in [
            "",
            "echo hi\n",
            "#!/usr/bin/env python3\n",
            "#!/usr/bin/fish\n",
            "# /bin/sh\n",
            "[Unit]\nDescription=sh\n",
            "#!/usr/bin/env zsh\nsh",
        ] {
            assert!(!names_a_shell(start.as_bytes()), "{start:?}");
        }
    }

    #[test]
    fn a_directive_with_a_reason_passes() {
        let script = "\u{feff}#!/bin/sh\n# shellcheck source-path=SCRIPTDIR\nset -eu\n\
                      # shellcheck disable=SC2086,2046 # The list splits on purpose.\n\
                      rm $FILES\n#\tshellcheck source=lib/common.sh enable=require-variable-braces\n\
                      . ./lib/common.sh\n";
        assert_eq!(
            directive_problems("deploy/install.sh", script),
            Vec::<String>::new()
        );
        let library = "# shellcheck shell=bash source=../install.sh\nVALUE=1\n";
        assert_eq!(
            directive_problems("deploy/lib/common", library),
            Vec::<String>::new()
        );
    }

    #[test]
    fn directives_that_hide_findings_are_problems() {
        let cases = [
            ("\u{feff}# shellcheck disable=all\necho\n", "disables `all`"),
            (
                "set -eu\n# shellcheck disable=SC2086\nrm $a\n",
                "without a reason",
            ),
            (
                "set -eu\n#shellcheck disable=all # Everything.\n",
                "disables `all`",
            ),
            (
                "set -eu\n# shellcheck\tdisable=all # A tab.\n",
                "disables `all`",
            ),
            (
                "set -eu\n# shellcheck disable=SC1091,ALL # Too.\n",
                "disables `SC1091,ALL`",
            ),
            (
                "set -eu\n# shellcheck disable=1000-9999 # A range.\n",
                "disables `1000-9999`",
            ),
            (
                "set -eu\n# shellcheck disable=sc2086 # Lowercase.\n",
                "disables `sc2086`",
            ),
            (
                "set -eu\n# shellcheck disable=\"all\" # Quoted.\n",
                "disables `\"all\"`",
            ),
            ("set -eu\nf() { # shellcheck disable=all\n}\n", "after code"),
            ("#!/bin/sh\n# shellcheck shell=bash\n", "sets the shell"),
            (
                "set -eu\n# shellcheck source=/dev/null\n. ~/.profile\n",
                "points shellcheck at `/dev/null`",
            ),
            (
                "set -eu\n# shellcheck source=../../tools/lib.sh\n. x\n",
                "points shellcheck at `../../tools/lib.sh`",
            ),
            (
                "set -eu\n# shellcheck extended-analysis=false\n",
                "sets `extended-analysis`",
            ),
            ("set -eu\n# shellcheck DISABLE=all # r\n", "sets `DISABLE`"),
            (
                "set -eu\n# shellcheck disable=SC1072,SC1073 # r\n{\n",
                "a check of how shellcheck parses",
            ),
            (
                "set -eu\n# shellcheck source-path=/dev source=null\n. ./lib\n",
                "to look for sourced files",
            ),
            (
                "set -eu\n#\u{a0}shellcheck disable=SC2086 # r\nrm $a\n",
                "a space that isn't an ASCII space or tab",
            ),
        ];
        for (script, expected) in cases {
            let problems = directive_problems("deploy/install.sh", script);
            assert!(
                problems.iter().any(|problem| problem.contains(expected)),
                "{script:?} should report {expected:?}: {problems:#?}"
            );
        }
    }

    #[test]
    fn disables_name_no_check_and_every_other_byte_stays() {
        let script = "#!/bin/sh\n# shellcheck disable=SC2086 # r\nrm $a\n\
                      # shellcheck disable=SC1091,2046 source=lib.sh # r\n. ./lib.sh\n\
                      echo \"\n# shellcheck disable=SC2086 # \" && rm $b\n";
        assert_eq!(
            without_disables(script),
            "#!/bin/sh\n# shellcheck disable=SC0000 # r\nrm $a\n\
             # shellcheck disable=SC0000 source=lib.sh # r\n. ./lib.sh\n\
             echo \"\n# shellcheck disable=SC0000 # \" && rm $b\n"
        );
    }

    #[test]
    fn a_finding_in_a_continued_command_or_a_heredoc_counts_from_its_start() {
        let script = "set -eu\n# shellcheck disable=SC2086 # r\nrm \\\n  $a\n\
                      # shellcheck disable=SC2154 # r\ncat <<- EOF\n\t$port\n\tEOF\nrm $b\n";
        assert!(silenced_right_above(script, 4, 2086));
        assert!(silenced_right_above(script, 7, 2154));
        assert!(!silenced_right_above(script, 9, 2086));
    }

    #[test]
    fn a_silencing_directive_counts_only_right_above_its_line() {
        let script = "set -eu\n# shellcheck disable=SC2086 # r\n\n# More.\nrm $a\nrm $b\n";
        assert!(silenced_right_above(script, 5, 2086));
        assert!(!silenced_right_above(script, 6, 2086));
        assert!(!silenced_right_above(script, 5, 2046));
    }

    #[test]
    fn findings_are_read_from_json() {
        let report = r#"{"comments":[{"file":"deploy/./lib/../install.sh","line":3,"endLine":3,
            "column":6,"endColumn":8,"level":"warning","code":2086,"message":"Quote this."}]}"#;
        let found = findings(report).expect("the report should parse");
        let finding = found.first().expect("there should be a finding");
        assert_eq!(finding.file, "deploy/install.sh");
        assert_eq!((finding.line, finding.column, finding.code), (3, 6, 2086));
        assert!(findings("not json").is_none());
    }
}
