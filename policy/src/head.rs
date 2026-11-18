//! Where the changes `trellis-policy rule-changes` judges end.

/// Where the changes that [`ChangedFiles`](crate::ChangedFiles) reads end.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Head {
    /// The working tree: the commits on `HEAD`, and the staged, unstaged and untracked files that
    /// `.gitignore` doesn't exclude, so a check before committing sees what is about to be
    /// committed.
    WorkingTree,
    /// A commit, such as a pull request's head, which needn't be checked out.
    Commit(String),
}
