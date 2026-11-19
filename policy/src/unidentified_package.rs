//! A place under `node_modules` where an installer puts a package, holding something the check
//! can't read as one.

use std::path::{Path, PathBuf};

/// A directory or link where an installer puts a package, such as `node_modules/<name>`, whose
/// package the check can't name, so its license can't be checked.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct UnidentifiedPackage {
    /// The directory or link.
    path: PathBuf,
    /// What is wrong, worded to follow the path in a sentence, such as `holds files but no
    /// package.json`.
    problem: String,
}

impl UnidentifiedPackage {
    /// The package at `path`, with what is wrong with it, worded to follow the path in a sentence.
    pub(crate) fn new(path: PathBuf, problem: String) -> Self {
        Self { path, problem }
    }

    /// The directory or link where the package would be.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// What is wrong, worded to follow the path in a sentence.
    #[must_use]
    pub fn problem(&self) -> &str {
        &self.problem
    }
}
