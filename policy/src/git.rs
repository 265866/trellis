//! Running git.

use std::path::Path;
use std::process::{Command, Stdio};
use std::{io, thread};

use crate::policy_error::PolicyError;

/// Runs git in `repository` with `arguments` and returns its output.
///
/// # Errors
///
/// Fails if git doesn't run, exits with an error, or writes output that isn't UTF-8.
pub(crate) fn run(repository: &Path, arguments: &[&str]) -> Result<String, PolicyError> {
    run_with_input(repository, arguments, None)
}

/// Runs git in `repository` with `arguments` and `input`, if any, on its standard input, and
/// returns its output.
///
/// # Errors
///
/// Fails if git doesn't run, exits with an error, or writes output that isn't UTF-8.
pub(crate) fn run_with_input(
    repository: &Path,
    arguments: &[&str],
    input: Option<&str>,
) -> Result<String, PolicyError> {
    use std::io::Write as _;

    let command = format!("git {}", arguments.join(" "));
    let failed = |error: io::Error| PolicyError::Git {
        command: command.clone(),
        message: error.to_string(),
    };
    let mut child = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(arguments)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(failed)?;
    let stdin = child.stdin.take();
    let (written, output) = thread::scope(|scope| {
        let writer = scope.spawn(move || match (input, stdin) {
            (Some(input), Some(mut stdin)) => stdin.write_all(input.as_bytes()),
            _ => Ok(()),
        });
        let output = child.wait_with_output();
        let written = writer
            .join()
            .unwrap_or_else(|_| Err(io::Error::other("writing git's input panicked")));
        (written, output)
    });
    written.map_err(failed)?;
    let output = output.map_err(failed)?;
    if !output.status.success() {
        return Err(PolicyError::Git {
            command,
            message: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    String::from_utf8(output.stdout).map_err(|error| PolicyError::Git {
        command,
        message: format!("the output isn't UTF-8: {error}"),
    })
}

/// The mode git gives a submodule's entry, a gitlink: a commit of another repository, whose files
/// the tree doesn't hold.
pub(crate) const GITLINK_MODE: &str = "160000";

/// The paths in `output`, git output written with `-z`, without the empty one after the last
/// separator.
pub(crate) fn paths(output: &str) -> impl Iterator<Item = &str> {
    output.split('\0').filter(|path| !path.is_empty())
}
