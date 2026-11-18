//! A rule that a file in the repository breaks.

use std::fmt::{self, Display, Formatter};

/// A rule that a file in the repository breaks, with what to do about it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Violation {
    /// The file, relative to the workspace root and with `/` separators.
    file: String,
    /// What is wrong, and how to fix it.
    message: String,
}

impl Violation {
    /// A violation of a rule by `file`, which `message` explains.
    pub(crate) fn new(file: String, message: String) -> Self {
        Self { file, message }
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
}

impl Display for Violation {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.file, self.message)
    }
}
