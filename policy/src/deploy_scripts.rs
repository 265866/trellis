//! The shell scripts of a deploy kit, the files under the root `deploy/` or a crate's
//! `crates/<crate>/deploy/`, pass shellcheck, since they are code that no other check reads.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, ErrorKind};
use std::num::NonZeroUsize;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use std::{env, fs, panic, thread};

use serde_json::Value;

use crate::policy_error::PolicyError;
use crate::repository_files;
use crate::violation::Violation;
use crate::workspace::Workspace;

/// The name of a deploy kit's directory, with its `/`: `deploy/` at the root, or in a crate's
/// directory, `crates/<crate>/deploy/`.
const DEPLOY_DIRECTORY: &str = "deploy/";

/// The extensions that make a kit file a shell script, in any case.
const SCRIPT_EXTENSIONS: [&str; 5] = ["sh", "bash", "dash", "ksh", "bats"];

/// The shells shellcheck reads, which a script's `#!` line can name.
const SHELLS: [&str; 6] = ["sh", "bash", "dash", "ksh", "ash", "bats"];

/// The directive keys a script may use, which tell shellcheck what it can't find out alone.
/// `disable` is checked apart, and any other key, such as `extended-analysis` or
/// `external-sources`, would turn checks off for the whole script.
const ALLOWED_DIRECTIVES: [&str; 4] = ["enable", "shell", "source", "source-path"];

/// What to do when shellcheck isn't installed.
const INSTALL_REMEDY: &str = "Install the shellcheck version the Set up section of CONTRIBUTING.md \
                              names; CI installs it too.";

/// The directory of the deploy kit that holds `path`, relative to the root with `/` separators,
/// with its `/`: `deploy/` for a file under the root `deploy/`, `crates/<crate>/deploy/` for one
/// under a crate's, or `None` for any other file. `<crate>` is a single directory, which
/// `workspace_shape` makes a member's. A kit's files are code to `trellis-policy rule-changes`,
/// apart from the ones that are rules by their names, so a kit change lands with the code change
/// it needs.
pub(crate) fn kit_of(path: &str) -> Option<&str> {
    if path.starts_with(DEPLOY_DIRECTORY) {
        return Some(&path[..DEPLOY_DIRECTORY.len()]);
    }
    let (krate, rest) = path.strip_prefix("crates/")?.split_once('/')?;
    let length = "crates/".len() + krate.len() + 1 + DEPLOY_DIRECTORY.len();
    (!krate.is_empty() && rest.starts_with(DEPLOY_DIRECTORY)).then(|| &path[..length])
}

/// Checks every shell script in a deploy kit, under the root `deploy/` or a crate's
/// `crates/<crate>/deploy/`, with shellcheck, and reports each finding at its file. A script is a
/// file whose name ends in `.sh`, `.bash`, `.dash`, `.ksh` or `.bats`, whose first line is a `#!`
/// line that names `sh`, `bash`, `dash`, `ksh`, `ash` or `bats`, or that sets its shell with a
/// `# shellcheck shell=<shell>` directive, as a library a script sources can. Each kit is checked
/// on its own: shellcheck runs on a copy that holds that kit's files alone, each at its path, with
/// `--norc` and without `SHELLCHECK_OPTS`, so neither a `.shellcheckrc` nor the environment can
/// turn a check off and a file outside the kit, another kit's included, can't be sourced unseen,
/// and with `--check-sourced`, so it reports the files a script sources too, looked up from the
/// script's directory. A finding in a file outside the kit, such as an `/etc/os-release` a script
/// sources from the host, isn't the kit's, so it isn't reported.
///
/// A kit file silences a finding with a `# shellcheck disable=SC<code> # <reason>` line above it,
/// where review sees it. shellcheck applies a directive to the whole command below it, a function
/// or a sourced file included, so the check measures what each directive hides: it runs
/// shellcheck a second time with every `disable` taken out, and a finding that only that run
/// reports fails when a directive closer to it would silence it, as [`Kit::closer_directives`]
/// finds by asking shellcheck. [`directive_problems`] says which other directives are violations.
/// A symbolic link in a kit is one too, since the kit's files are what review read, and so is a
/// file that sources a kit file by its absolute path in the checkout, which shellcheck would read
/// there instead of in the copy.
///
/// A project without a kit has no script, so the check runs nothing.
///
/// # Errors
///
/// Fails if the repository's files or a kit file can't be read or copied, or shellcheck doesn't
/// run, such as when it isn't installed.
pub(crate) fn check(workspace: &Workspace) -> Result<Vec<Violation>, PolicyError> {
    let root = workspace.root();
    let width = thread::available_parallelism().map_or(1, NonZeroUsize::get);
    let mut violations = Vec::new();
    let mut kits: BTreeMap<String, Kit> = BTreeMap::new();
    for file in repository_files::list(root)? {
        let Some(directory) = kit_of(&file) else {
            continue;
        };
        let directory = directory.to_owned();
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
            directive_problems(&directory, &file, &text)
                .into_iter()
                .map(|problem| Violation::new(file.clone(), problem)),
        );
        let kit = kits.entry(directory).or_insert_with(|| Kit::new(width));
        if is_script(&path, &text) {
            kit.scripts.push(file.clone());
        }
        kit.texts.insert(file, text);
    }
    for kit in kits.values() {
        violations.extend(kit.check(root, width)?);
    }
    Ok(violations)
}

