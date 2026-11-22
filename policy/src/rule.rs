//! The structure rules that `trellis-policy check` runs.

use crate::policy_error::PolicyError;
use crate::violation::Violation;
use crate::workspace::Workspace;
use crate::{
    agent_files, build_scripts, config_files, deploy_scripts, docs_index, docs_layout,
    internal_dependencies, lints_tables, member_manifests, member_sources, proc_macros,
    release_changelog, workspace_shape,
};

/// A structure rule of the workspace, which one check enforces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rule {
    /// Every member's `[lints]` table is exactly `workspace = true`.
    LintsTables,
    /// Only the crates in `build_script_crates` have a build script.
    BuildScripts,
    /// Tool configuration files exist only where the workspace keeps them, each
    /// `tsconfig.json` reaches only other `tsconfig.json` files, and the root `.gitattributes`
    /// sets only how git converts line endings.
    ConfigFiles,
    /// The workspace keeps its exclusions, members, custom lints and release profile.
    WorkspaceShape,
    /// Every member's `Cargo.toml` holds only the settings the workspace allows.
    MemberManifests,
    /// Every member reads its sources only from its own directory.
    MemberSources,
    /// Members depend directly only on the proc-macro crates in `proc_macro_crates`.
    ProcMacros,
    /// The root `AGENTS.md` is the only file of instructions for coding agents.
    AgentFiles,
    /// Every member depends only on the workspace crates its entry in `internal_dependencies`
    /// lists.
    InternalDependencies,
    /// The `Documentation` section of the root `README.md` links every page directly under
    /// `docs/`, and a page in the directory above links each page in a subdirectory.
    DocsIndex,
    /// The docs files `docs_files` lists exist, the root `README.md` has one title, the
    /// sections `readme_sections` lists and an index for its `Documentation` section, and every
    /// page in `docs/` opens with its title and a paragraph.
    DocsLayout,
    /// The main package's entry in `release-plz.toml` takes every other member into its
    /// changelog.
    ReleaseChangelog,
    /// The shell scripts under `deploy/` pass shellcheck.
    DeployScripts,
}

impl Rule {
    /// Every rule, in the order `trellis-policy check` runs them.
    pub const ALL: [Self; 13] = [
        Self::WorkspaceShape,
        Self::LintsTables,
        Self::MemberManifests,
        Self::MemberSources,
        Self::BuildScripts,
        Self::ProcMacros,
        Self::InternalDependencies,
        Self::ConfigFiles,
        Self::AgentFiles,
        Self::DocsIndex,
        Self::DocsLayout,
        Self::ReleaseChangelog,
        Self::DeployScripts,
    ];

    /// The rule's name, as `trellis-policy check` prints it: the name of its module.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::LintsTables => "lints_tables",
            Self::BuildScripts => "build_scripts",
            Self::ConfigFiles => "config_files",
            Self::WorkspaceShape => "workspace_shape",
            Self::MemberManifests => "member_manifests",
            Self::MemberSources => "member_sources",
            Self::ProcMacros => "proc_macros",
            Self::AgentFiles => "agent_files",
            Self::InternalDependencies => "internal_dependencies",
            Self::DocsIndex => "docs_index",
            Self::DocsLayout => "docs_layout",
            Self::ReleaseChangelog => "release_changelog",
            Self::DeployScripts => "deploy_scripts",
        }
    }

    /// Checks `workspace` against the rule.
    ///
    /// # Errors
    ///
    /// Fails if the check can't read a file it needs, as each module's `check` says.
    pub fn check(self, workspace: &Workspace) -> Result<Vec<Violation>, PolicyError> {
        match self {
            Self::LintsTables => Ok(lints_tables::check(workspace)),
            Self::BuildScripts => build_scripts::check(workspace),
            Self::ConfigFiles => config_files::check(workspace),
            Self::WorkspaceShape => workspace_shape::check(workspace),
            Self::MemberManifests => Ok(member_manifests::check(workspace)),
            Self::MemberSources => member_sources::check(workspace),
            Self::ProcMacros => Ok(proc_macros::check(workspace)),
            Self::AgentFiles => agent_files::check(workspace),
            Self::InternalDependencies => Ok(internal_dependencies::check(workspace)),
            Self::DocsIndex => docs_index::check(workspace),
            Self::DocsLayout => docs_layout::check(workspace),
            Self::ReleaseChangelog => release_changelog::check(workspace),
            Self::DeployScripts => deploy_scripts::check(workspace),
        }
    }
}
