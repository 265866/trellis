//! The `trellis-policy` command.
//!
//! - `trellis-policy check [--manifest-path <Cargo.toml>]` runs every structure rule on the
//!   workspace that holds `<Cargo.toml>`, or, without it, on the one Cargo finds from the current
//!   directory, and prints each violation with its rule.
//! - `trellis-policy rule-changes <base> [--head <commit>]` fails when the changes since the merge
//!   base of `<base>` and the head change both the repository's rules and the code they check,
//!   which must land in separate pull requests. The head is `<commit>`, or, without `--head`, the
//!   working tree, uncommitted and untracked files included.
//!
//! Both exit with 0 when the rules hold, 1 when they don't, and 2 on a usage error or when they
//! can't read the workspace or its history.

use std::env;
use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;

use trellis_policy::{ChangedFiles, Head, Rule, Workspace};

/// The exit code for a command line, IO or git failure, apart from a broken rule.
const USAGE_ERROR: u8 = 2;

/// How to run the command.
const USAGE: &str = "usage: trellis-policy check [--manifest-path <Cargo.toml>]\n       \
                     trellis-policy rule-changes <base> [--head <commit>]";

fn main() -> ExitCode {
    let arguments: Vec<String> = env::args().skip(1).collect();
    let mut stdout = io::stdout().lock();
    let outcome = match arguments.as_slice() {
        [command] if command == "check" => check(None, &mut stdout),
        [command, option, manifest] if command == "check" && option == "--manifest-path" => {
            check(Some(Path::new(manifest)), &mut stdout)
        }
        [command, base] if command == "rule-changes" && !base.starts_with('-') => {
            rule_changes(base, &Head::WorkingTree, &mut stdout)
        }
        [command, base, option, head]
            if command == "rule-changes" && !base.starts_with('-') && option == "--head" =>
        {
            rule_changes(base, &Head::Commit(head.clone()), &mut stdout)
        }
        [option] if option == "--help" || option == "-h" => {
            writeln!(stdout, "{USAGE}").map(|()| ExitCode::SUCCESS)
        }
        _ => writeln!(io::stderr().lock(), "{USAGE}").map(|()| ExitCode::from(USAGE_ERROR)),
    };
    outcome.unwrap_or(ExitCode::from(USAGE_ERROR))
}

/// How an error line starts: as a GitHub Actions error annotation under GitHub Actions, and as
/// `error: ` elsewhere.
fn error_prefix() -> &'static str {
    if env::var_os("GITHUB_ACTIONS").is_some() {
        "::error::"
    } else {
        "error: "
    }
}

/// Runs every rule on the workspace that holds `manifest_path`, or the one Cargo finds from the
/// current directory, and writes each violation to `out`. A rule that can't read what it checks
/// doesn't stop the others, but the run then exits with [`USAGE_ERROR`].
fn check(manifest_path: Option<&Path>, out: &mut impl Write) -> io::Result<ExitCode> {
    let prefix = error_prefix();
    let workspace = match Workspace::load(manifest_path) {
        Ok(workspace) => workspace,
        Err(error) => {
            writeln!(out, "{prefix}couldn't load the workspace: {error}")?;
            return Ok(ExitCode::from(USAGE_ERROR));
        }
    };
    let mut violations = 0_usize;
    let mut failed = false;
    for rule in Rule::ALL {
        let name = rule.name();
        match rule.check(&workspace) {
            Ok(found) => {
                violations += found.len();
                for violation in found {
                    writeln!(out, "{prefix}[{name}] {violation}")?;
                }
            }
            Err(error) => {
                failed = true;
                writeln!(out, "{prefix}[{name}] the check couldn't run: {error}")?;
            }
        }
    }
    let root = workspace.root().display();
    if failed {
        return Ok(ExitCode::from(USAGE_ERROR));
    }
    if violations > 0 {
        writeln!(
            out,
            "{violations} structure rule violation(s) in {root}. Each line names the rule, the \
             file and what to do."
        )?;
        return Ok(ExitCode::FAILURE);
    }
    writeln!(
        out,
        "All {} structure rules hold in {root}.",
        Rule::ALL.len()
    )?;
    Ok(ExitCode::SUCCESS)
}

/// Checks the changes from the merge base of `base` and `head` to `head` in the repository in the
/// current directory, and writes the verdict to `out`.
fn rule_changes(base: &str, head: &Head, out: &mut impl Write) -> io::Result<ExitCode> {
    let prefix = error_prefix();
    let changed = match ChangedFiles::between(&env::current_dir()?, base, head) {
        Ok(changed) => changed,
        Err(error) => {
            writeln!(out, "{prefix}{error}")?;
            return Ok(ExitCode::from(USAGE_ERROR));
        }
    };
    if !changed.mixes_rules_and_code() {
        writeln!(
            out,
            "Rule changes stand alone: {} rule files and {} code files changed.",
            changed.rules().len(),
            changed.code().len()
        )?;
        return Ok(ExitCode::SUCCESS);
    }
    writeln!(
        out,
        "{prefix}These changes touch rules and the code they check together. Split them. If the \
         rule change tightens a rule, land the code fixes first, or add an `expect` with a reason \
         at each place the new rule flags, beside the rule; if it loosens one, such as \
         allowing a proc-macro crate or a dependency between crates, land the rule change first."
    )?;
    writeln!(out, "Rule changes:")?;
    for path in changed.rules() {
        writeln!(out, "  {path}")?;
    }
    writeln!(out, "Code changes:")?;
    for path in changed.code() {
        writeln!(out, "  {path}")?;
    }
    Ok(ExitCode::FAILURE)
}
