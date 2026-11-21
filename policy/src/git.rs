//! Running git.

use std::path::Path;
use std::process::Command;

use crate::policy_error::PolicyError;

/// Runs git in `repository` with `arguments` and returns its output.
///
/// # Errors
///
/// Fails if git doesn't run, exits with an error, or writes output that isn't UTF-8.
pub(crate) fn run(repository: &Path, arguments: &[&str]) -> Result<String, PolicyError> {
    let command = format!("git {}", arguments.join(" "));
    let output = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(arguments)
        .output()
        .map_err(|error| PolicyError::Git {
            command: command.clone(),
            message: error.to_string(),
        })?;
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