/// A violation for each finding in `unsilenced`, the run with every `disable` taken out, that the
/// run as written, `reported`, doesn't report, when a directive closer to it would silence it.
/// Up to `width` groups of findings, those of one code in one file, are probed at once.
fn hidden_too_far(
    kit: &Kit,
    reported: &BTreeSet<Finding>,
    unsilenced: &BTreeSet<Finding>,
    width: usize,
) -> Result<Vec<Violation>, PolicyError> {
    let mut violations = Vec::new();
    let mut groups: BTreeMap<(&str, u32), Vec<&Finding>> = BTreeMap::new();
    let newly_reported = kit
        .own(unsilenced)
        .filter(|finding| !reported.contains(finding));
    for finding in newly_reported {
        groups
            .entry((finding.file.as_str(), finding.code))
            .or_default()
            .push(finding);
    }
    let groups: Vec<Vec<&Finding>> = groups.into_values().collect();
    for chunk in groups.chunks(width) {
        let each_width = (width / chunk.len()).max(1);
        let closer = in_parallel(chunk, |group| {
            kit.closer_directives(group, unsilenced, each_width)
        })?;
        for (finding, closer) in chunk.iter().flatten().zip(closer.into_iter().flatten()) {
            let Some(closer) = closer else {
                continue;
            };
            violations.push(Violation::new(
                finding.file.clone(),
                format!(
                    "A shellcheck directive hides SC{code} at line {line}, but one right above \
                     line {closer} would silence it there alone, so the directive that hid it \
                     hides more than it says: a whole function, block or sourced file, or the \
                     rest of the file. Put `# shellcheck disable=SC{code} # <reason>` on the line \
                     above line {closer}, and take the code out of the directive that hid it: \
                     one for SC{code} above the function, block or file that holds line {line}, \
                     or above the line that sources this file.",
                    code = finding.code,
                    line = finding.line,
                ),
            ));
        }
    }
    Ok(violations)
}

/// The kit files among `texts` that shellcheck may read, the scripts and the files without an
/// extension, with a line that sources a path in any kit in the checkout at `root` by its absolute
/// path: `. /path` or `source /path` where a command starts, with the path's `.` and `..` parts
/// and its symbolic links resolved. shellcheck follows only such a literal path.
fn sourced_from_checkout(
    root: &Path,
    texts: &BTreeMap<String, String>,
    scripts: &[String],
) -> Vec<String> {
    let Ok(root) = fs::canonicalize(root) else {
        return Vec::new();
    };
    let in_checkout = |path: &str| {
        let path = path.trim_matches(['"', '\'']);
        path.starts_with('/')
            && normalized(path)
                .and_then(|path| fs::canonicalize(format!("/{path}")).ok())
                .and_then(|path| path.strip_prefix(&root).ok().map(Path::to_path_buf))
                .is_some_and(|path| kit_of(&path.to_string_lossy().replace('\\', "/")).is_some())
    };
    let read = |file: &str| {
        scripts.iter().any(|script| script == file) || Path::new(file).extension().is_none()
    };
    texts
        .iter()
        .filter(|(file, text)| {
            read(file)
                && text
                    .lines()
                    .any(|line| sources_where_a_command_starts(line, in_checkout))
        })
        .map(|(file, _)| file.clone())
        .collect()
}

/// Whether `line` sources a path `matches` accepts, with `.` or `source` where a command starts:
/// at the start of the line, after `;`, `&`, `|`, a bracket or a brace, or after a word such as
/// `if`, `then` or `do` that a command follows.
fn sources_where_a_command_starts(line: &str, matches: impl Fn(&str) -> bool) -> bool {
    let mut spaced = String::with_capacity(line.len() * 2);
    for character in line.chars() {
        if ";&|(){}".contains(character) {
            spaced.extend([' ', character, ' ']);
        } else {
            spaced.push(character);
        }
    }
    let words: Vec<&str> = spaced.split_whitespace().collect();
    words.windows(2).enumerate().any(|(at, pair)| {
        let at_command = at
            .checked_sub(1)
            .and_then(|before| words.get(before))
            .is_none_or(|before| {
                matches!(
                    *before,
                    ";" | "&"
                        | "|"
                        | "("
                        | ")"
                        | "{"
                        | "}"
                        | "!"
                        | "if"
                        | "elif"
                        | "then"
                        | "else"
                        | "do"
                        | "while"
                        | "until"
                        | "time"
                )
            });
        at_command && matches!(pair[0], "." | "source") && matches(pair[1])
    })
}

