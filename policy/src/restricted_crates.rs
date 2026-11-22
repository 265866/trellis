//! Only the members `restricted_crates` allows reach a restricted package, directly or through
//! other packages.

use std::collections::{HashMap, HashSet, VecDeque};

use cargo_metadata::{DependencyKind, Node, PackageId};

use crate::restricted_crate::RestrictedCrate;
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
/// The graph is the one `cargo metadata` resolves: every platform's dependencies, with every
/// feature of every member on and each package's features unified across the workspace. So a
/// member that uses a package without the feature that brings a restricted one still fails when
/// another member turns that feature on. The check errs toward failing; the remedy is to move the
/// code behind that feature into a crate of its own.
///
/// A key that matches no package in that graph fails too, since it is misspelled or stale and
/// would restrict nothing.
///
/// A project without `restricted_crates` restricts nothing.
#[must_use]
pub fn check(workspace: &Workspace) -> Vec<Violation> {
    let restricted = workspace.config().restricted_crates();
    if restricted.is_empty() {
        return Vec::new();
    }
    let metadata = workspace.metadata();
    let mut matching: HashMap<&PackageId, Vec<Match<'_>>> = HashMap::new();
    for package in &metadata.packages {
        let found: Vec<Match<'_>> = restricted
            .iter()
            .filter(|(_, entry)| entry.names(package))
            .map(|(key, entry)| Match { key, entry })
            .collect();
        if !found.is_empty() {
            matching.insert(&package.id, found);
        }
    }
    let mut violations: Vec<Violation> = restricted
        .keys()
        .filter(|key| !matching.values().flatten().any(|found| found.key == *key))
        .map(|key| {
            Violation::new(
                "Cargo.toml".to_owned(),
                format!(
                    "`restricted_crates` has a key `{key}` that matches no package in the \
                     dependency graph, on any platform or with any feature. Fix the spelling, or \
                     remove the key."
                ),
            )
        })
        .collect();
    let Some(resolve) = &metadata.resolve else {
        violations.push(Violation::new(
            "Cargo.toml".to_owned(),
            "`cargo metadata` returned no dependency graph to check.".to_owned(),
        ));
        return violations;
    };
    let nodes: HashMap<&PackageId, &Node> =
        resolve.nodes.iter().map(|node| (&node.id, node)).collect();
    let entries = |id: &PackageId| matching.get(id).map_or(&[][..], Vec::as_slice);
    for member in workspace.members() {
        let forbidden = |id: &PackageId| {
            id != member.id()
                && entries(id)
                    .iter()
                    .any(|found| !found.entry.allows(member.name()))
        };
        for path in forbidden_paths(member.id(), &nodes, forbidden) {
            let Some(reached) = path.last() else {
                continue;
            };
            let found = entries(reached);
            let keys: Vec<String> = found
                .iter()
                .filter(|found| !found.entry.allows(member.name()))
                .map(|found| format!("`{}`", found.key))
                .collect();
            let through = match keys.as_slice() {
                [key] => format!("its key {key}"),
                _ => format!("its keys {}", keys.join(", ")),
            };
            let allowed: Vec<String> = found
                .first()
                .map_or(&[][..], |first| first.entry.allowed())
                .iter()
                .filter(|name| found.iter().all(|other| other.entry.allows(name)))
                .map(|name| format!("`{name}`"))
                .collect();
            let names: Vec<&str> = path.iter().map(|id| metadata[*id].name.as_str()).collect();
            let route = format!(
                "`{}` reaches `{}` ({})",
                member.name(),
                metadata[*reached].name,
                names.join(" -> ")
            );
            let message = if allowed.is_empty() {
                format!(
                    "{route}, which `restricted_crates` lets no member reach, through {through}. \
                     Drop the dependency that brings it."
                )
            } else {
                format!(
                    "{route}, which `restricted_crates` lets only {} reach, through {through}. \
                     Move the code that needs it into one of those crates, or drop the dependency \
                     that brings it.",
                    allowed.join(", ")
                )
            };
            violations.push(Violation::new(
                workspace.relative(member.manifest_path()),
                message,
            ));
        }
    }
    violations
}

/// A `restricted_crates` entry that matches a package.
struct Match<'config> {
    /// The entry's key.
    key: &'config String,
    /// The entry.
    entry: &'config RestrictedCrate,
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
