//! Why a check couldn't read the repository.

use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::io;
use std::path::PathBuf;

use toml::de;

/// Why a check couldn't read the repository or its history. The checks fail closed: an error
/// fails the run rather than skipping a rule.
#[derive(Debug)]
#[must_use]
pub enum PolicyError {
    /// `cargo metadata` failed.
    Metadata(cargo_metadata::Error),
    /// A file or directory couldn't be read.
    Read {
        /// The file or directory.
        path: PathBuf,
        /// Why reading it failed.
        source: io::Error,
    },
    /// A TOML file didn't parse.
    Parse {
        /// The file.
        path: PathBuf,
        /// Why it didn't parse.
        source: de::Error,
    },
    /// The `[workspace.metadata.policy]` table of the root `Cargo.toml` is malformed.
    Config(String),
    /// A `git` command failed.
    Git {
        /// The command, as `git` and its arguments.
        command: String,
        /// What went wrong: git's error output, or why git didn't run.
        message: String,
    },
}

impl Display for PolicyError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Metadata(error) => write!(formatter, "`cargo metadata` failed: {error}"),
            Self::Read { path, source } => {
                write!(formatter, "couldn't read {}: {source}", path.display())
            }
            Self::Parse { path, source } => {
                write!(formatter, "{} isn't valid TOML: {source}", path.display())
            }
            Self::Config(message) => write!(
                formatter,
                "the [workspace.metadata.policy] table of Cargo.toml is malformed: {message}"
            ),
            Self::Git { command, message } => write!(formatter, "`{command}` failed: {message}"),
        }
    }
}

impl Error for PolicyError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Metadata(error) => Some(error),
            Self::Read { source, .. } => Some(source),
            Self::Parse { source, .. } => Some(source),
            Self::Config(_) | Self::Git { .. } => None,
        }
    }
}
