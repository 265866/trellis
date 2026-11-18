//! Members depend directly only on the proc-macro crates the policy settings list.

use cargo_metadata::Target;

use crate::violation::Violation;
use crate::workspace::Workspace;

/// Checks that each package a member depends on directly, as a normal, dev or build dependency
/// on any platform and with every feature on, is a proc-macro crate only if `proc_macro_crates`
/// in `[workspace.metadata.policy]` of the root `Cargo.toml` lists it. A proc macro writes code
/// into the crate that the custom lints don't check, so each one is a rule change, listed in a
/// pull request of its own that says why.
///
/// The list may name crates no member uses, unlike `build_script_crates`. The list entry is a
/// rule, but a member's dependency on the proc macro is code, and `cargo shear` rejects a
/// dependency nothing uses yet, so the dependency lands with its first use. A check for stale
/// entries made adding a direct proc-macro dependency impossible: the entry can't land first
/// without the dependency, and the dependency can't land with the entry. So a rules pull request
/// adds the entry, then a code pull request adds the dependency and its first use; removing one
/// goes the other way. A proc-macro dependency the list doesn't name still fails.
#[must_use]
pub fn check(workspace: &Workspace) -> Vec<Violation> {
    let metadata = workspace.metadata();
    let Some(resolve) = &metadata.resolve else {
        return vec![Violation::new(
            "Cargo.toml".to_owned(),
            "`cargo metadata` returned no dependency graph to check.".to_owned(),
        )];
    };
    let allowed = workspace.config().proc_macro_crates();
    let mut violations = Vec::new();
    for member in workspace.members() {
        let file = workspace.relative(member.manifest_path());
        let Some(node) = resolve.nodes.iter().find(|node| node.id == *member.id()) else {
            violations.push(Violation::new(
                file,
                format!(
                    "`cargo metadata` has no dependencies for `{}`, so the check can't see its \
                     proc macros.",
                    member.name()
                ),
            ));
            continue;
        };
        for dependency in &node.deps {
            let package = &metadata[&dependency.pkg];
            if !package.targets.iter().any(Target::is_proc_macro) {
                continue;
            }
            let name = package.name.as_str();
            if !allowed.iter().any(|allowed| allowed == name) {
                violations.push(Violation::new(
                    file.clone(),
                    format!(
                        "`{}` depends directly on the proc-macro crate `{name}`, which \
                         `proc_macro_crates` in `[workspace.metadata.policy]` of the root \
                         Cargo.toml doesn't list. A proc macro writes code into the crate that \
                         the custom lints don't check. Remove the dependency, or, if the crate \
                         needs it, add `{name}` to `proc_macro_crates` in a pull request of its \
                         own that says why.",
                        member.name()
                    ),
                ));
            }
        }
    }
    violations
}
