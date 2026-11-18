//! Where trellis lives, for the messages that say where a rule can change.

/// The trellis repository, which holds the template, the lints and these checks.
pub(crate) const REPOSITORY: &str = "https://github.com/265866/trellis";

/// What to do when a project needs a setting that the check in `source`, a file of trellis such
/// as `policy/src/member_manifests.rs`, doesn't allow.
pub(crate) fn change_the_rule(source: &str) -> String {
    format!(
        "If the project can't do without it, allow it in `{source}` of trellis ({REPOSITORY}) and \
         move the project to that revision, in a pull request of its own that says why."
    )
}