/// What `run` returns for each of `items`, run at once on threads of their own, in order, or the
/// first error.
fn in_parallel<Item: Sync, Output: Send>(
    items: &[Item],
    run: impl Fn(&Item) -> Result<Output, PolicyError> + Sync,
) -> Result<Vec<Output>, PolicyError> {
    if let [item] = items {
        return Ok(vec![run(item)?]);
    }
    thread::scope(|scope| {
        let handles: Vec<_> = items.iter().map(|item| scope.spawn(|| run(item))).collect();
        handles
            .into_iter()
            .map(|handle| {
                handle
                    .join()
                    .unwrap_or_else(|panic| panic::resume_unwind(panic))
            })
            .collect()
    })
}

/// A deploy kit's files, which shellcheck reads in a copy that holds them alone.
struct Kit {
    /// Each file in the kit, relative to the root, mapped to its text.
    texts: BTreeMap<String, String>,
    /// The files that are scripts, which shellcheck runs on.
    scripts: Vec<String>,
    /// The shellcheck runs that may go at once, however many threads probe.
    slots: Slots,
}

/// A count of the runs that may go at once, which a run waits on until one is free.
struct Slots {
    /// How many are free.
    free: AtomicUsize,
}

impl Slots {
    /// `count` slots, all free.
    fn new(count: usize) -> Self {
        Self {
            free: AtomicUsize::new(count.max(1)),
        }
    }

    /// What `run` returns, run once a slot is free, which it holds until it returns.
    fn hold<Output>(&self, run: impl FnOnce() -> Output) -> Output {
        while self
            .free
            .try_update(Ordering::AcqRel, Ordering::Acquire, |free| {
                free.checked_sub(1)
            })
            .is_err()
        {
            thread::sleep(Duration::from_millis(5));
        }
        let output = run();
        self.free.fetch_add(1, Ordering::AcqRel);
        output
    }
}

impl Kit {
    /// A kit with no files yet, whose shellcheck runs go up to `width` at once.
    fn new(width: usize) -> Self {
        Self {
            texts: BTreeMap::new(),
            scripts: Vec::new(),
            slots: Slots::new(width),
        }
    }

    /// The violations of the kit's scripts in the checkout at `root`: the files that source a kit
    /// file by its absolute path there, the findings shellcheck reports in the kit's files, and
    /// the findings a directive hides where a closer one would silence them alone. A kit without
    /// a script has none.
    fn check(&self, root: &Path, width: usize) -> Result<Vec<Violation>, PolicyError> {
        if self.scripts.is_empty() {
            return Ok(Vec::new());
        }
        let mut violations: Vec<Violation> =
            sourced_from_checkout(root, &self.texts, &self.scripts)
                .into_iter()
                .map(|file| {
                    Violation::new(
                        file,
                        "This file sources a kit file by its absolute path in the checkout. \
                         shellcheck reads that path in the checkout instead of the kit's copy, with \
                         its directives as they are, so source it relative to the script, such as \
                         `. \"$(dirname \"$0\")/lib/common\"`."
                            .to_owned(),
                    )
                })
                .collect();
        let reported = self.shellcheck(|_, text| text.to_owned())?;
        let disables = self.texts.values().any(|text| text.contains("disable="));
        let unsilenced = if disables {
            self.shellcheck(|_, text| without_disables(text))?
        } else {
            BTreeSet::new()
        };
        violations.extend(hidden_too_far(self, &reported, &unsilenced, width)?);
        violations.extend(self.own(&reported).map(Finding::violation));
        Ok(violations)
    }

