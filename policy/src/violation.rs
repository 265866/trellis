//! A rule that a file in the repository breaks.

use std::fmt::{self, Display, Formatter};

/// A rule that a file in the repository breaks, with what to do about it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Violation {
    /// The file, relative to the workspace root and with `/` separators.
    file: String,
    /// What is wrong, and how to fix it.
    message: String,
    /// Which checks fail on it.
    fails: Fails,
}

/// Which checks fail on a [`Violation`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fails {
    /// Every check: the form is wrong at any age.
    Always,
    /// Only a release's check, `trellis-policy check --release`; otherwise the check prints it as
    /// a notice. It is something a project grows into, such as its written readme.
    Release,
    /// No check: the check prints it as a notice, about something it can't check, such as a file
    /// CI doesn't fetch.
    Never,
}

impl Violation {
    /// A violation of a rule by `file`, which `message` explains, that fails every check.
    pub(crate) fn new(file: String, message: String) -> Self {
        Self::failing(file, message, Fails::Always)
    }

    /// A violation by `file`, which `message` explains, that only a release fails on.
    pub(crate) fn release_only(file: String, message: String) -> Self {
        Self::failing(file, message, Fails::Release)
    }

    /// A notice about `file`, which `message` explains, that no check fails on.
    pub(crate) fn notice(file: String, message: String) -> Self {
        Self::failing(file, message, Fails::Never)
    }

    /// A violation by `file`, which `message` explains, that the checks `fails` says fail on.
    fn failing(file: String, message: String, fails: Fails) -> Self {
        Self {
            file,
            message,
            fails,
        }
    }

    /// The file that breaks the rule, relative to the workspace root and with `/` separators.
    #[must_use]
    pub fn file(&self) -> &str {
        &self.file
    }

    /// What is wrong, and how to fix it.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Which checks fail on it.
    #[must_use]
    pub fn fails(&self) -> Fails {
        self.fails
    }
}

impl Display for Violation {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.file, self.message)
    }
}
