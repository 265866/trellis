//! The `trellis-policy` command.
//!
//! - `trellis-policy check [--manifest-path <Cargo.toml>]` runs every structure rule on the
//!   workspace that holds `<Cargo.toml>`, or, without it, on the one Cargo finds from the current
//!   directory, and prints each violation with its rule.
//! - `trellis-policy rule-changes <base> [--head <commit>]` fails when the changes since the merge
//!   base of `<base>` and the head change both the repository's rules and the code they check,
//!   which must land in separate pull requests. The head is `<commit>`, or, without `--head`, the
//!   working tree, uncommitted and untracked files included. It passes with nothing to compare
//!   when `<base>` isn't a commit and the repository has no remote-tracking branches, as before
//!   its first push, so a new project's checks run before its repository exists.
//! - `trellis-policy web-notices <about.toml> <web-licenses.toml> <node_modules> <output>` writes
//!   to `<output>` the license notices of every npm package installed under `<node_modules>`,
//!   development and transitive ones included. It first removes `<output>` from an earlier run,
//!   and fails, writing nothing, when a package's license isn't satisfied by the `accepted` array
//!   of `<about.toml>`, the one cargo-about reads, together with the licenses
//!   `<web-licenses.toml>` accepts for that package by name. An entry there for a package that
//!   isn't installed is allowed, so a license can be accepted before its package arrives.
//! - `trellis-policy stale-web-licenses <web-licenses.toml> <node_modules>` fails on each entry
//!   of `<web-licenses.toml>` that names no package installed under `<node_modules>`.
//!
//! Each exits with 0 when the rules hold, 1 when they don't, and 2 on a usage error, when it
//! can't read the workspace, its history, or the files it was given, or when it can't write its
//! output.

use std::ffi::OsString;
use std::fmt::Display;
use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;
use std::{env, fs};

use trellis_policy::{
    AcceptedLicenses, ChangedFiles, Head, LicenseVerdict, NpmPackage, Rule, WebLicenses,
    WebNotices, Workspace,
};

/// The exit code for a command line, IO or git failure, apart from a broken rule.
const USAGE_ERROR: u8 = 2;

/// How to run the command.
const USAGE: &str = "usage: trellis-policy check [--manifest-path <Cargo.toml>]\n       \
                     trellis-policy rule-changes <base> [--head <commit>]\n       \
                     trellis-policy web-notices <about.toml> <web-licenses.toml> <node_modules> \
                     <output>\n       \
                     trellis-policy stale-web-licenses <web-licenses.toml> <node_modules>";

fn main() -> ExitCode {
    let arguments: Result<Vec<String>, OsString> =
        env::args_os().skip(1).map(OsString::into_string).collect();
    let outcome = match arguments {
        Ok(arguments) => run(&arguments),
        Err(argument) => writeln!(
            io::stderr().lock(),
            "error: the argument `{}` isn't UTF-8\n{USAGE}",
            argument.display()
        )
        .map(|()| ExitCode::from(USAGE_ERROR)),
    };
    outcome.unwrap_or(ExitCode::from(USAGE_ERROR))
}