    /// The findings in `found` in the kit's files, leaving out those in a host file a script
    /// sources by its absolute path, which aren't the kit's and which no directive in it could
    /// silence.
    fn own<'found>(
        &self,
        found: &'found BTreeSet<Finding>,
    ) -> impl Iterator<Item = &'found Finding> {
        found
            .iter()
            .filter(|finding| self.texts.contains_key(&finding.file))
    }

    /// The findings shellcheck reports for the kit's scripts in a copy that holds the kit's files
    /// alone, each at its path, made in a temporary directory from each file's text as `edit`,
    /// given the file and its text, changes it, in kit files and in the host files they source.
    fn shellcheck(
        &self,
        edit: impl Fn(&str, &str) -> String,
    ) -> Result<BTreeSet<Finding>, PolicyError> {
        self.slots.hold(|| self.shellcheck_now(edit))
    }

    /// What [`Kit::shellcheck`] returns, run without waiting for a slot.
    fn shellcheck_now(
        &self,
        edit: impl Fn(&str, &str) -> String,
    ) -> Result<BTreeSet<Finding>, PolicyError> {
        let copy = tempfile::tempdir().map_err(|source| PolicyError::Read {
            path: env::temp_dir(),
            source,
        })?;
        for (file, text) in &self.texts {
            let path = copy.path().join(file);
            let write_error = |source| PolicyError::Read {
                path: path.clone(),
                source,
            };
            if let Some(directory) = path.parent() {
                fs::create_dir_all(directory).map_err(write_error)?;
            }
            fs::write(&path, edit(file, text)).map_err(write_error)?;
        }
        shellcheck(copy.path(), &self.scripts)
    }

    /// For each finding of `group`, findings of one code in one file that shellcheck reports only
    /// in `unsilenced`, the run with every `disable` taken out, the line, counted from 1, right
    /// above which a directive for the code would silence it alone, when the directive that hid it
    /// isn't there, or `None` if it is. The probes run up to `width` at once.
    ///
    /// shellcheck refuses a directive inside a command, such as after a `|`, `&&` or `||`, inside
    /// `[[ ]]`, above a `case` pattern or above the `done` that ends a loop, and ignores one in a
    /// string, a heredoc or an array, but takes one above a command in a function or a block. So
    /// the check asks it. From each finding's line up to the topmost directive for its code in its
    /// file, or to its first line, it adds a probe directive above each line in the run with every
    /// `disable` taken out. The closest probe that silences the finding, with every other finding
    /// still reported and no new parse error, is where its directive belongs, and a directive for
    /// its code must sit among the comments right above it. One probe answers for every finding
    /// of the group below it, so the findings in one heredoc share their probes. A line after one
    /// that ends in a `\`, before any comment, is skipped, since a comment there would end the
    /// command the `\` continues.
    fn closer_directives(
        &self,
        group: &[&Finding],
        unsilenced: &BTreeSet<Finding>,
        width: usize,
    ) -> Result<Vec<Option<usize>>, PolicyError> {
        let Some(first) = group.first() else {
            return Ok(Vec::new());
        };
        let Some(text) = self.texts.get(&first.file) else {
            return Ok(vec![None; group.len()]);
        };
        let lines: Vec<&str> = text.lines().collect();
        let candidates: Vec<Vec<usize>> = group
            .iter()
            .map(|finding| probe_lines(&lines, finding))
            .collect();
        let mut probed: BTreeMap<usize, BTreeSet<Finding>> = BTreeMap::new();
        let mut size = group.len();
        loop {
            let closest: Vec<Probing> = group
                .iter()
                .zip(&candidates)
                .map(|(finding, own)| resolve(own, &probed, finding, unsilenced))
                .collect();
            let mut next: Vec<usize> = candidates
                .iter()
                .zip(&closest)
                .filter(|(_, closest)| **closest == Probing::Unknown)
                .filter_map(|(own, _)| own.iter().find(|index| !probed.contains_key(index)))
                .copied()
                .collect();
            if next.is_empty() {
                let placed = |index: usize| directive_right_above(&lines, index, first.code);
                return Ok(closest
                    .into_iter()
                    .map(|closest| match closest {
                        Probing::SilencedAt(index) if !placed(index) => Some(index + 1),
                        Probing::SilencedAt(_) | Probing::Unknown | Probing::Unsilenced => None,
                    })
                    .collect());
            }
            let mut below: Vec<usize> = candidates
                .iter()
                .zip(&closest)
                .filter(|(_, closest)| **closest == Probing::Unknown)
                .flat_map(|(own, _)| own.iter().copied())
                .filter(|index| !probed.contains_key(index) && !next.contains(index))
                .collect();
            below.sort_unstable_by(|a, b| b.cmp(a));
            below.dedup();
            next.sort_unstable();
            next.dedup();
            next.extend(below.into_iter().take(size.saturating_sub(next.len())));
            let found = in_parallel(&next, |&index| self.probe(first, index))?;
            probed.extend(next.into_iter().zip(found));
            size = (size * 2).min(width.max(1)).max(group.len());
        }
    }

    /// The findings of the run with every `disable` taken out and a directive for `hidden`'s code
    /// added above the line at `index`, counted from 0, of its file.
    fn probe(&self, hidden: &Finding, index: usize) -> Result<BTreeSet<Finding>, PolicyError> {
        let probe = format!("# shellcheck disable=SC{} # probe\n", hidden.code);
        self.shellcheck(|file, text| {
            let unsilenced = without_disables(text);
            if file != hidden.file {
                return unsilenced;
            }
            let mut probed = String::with_capacity(unsilenced.len() + probe.len());
            for (at, piece) in unsilenced.split_inclusive('\n').enumerate() {
                if at == index {
                    probed.push_str(&probe);
                }
                probed.push_str(piece);
            }
            probed
        })
    }
}

