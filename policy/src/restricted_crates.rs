//! Only the members `restricted_crates` allows reach a restricted package, directly or through
//! other packages.

use std::collections::{HashMap, HashSet, VecDeque};

use cargo_metadata::{DependencyKind, Node, PackageId};

use crate::violation::Violation;
use crate::workspace::Workspace;

/// Checks that only the members each `restricted_crates` entry lists depend on a package its key
/// names, as a normal, dev or build dependency or through any chain of other packages, workspace
/// members included. Past the member itself only normal and build dependencies count, since a
/// dependency's own dev-dependencies aren't built for it. So a crate that must stay buildable
/// without a UI toolkit, such as a protocol crate, can't reach it through a new dependency that
/// brings it along, which neither `internal_dependencies` nor cargo-deny's `wrappers`, which reads
/// direct dependents only, would see.
///
/// A project without `restricted_crates` restricts nothing.
#[must_use]
pub fn check(workspace: &Workspace) -> Vec<Violation> {
    let restricted = workspace.config().restricted_crates();
    if restricted.is_empty() {
        return Vec::new();
    }
    let metadata = workspace.metadata();
    let Some(resolve) = &metadata.resolve else {
        return vec![Violation::new(
            "Cargo.toml".to_owned(),
            "`cargo metadata` returned no dependency graph to check.".to_owned(),
        )];
    };
    let nodes: HashMap<&PackageId, &Node> =
        resolve.nodes.iter().map(|node| (&node.id, node)).collect();
    let mut violations = Vec::new();
    for member in workspace.members() {
        for (key, entry) in restricted {
            let allowed = entry.allowed();
            if allowed.iter().any(|name| name == member.name()) {
                continue;
            }
            let forbidden = |id: &PackageId| id != member.id() && entry.names(metadata, id);
            for path in forbidden_paths(member.id(), &nodes, forbidden) {
                let names: Vec<&str> = path.iter().map(|id| metadata[*id].name.as_str()).collect();
                let reached = names.last().copied().unwrap_or_default();
                let allowed = if allowed.is_empty() {
                    "no member".to_owned()
                } else {
                    allowed
                        .iter()
                        .map(|name| format!("`{name}`"))
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                violations.push(Violation::new(
                    workspace.relative(member.manifest_path()),
                    format!(
                        "`{}` reaches `{reached}` ({}), which `restricted_crates` lets only {allowed} \
                         reach, through its key `{key}`. Move the code that needs it into one of \
                         those crates, or drop the dependency that brings it.",
                        member.name(),
                        names.join(" -> ")
                    ),
                ));
            }
        }
    }
    violations
}

/// The shortest dependency path from `start` to each package `forbidden` matches, without going
/// on past one. Every dependency of `start` counts; past it, only normal and build dependencies
/// do.
fn forbidden_paths<'graph>(
    start: &'graph PackageId,
    nodes: &HashMap<&'graph PackageId, &'graph Node>,
    forbidden: impl Fn(&PackageId) -> bool,
) -> Vec<Vec<&'graph PackageId>> {
    let mut parents: HashMap<&PackageId, &PackageId> = HashMap::new();
    let mut seen: HashSet<&PackageId> = HashSet::from([start]);
    let mut queue = VecDeque::from([start]);
    let mut paths = Vec::new();
    while let Some(id) = queue.pop_front() {
        let Some(node) = nodes.get(id) else {
            continue;
        };
        for dependency in &node.deps {
            let dev_only = !dependency.dep_kinds.is_empty()
                && dependency
                    .dep_kinds
                    .iter()
                    .all(|info| info.kind == DependencyKind::Development);
            if (id != start && dev_only) || !seen.insert(&dependency.pkg) {
                continue;
            }
            parents.insert(&dependency.pkg, id);
            if forbidden(&dependency.pkg) {
                paths.push(path_to(&dependency.pkg, &parents));
            } else {
                queue.push_back(&dependency.pkg);
            }
        }
    }
    paths
}

/// The path from the start of the search to `end`, following `parents`.
fn path_to<'graph>(
    end: &'graph PackageId,
    parents: &HashMap<&'graph PackageId, &'graph PackageId>,
) -> Vec<&'graph PackageId> {
    let mut path = vec![end];
    let mut current = end;
    while let Some(parent) = parents.get(current) {
        path.push(parent);
        current = parent;
    }
    path.reverse();
    path
}