/// Runs the command `arguments` name.
fn run(arguments: &[String]) -> io::Result<ExitCode> {
    let mut stdout = io::stdout().lock();
    match arguments {
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
        [command, about, web_licenses, node_modules, output]
            if command == "web-notices"
                && [about, web_licenses, node_modules, output]
                    .iter()
                    .all(|argument| !argument.starts_with('-')) =>
        {
            web_notices(
                Path::new(about),
                Path::new(web_licenses),
                Path::new(node_modules),
                Path::new(output),
                &mut stdout,
            )
        }
        [command, web_licenses, node_modules]
            if command == "stale-web-licenses"
                && [web_licenses, node_modules]
                    .iter()
                    .all(|argument| !argument.starts_with('-')) =>
        {
            stale_web_licenses(
                Path::new(web_licenses),
                Path::new(node_modules),
                &mut stdout,
            )
        }
        [option] if option == "--help" || option == "-h" => {
            writeln!(stdout, "{USAGE}").map(|()| ExitCode::SUCCESS)
        }
        _ => writeln!(io::stderr().lock(), "{USAGE}").map(|()| ExitCode::from(USAGE_ERROR)),
    }
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
        Ok(Some(changed)) => changed,
        Ok(None) => {
            writeln!(
                out,
                "Nothing to compare yet: `{base}` isn't a commit and the repository has no \
                 remote branches. Once a branch is pushed, the changes are compared with `{base}`."
            )?;
            return Ok(ExitCode::SUCCESS);
        }
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

/// Writes the license notices of the npm packages installed under `node_modules` to `output`, or,
/// when the licenses in the `about.toml` at `about`, with those the `web-licenses.toml` at
/// `web_licenses` accepts for each package, don't accept every package's license, writes each
/// such package to `out` with what to do and writes no `output`. It first removes an `output`
/// from an earlier run.
fn web_notices(
    about: &Path,
    web_licenses: &Path,
    node_modules: &Path,
    output: &Path,
    out: &mut impl Write,
) -> io::Result<ExitCode> {
    let prefix = error_prefix();
    if let Err(error) = fs::remove_file(output)
        && error.kind() != io::ErrorKind::NotFound
    {
        writeln!(
            out,
            "{prefix}couldn't remove {} from an earlier run: {error}",
            output.display()
        )?;
        return Ok(ExitCode::from(USAGE_ERROR));
    }
    let loaded = AcceptedLicenses::read(about).and_then(|accepted| {
        Ok((
            accepted,
            WebLicenses::read(web_licenses)?,
            WebNotices::collect(node_modules)?,
        ))
    });
    let (accepted, per_package, notices) = match loaded {
        Ok(loaded) => loaded,
        Err(error) => {
            writeln!(out, "{prefix}{error}")?;
            return Ok(ExitCode::from(USAGE_ERROR));
        }
    };
    let about = about.display();
    let web_licenses = web_licenses.display();
    for package in notices.unidentified() {
        writeln!(
            out,
            "{prefix}{} {}, so its license can't be checked. Drop the package, or ask the trellis \
             owner to support this layout if an installer wrote it.",
            package.path().display(),
            package.problem()
        )?;
    }
    let mut refused = notices.unidentified().len();
    for package in notices.packages() {
        let accepted = per_package.accepted_for(package.name(), &accepted);
        let Some(Refusal { problem, fix }) = refusal(package, &accepted, &about, &web_licenses)
        else {
            continue;
        };
        refused += 1;
        let name = package.name();
        let version = package.version();
        let directory = package.directory().display();
        writeln!(
            out,
            "{prefix}{name} {version} in {directory} {problem}. {fix}"
        )?;
    }
    let node_modules = node_modules.display();
    let output_name = output.display();
    let total = notices.packages().len() + notices.unidentified().len();
    if refused > 0 {
        writeln!(
            out,
            "{refused} of {total} npm package(s) under {node_modules} can't be accepted under \
             {about} and {web_licenses}, so {output_name} wasn't written."
        )?;
        return Ok(ExitCode::FAILURE);
    }
    if let Err(error) = fs::write(output, notices.to_string()) {
        writeln!(out, "{prefix}couldn't write {output_name}: {error}")?;
        return Ok(ExitCode::from(USAGE_ERROR));
    }
    writeln!(
        out,
        "Wrote the licenses of {total} npm package(s) under {node_modules} to {output_name}."
    )?;
    Ok(ExitCode::SUCCESS)
}

/// Why an npm package's license can't be accepted, and what to do about it.
struct Refusal {
    /// What is wrong, said after the package's name, version and directory.
    problem: String,
    /// What to do.
    fix: String,
}

/// Why `package` can't be accepted under `accepted`, the licenses accepted for it, and what to do,
/// or `None` when it is accepted. `about` and `web_licenses` name the files the licenses come
/// from.
fn refusal(
    package: &NpmPackage,
    accepted: &AcceptedLicenses,
    about: &impl Display,
    web_licenses: &impl Display,
) -> Option<Refusal> {
    let drop_it = format!("No entry in `accepted` in {about} can accept it, so drop the package.");
    let Some(license) = package.license() else {
        return Some(Refusal {
            problem: "states no license in its package.json".to_owned(),
            fix: drop_it,
        });
    };
    match accepted.judge_any(package.licenses()) {
        LicenseVerdict::Accepted => None,
        LicenseVerdict::NotAccepted => Some(Refusal {
            problem: format!("has the license `{license}`, which isn't accepted"),
            fix: format!(
                "If that license is acceptable for every package, add its SPDX id to `accepted` \
                 in {about}; if only for this one, accept it for the package in {web_licenses}; \
                 otherwise drop the package."
            ),
        }),
        LicenseVerdict::Exception => Some(Refusal {
            problem: format!("has the license `{license}`, which adds an exception"),
            fix: "trellis doesn't support `WITH` exceptions, so drop the package, or ask the trellis \
             owner to support this one."
                .to_owned(),
        }),
        LicenseVerdict::Unreadable => Some(Refusal {
            problem: format!(
                "has the license `{license}`, which isn't a strict SPDX expression: current \
                 license ids, in their exact case, joined with AND, OR and parentheses"
            ),
            fix: drop_it,
        }),
    }
}

/// Writes to `out` each entry of the `web-licenses.toml` at `web_licenses` that names no npm
/// package installed under `node_modules`, with what to do, and fails if there is one.
fn stale_web_licenses(
    web_licenses: &Path,
    node_modules: &Path,
    out: &mut impl Write,
) -> io::Result<ExitCode> {
    let prefix = error_prefix();
    let loaded = WebLicenses::read(web_licenses)
        .and_then(|per_package| Ok((per_package, WebNotices::collect(node_modules)?)));
    let (per_package, notices) = match loaded {
        Ok(loaded) => loaded,
        Err(error) => {
            writeln!(out, "{prefix}{error}")?;
            return Ok(ExitCode::from(USAGE_ERROR));
        }
    };
    let web_licenses = web_licenses.display();
    let node_modules = node_modules.display();
    let stale = per_package.stale(&notices);
    for name in &stale {
        writeln!(
            out,
            "{prefix}{web_licenses} accepts licenses for {name}, which isn't installed under \
             {node_modules}. Remove the entry in a rules pull request, unless the code pull \
             request that adds the package is still open; land that instead."
        )?;
    }
    if !stale.is_empty() {
        writeln!(
            out,
            "{} of {} entries in {web_licenses} name no installed npm package.",
            stale.len(),
            per_package.len()
        )?;
        return Ok(ExitCode::FAILURE);
    }
    writeln!(
        out,
        "Each of the {} entries in {web_licenses} names an npm package installed under \
         {node_modules}.",
        per_package.len()
    )?;
    Ok(ExitCode::SUCCESS)
}