/// The lines, counted from 0, above which a probe for `hidden` may go, closest first: from its own
/// line up to the line below the topmost directive for its code in its file, or to the first
/// line. A blank or comment line is left out, and so is a line after one that ends in a `\\` and
/// holds no `#`, since a comment there would end the command the `\\` continues. A line with a
/// `#` is probed anyway, since a probe after a real `\\` adds a parse error and doesn't count.
fn probe_lines(lines: &[&str], hidden: &Finding) -> Vec<usize> {
    let own = hidden.line.saturating_sub(1).min(lines.len());
    let lowest = lines[..own]
        .iter()
        .position(|line| disables_code(line, hidden.code))
        .map_or(0, |topmost| topmost + 1);
    let continued = |index: usize| {
        index
            .checked_sub(1)
            .and_then(|previous| lines.get(previous))
            .is_some_and(|previous| {
                !previous.contains('#') && previous.ends_with('\\') && !previous.ends_with("\\\\")
            })
    };
    (lowest..=own)
        .rev()
        .filter(|&index| {
            lines.get(index).is_some_and(|line| !is_comment(line)) && !continued(index)
        })
        .collect()
}

/// What the probes so far say of a hidden finding.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Probing {
    /// A line closer than any a probe silenced it at has no probe yet.
    Unknown,
    /// The closest probe that silences it is above the line at this index, counted from 0.
    SilencedAt(usize),
    /// No probe silences it.
    Unsilenced,
}

/// What the probes in `probed` say of `hidden`, whose probes may go above `own`, closest first.
fn resolve(
    own: &[usize],
    probed: &BTreeMap<usize, BTreeSet<Finding>>,
    hidden: &Finding,
    unsilenced: &BTreeSet<Finding>,
) -> Probing {
    for &index in own {
        let Some(found) = probed.get(&index) else {
            return Probing::Unknown;
        };
        if silences(found, hidden, index, unsilenced) {
            return Probing::SilencedAt(index);
        }
    }
    Probing::Unsilenced
}

/// Whether a directive that disables `code` sits among the blank and comment lines right above the
/// line at `index`, counted from 0, of `lines`.
fn directive_right_above(lines: &[&str], index: usize, code: u32) -> bool {
    lines[..index.min(lines.len())]
        .iter()
        .rev()
        .take_while(|line| is_comment(line))
        .any(|line| disables_code(line, code))
}

/// Whether `line` is a directive on a line of its own that disables `code`.
fn disables_code(line: &str, code: u32) -> bool {
    Directive::of(line).is_some_and(|directive| {
        directive.code.trim().is_empty() && directive.disabled_codes().contains(&code)
    })
}

/// Whether `found`, the findings of a probe above the line at `index`, counted from 0, of
/// `hidden`'s file, silences `hidden` while every finding of another code in `unsilenced`, the
/// run without the probe, is still reported and no new parse error is.
fn silences(
    found: &BTreeSet<Finding>,
    hidden: &Finding,
    index: usize,
    unsilenced: &BTreeSet<Finding>,
) -> bool {
    let moved = |finding: &Finding| finding.moved_below(&hidden.file, index);
    let reported = |expected: &Finding| found.iter().any(|finding| finding.is_at(expected));
    let others_kept = unsilenced
        .iter()
        .filter(|finding| finding.code != hidden.code)
        .all(|finding| reported(&moved(finding)));
    let new_parse_error = found.iter().any(|finding| {
        finding.is_parse_error()
            && !unsilenced
                .iter()
                .any(|before| finding.is_at(&moved(before)))
    });
    !reported(&moved(hidden)) && others_kept && !new_parse_error
}

/// Whether `line` is blank or a comment.
fn is_comment(line: &&str) -> bool {
    let line = line.trim_start_matches([' ', '\t']);
    line.is_empty() || line.starts_with('#')
}

