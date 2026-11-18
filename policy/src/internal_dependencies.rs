//! Each workspace crate depends only on the workspace crates the policy settings allow it.

use std::collections::HashSet;

use cargo_metadata::{DependencyKind, PackageId};

use crate::violation::Violation;
use crate::workspace::{Workspace, WorkspaceMember};

/// Checks that each member is a key of `internal_dependencies` in `[workspace.metadata.policy]`
/// of the root `Cargo.toml`, and depends directly, as a normal, dev or build dependency on any
/// platform and with every feature on, only on the members its entry lists. The map sets which
/// layer each crate is in, so a new crate or a new edge between crates is a rule change, made in
/// a pull request of its own that says why.
///
/// A crate in `dev_only_crates`, such as the test-support crate, is the one exception: any
/// member may depend on it, but only as a dev-dependency, so it never reaches a crate's library
/// or binaries.
///
/// Only actual members and their edges are checked, so the map can name crates that don't exist
/// yet.
#[must_use]
pub fn check(workspace: &Workspace) -> Vec<Violation> {
    let metadata = workspace.metadata();
    let Some(resolve) = &metadata.resolve else {
        return vec![Violation::new(
            "Cargo.toml".to_owned(),
            "`cargo metadata` returned no dependency graph to check.".to_owned(),
        )];
    };
    let config = workspace.config();
    let member_ids: HashSet<&PackageId> = workspace
        .members()
        .iter()
        .map(WorkspaceMember::id)
        .collect();
    let mut violations = Vec::new();
    for member in workspace.members() {
        let file = workspace.relative(member.manifest_path());
        let member_name = member.name();
        let Some(allowed) = config.internal_dependencies(member_name) else {
            violations.push(Violation::new(
                file,
                format!(
                    "`{member_name}` is a workspace member that `internal_dependencies` in \
                     `[workspace.metadata.policy]` of the root Cargo.toml doesn't map. Add it \
                     with the workspace crates it may depend on, in a pull request of its own \
                     that says which layer it belongs to."
                ),
            ));
            continue;
        };
        let Some(node) = resolve.nodes.iter().find(|node| node.id == *member.id()) else {
            violations.push(Violation::new(
                file,
                format!(
                    "`cargo metadata` has no dependencies for `{member_name}`, so the check can't \
                     see which workspace crates it depends on."
                ),
            ));
            continue;
        };
        for dependency in &node.deps {
            if !member_ids.contains(&dependency.pkg) {
                continue;
            }
            let name = metadata[&dependency.pkg].name.as_str();
            if allowed.iter().any(|allowed| allowed == name) {
                continue;
            }
            let message = if config.is_dev_only(name) {
                let only_dev = !dependency.dep_kinds.is_empty()
                    && dependency
                        .dep_kinds
                        .iter()
                        .all(|info| info.kind == DependencyKind::Development);
                if only_dev {
                    continue;
                }
                format!(
                    "`{member_name}` depends on the test-support crate `{name}` as more than a \
                     dev-dependency. `dev_only_crates` in `[workspace.metadata.policy]` lists \
                     it, so only tests may use it; keep it only in `[dev-dependencies]`."
                )
            } else {
                format!(
                    "`{member_name}` depends on the workspace crate `{name}`, which \
                     `internal_dependencies` in `[workspace.metadata.policy]` of the root \
                     Cargo.toml doesn't let it depend on, as a normal, dev or build dependency. \
                     Move the code `{member_name}` needs from `{name}` into a crate \
                     `{member_name}` may use, or, if the layering should change, add `{name}` to \
                     the entry of `{member_name}` in a pull request of its own that says why."
                )
            };
            violations.push(Violation::new(file.clone(), message));
        }
    }
    violations
}