/// What is wrong with the shellcheck directives in `text`, the text of `file` in the kit whose
/// directory is `kit`, as [`kit_of`] names it. shellcheck reads a directive in a comment after any
/// run of spaces and tabs, at the start of a line or after code such as `f() {` or `then`. So each
/// of these is a problem:
///
/// - a directive after code on its line, where review misses it;
/// - a `disable` whose value isn't a list of codes, such as `all`, a range or a quoted value;
/// - a `disable` without a reason after it, as a `#` comment;
/// - `shell` in a file whose `#!` line names its shell already, which it would override;
/// - `source` that names a file outside the kit or an absolute path, such as `/dev/null`, which
///   would keep shellcheck from reading what the script sources, and `source-path` that names a
///   directory outside the kit;
/// - any key but `disable`, `enable`, `shell`, `source` and `source-path`, which turns checks off
///   for the whole file.
///
/// A line in a heredoc that looks like a directive is read as one too, which shellcheck doesn't.
fn directive_problems(kit: &str, file: &str, text: &str) -> Vec<String> {
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
        let reason = directive.split_once('#').map_or("", |(_, reason)| reason);
        for setting in Directive::settings_of(directive).split_whitespace() {
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
                "source-path" if !source_path_inside_kit(kit, directory, value) => {
                    problems.push(format!(
                        "Line {number} points shellcheck at `{value}` to look for sourced files, \
                         which isn't in the kit. Write `SCRIPTDIR`, a directory below it such as \
                         `SCRIPTDIR/lib`, or a path from the root such as `{kit}lib`."
                    ));
                }
                "source" if !inside_kit(kit, directory, value) => problems.push(format!(
                    "Line {number} points shellcheck at `{value}`, which isn't a file in \
                     `{kit}`, so it can't read what the script sources. Point it at the file \
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
    /// and tabs, `shellcheck`, at least one space or tab, then a `key=value` setting. A comment
    /// such as `# shellcheck runs in CI` isn't one, which in a script shellcheck reports as a
    /// directive it can't parse.
    fn of(line: &'line str) -> Option<Self> {
        line.match_indices('#').find_map(|(at, _)| {
            let after = line[at + 1..].trim_start_matches(char::is_whitespace);
            let rest = after.strip_prefix("shellcheck")?;
            let text = rest.trim_start_matches(char::is_whitespace);
            let sets_a_key = Self::settings_of(text)
                .split_whitespace()
                .next()
                .is_some_and(|setting| setting.contains('='));
            (text.len() < rest.len() && sets_a_key).then_some(Self {
                code: &line[..at],
                text,
            })
        })
    }

    /// The settings in `text`, a directive's text after `shellcheck`, before any `#` comment.
    fn settings_of(text: &str) -> &str {
        text.split_once('#').map_or(text, |(settings, _)| settings)
    }

    /// The directive's settings, before any `#` comment.
    fn settings(&self) -> &'line str {
        Self::settings_of(self.text)
    }

    /// Whether the directive's line spells it with a space other than an ASCII space or tab, such
    /// as a non-breaking space, which shellcheck reads but a reader may not see.
    fn has_unusual_spaces(line: &str) -> bool {
        line.chars()
            .any(|character| character.is_whitespace() && !matches!(character, ' ' | '\t'))
    }

    /// The codes this directive's `disable` settings name, as numbers.
    fn disabled_codes(&self) -> BTreeSet<u32> {
        self.settings()
            .split_whitespace()
            .filter_map(|setting| setting.strip_prefix("disable="))
            .flat_map(|value| value.split(','))
            .filter_map(code_number)
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

/// The number of `code`, a code such as `SC2086` or `2086`, as shellcheck reads it, leading zeros
/// and all, or `None` if it isn't one.
fn code_number(code: &str) -> Option<u32> {
    code.strip_prefix("SC").unwrap_or(code).parse().ok()
}

/// Whether `value`, a list of codes, names one of shellcheck's parse checks, SC1000 to SC1999,
/// other than SC1090 and SC1091, which are about the files a script sources.
fn disables_a_parse_check(value: &str) -> bool {
    value
        .split(',')
        .filter_map(code_number)
        .any(|code| (1000..=1999).contains(&code) && !matches!(code, 1090 | 1091))
}

/// Whether `source`, a `source=` or `source-path=` value in a file in `directory`, names `kit`, the
/// kit's directory, or a path under it: a relative path that stays inside the kit, read from the
/// file's directory. A path into another kit, or into the crate around a crate's kit, leaves it.
fn inside_kit(kit: &str, directory: &str, source: &str) -> bool {
    if source.starts_with('/') || source.contains('\\') {
        return false;
    }
    normalized(&format!("{directory}/{source}")).is_some_and(|path| {
        path.strip_prefix(kit.trim_end_matches('/'))
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
    })
}

/// Whether `value`, a `source-path=` value in a file in `directory`, names `kit`, the kit's
/// directory, or a directory under it. shellcheck reads `SCRIPTDIR` as the file's directory and
/// any other relative path from the directory it runs in, the copy's root, which holds the kit's
/// files alone.
fn source_path_inside_kit(kit: &str, directory: &str, value: &str) -> bool {
    match value.strip_prefix("SCRIPTDIR") {
        Some("") => true,
        Some(rest) => rest
            .strip_prefix('/')
            .is_some_and(|rest| inside_kit(kit, directory, rest)),
        None => normalized(value).is_some_and(|path| path.is_empty()) || inside_kit(kit, "", value),
    }
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

/// Whether the file at `path`, whose text is `text`, is a shell script: by its extension, by the
/// shell its `#!` line names, or by a `# shellcheck shell=<shell>` directive, which a library a
/// script sources needs to be checked on its own.
fn is_script(path: &Path, text: &str) -> bool {
    let has_script_extension = path.extension().is_some_and(|extension| {
        SCRIPT_EXTENSIONS
            .iter()
            .any(|script| extension.eq_ignore_ascii_case(script))
    });
    has_script_extension || names_a_shell(text.as_bytes()) || sets_its_shell(text)
}

/// Whether `text` has a directive on a line of its own, before its first command, that sets the
/// shell, such as `# shellcheck shell=bash`. A comment further down, such as in a README's code
/// block, doesn't make a file a script.
fn sets_its_shell(text: &str) -> bool {
    text.lines()
        .take_while(|line| {
            let line = line.trim_start_matches([' ', '\t', '\u{feff}']);
            line.is_empty() || line.starts_with('#')
        })
        .filter_map(Directive::of)
        .any(|directive| {
            directive.code.trim().is_empty()
                && directive
                    .settings()
                    .split_whitespace()
                    .any(|setting| setting.starts_with("shell="))
        })
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
    /// The file, relative to the root, its kit's directory included.
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
    /// The finding as it stands once a line is added above the line at `index`, counted from 0,
    /// of `file`.
    fn moved_below(&self, file: &str, index: usize) -> Self {
        let moved = self.file == file && self.line > index;
        Self {
            file: self.file.clone(),
            line: self.line + usize::from(moved),
            column: self.column,
            code: self.code,
            level: self.level.clone(),
            message: self.message.clone(),
        }
    }

    /// Whether the finding is at `other`'s place, with its code.
    fn is_at(&self, other: &Self) -> bool {
        self.file == other.file
            && self.line == other.line
            && self.column == other.column
            && self.code == other.code
    }

    /// Whether the finding is one of shellcheck's parse checks, SC1000 to SC1999.
    fn is_parse_error(&self) -> bool {
        (1000..=1999).contains(&self.code)
    }

    /// The finding as a violation of its file.
    fn violation(&self) -> Violation {
        Violation::new(
            self.file.clone(),
            format!(
                "shellcheck reports, at line:column {}:{}: {}: {} [SC{}]. Fix the script, or \
                 silence the finding with `# shellcheck disable=SC{} # <reason>` as close above \
                 it as shellcheck takes a directive: above its line, or above the command it \
                 continues.",
                self.line, self.column, self.level, self.message, self.code, self.code
            ),
        )
    }
}

/// Runs shellcheck on `scripts`, relative to `copy`, and reads its findings, those in a file a
/// script sources by its absolute path included.
fn shellcheck(copy: &Path, scripts: &[String]) -> Result<BTreeSet<Finding>, PolicyError> {
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
        .current_dir(copy)
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
/// path's `.` and `..` parts resolved and a leading `/` kept, or `None` if the report isn't that
/// format.
fn findings(report: &str) -> Option<BTreeSet<Finding>> {
    let report: Value = serde_json::from_str(report).ok()?;
    report
        .get("comments")?
        .as_array()?
        .iter()
        .map(|comment| {
            let number = |key: &str| comment.get(key)?.as_u64()?.try_into().ok();
            let text = |key: &str| Some(comment.get(key)?.as_str()?.to_owned());
            let file = text("file")?;
            let root = if file.starts_with('/') { "/" } else { "" };
            Some(Finding {
                file: format!("{root}{}", normalized(&file)?),
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
            directive_problems("deploy/", "deploy/install.sh", script),
            Vec::<String>::new()
        );
        let library = "# shellcheck shell=bash source=../install.sh\nVALUE=1\n";
        assert_eq!(
            directive_problems("deploy/", "deploy/lib/common", library),
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
            (
                "set -eu\n# shellcheck disable=SC01083 # r\necho }\n",
                "a check of how shellcheck parses",
            ),
        ];
        for (script, expected) in cases {
            let problems = directive_problems("deploy/", "deploy/install.sh", script);
            assert!(
                problems.iter().any(|problem| problem.contains(expected)),
                "{script:?} should report {expected:?}: {problems:#?}"
            );
        }
    }

    #[test]
    fn a_comment_that_sets_nothing_and_a_path_to_the_kit_are_no_problem() {
        let unit = "[Unit]\n# shellcheck is run in CI\n## shellcheck and the kit\n";
        assert_eq!(
            directive_problems("deploy/", "deploy/app.service", unit),
            Vec::<String>::new()
        );
        let script = "set -eu\n# shellcheck source-path=.\n# shellcheck source-path=deploy/lib\n\
                      # shellcheck source-path=SCRIPTDIR/lib\n. ./lib.sh\n";
        assert_eq!(
            directive_problems("deploy/", "deploy/run.sh", script),
            Vec::<String>::new()
        );
        let outside =
            directive_problems("deploy/", "deploy/run.sh", "# shellcheck source-path=..\n");
        assert!(
            outside
                .iter()
                .any(|problem| problem.contains("to look for sourced files")),
            "{outside:#?}"
        );
        for value in ["lib", "SCRIPTDIR/../..", "../deploy"] {
            let problems = directive_problems(
                "deploy/",
                "deploy/a/run.sh",
                &format!("# shellcheck source-path={value}\n"),
            );
            assert!(
                problems
                    .iter()
                    .any(|problem| problem.contains("to look for sourced files")),
                "{value}: {problems:#?}"
            );
        }
    }

    #[test]
    fn a_kit_is_the_root_deploy_directory_or_a_crate_s() {
        for (path, kit) in [
            ("deploy/install.sh", "deploy/"),
            ("deploy/lib/common", "deploy/"),
            ("crates/demo/deploy/install.sh", "crates/demo/deploy/"),
            (
                "crates/demo_core/deploy/systemd/app.service",
                "crates/demo_core/deploy/",
            ),
        ] {
            assert_eq!(kit_of(path), Some(kit), "{path}");
        }
        for path in [
            "deployment/install.sh",
            "Deploy/install.sh",
            "deploy",
            "crates/demo/deploy",
            "crates/demo/src/deploy/install.sh",
            "crates/demo/deployment/install.sh",
            "crates//deploy/install.sh",
            "crates/deploy/install.sh",
            "tools/deploy/install.sh",
        ] {
            assert_eq!(kit_of(path), None, "{path}");
        }
    }

    #[test]
    fn a_crate_kit_s_directives_stay_inside_it() {
        let kit = "crates/demo/deploy/";
        let inside = "set -eu\n# shellcheck source-path=SCRIPTDIR/lib\n\
                      # shellcheck source-path=crates/demo/deploy/lib source=lib/common.sh\n\
                      # shellcheck source-path=.\n. ./lib/common.sh\n";
        assert_eq!(
            directive_problems(kit, "crates/demo/deploy/run.sh", inside),
            Vec::<String>::new()
        );
        for (directive, expected) in [
            (
                "source=../src/setup.sh",
                "points shellcheck at `../src/setup.sh`, which isn't a file in \
                 `crates/demo/deploy/`",
            ),
            (
                "source=../../../deploy/common.sh",
                "isn't a file in `crates/demo/deploy/`",
            ),
            (
                "source=../../demo_core/deploy/common.sh",
                "isn't a file in `crates/demo/deploy/`",
            ),
            (
                "source-path=deploy",
                "a path from the root such as `crates/demo/deploy/lib`",
            ),
            ("source-path=SCRIPTDIR/..", "to look for sourced files"),
            ("source-path=crates/demo", "to look for sourced files"),
        ] {
            let problems = directive_problems(
                kit,
                "crates/demo/deploy/run.sh",
                &format!("set -eu\n# shellcheck {directive}\n. ./x\n"),
            );
            assert!(
                problems.iter().any(|problem| problem.contains(expected)),
                "{directive} should report {expected:?}: {problems:#?}"
            );
        }
        let root = directive_problems(
            "deploy/",
            "deploy/run.sh",
            "# shellcheck source=../crates/demo/deploy/common.sh\n",
        );
        assert!(
            root.iter()
                .any(|problem| problem.contains("isn't a file in `deploy/`")),
            "{root:#?}"
        );
    }

    #[test]
    fn a_script_is_known_by_its_name_its_shebang_or_its_shell_directive() {
        for (name, text) in [
            ("deploy/run.SH", ""),
            ("deploy/update", "#!/bin/sh\n"),
            ("deploy/conf.d/app.conf", "# shellcheck shell=sh\nPORT=1\n"),
        ] {
            assert!(is_script(Path::new(name), text), "{name}");
        }
        for (name, text) in [
            ("deploy/Containerfile", "FROM scratch\nRUN echo $a\n"),
            ("deploy/Caddyfile", "example.com {\n}\n"),
            ("deploy/lib/common", "clean() {\n  rm -rf $1\n}\n"),
            (
                "deploy/README.md",
                "# The kit\n\nA library starts:\n\n```sh\n# shellcheck shell=bash\n```\n",
            ),
        ] {
            assert!(!is_script(Path::new(name), text), "{name}");
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
    fn findings_are_read_from_json() {
        let report = r#"{"comments":[{"file":"deploy/./lib/../install.sh","line":3,"endLine":3,
            "column":6,"endColumn":8,"level":"warning","code":2086,"message":"Quote this."}]}"#;
        let found = findings(report).expect("the report should parse");
        let finding = found.first().expect("there should be a finding");
        assert_eq!(finding.file, "deploy/install.sh");
        assert_eq!((finding.line, finding.column, finding.code), (3, 6, 2086));
        assert!(findings("not json").is_none());
        let host = r#"{"comments":[{"file":"/etc/../etc/os-release","line":1,"column":1,
            "level":"warning","code":2034,"message":"Unused."}]}"#;
        let found = findings(host).expect("the report should parse");
        assert_eq!(
            found.first().map(|finding| finding.file.as_str()),
            Some("/etc/os-release")
        );
    }
}
