//! The structure rules that `trellis-policy check` runs.

use crate::policy_error::PolicyError;
use crate::violation::Violation;
use crate::workspace::Workspace;
use crate::{
    agent_files, build_scripts, config_files, deploy_scripts, docs_index, docs_layout,
    internal_dependencies, internal_dependency_versions, license_files, lints_tables,
    member_manifests, member_sources, proc_macros, readme, release_changelog, restricted_crates,
    workspace_shape,
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
    /// Every member's `Cargo.toml` holds only the settings the workspace allows, and takes its
    /// version from the workspace.
    MemberManifests,
    /// Every member reads its sources only from its own directory.
    MemberSources,
    /// Every published member's directory holds the license files its `license` names, since its
    /// package holds only the files under that directory.
    LicenseFiles,
    /// Members depend directly only on the proc-macro crates in `proc_macro_crates`.
    ProcMacros,
    /// The root `AGENTS.md` is the only file of instructions for coding agents.
    AgentFiles,
    /// Every member depends only on the workspace crates its entry in `internal_dependencies`
    /// lists.
    InternalDependencies,
    /// No member's `[dependencies]` or `[dev-dependencies]` gives a path dependency on another
    /// member a `version`, which release-plz would rewrite beside the root's.
    InternalDependencyVersions,
    /// A `Documentation` section of the root `README.md`, or of `CONTRIBUTING.md`, links every
    /// page directly under `docs/`, and a page in the directory above links each page in a
    /// subdirectory.
    DocsIndex,
    /// The docs files `docs_files` lists exist, the root `README.md` has one title, the
    /// sections `readme_sections` lists and an index for its `Documentation` section, a
    /// `Documentation` section in `CONTRIBUTING.md` is an index too, and every page in `docs/`
    /// opens with its title and a paragraph.
    DocsLayout,
    /// The readme and the pages in `docs/` have what a stranger needs: the sections and license
    /// links, images that exist, alt text on the images that link a page in `docs/`, PNG files of
    /// image chunks alone and a linked gallery fail every check, and the outline's prompts, the
    /// opening paragraph and the hero of a project with a UI fail only a release.
    Readme,
    /// `release-plz.toml` exists, its main package's entry takes every other member into its
    /// changelog, and release-plz can write no changelog to a file that is code.
    ReleaseChangelog,
    /// The shell scripts of a deploy kit, under the root `deploy/` or a crate's
    /// `crates/<crate>/deploy/`, pass shellcheck.
    DeployScripts,
    /// Only the members `restricted_crates` allows reach a restricted package, directly or
    /// through others.
    RestrictedCrates,
}

impl Rule {
    /// Every rule, in the order `trellis-policy check` runs them.
    pub const ALL: [Self; 17] = [
        Self::WorkspaceShape,
        Self::LintsTables,
        Self::MemberManifests,
        Self::MemberSources,
        Self::LicenseFiles,
        Self::BuildScripts,
        Self::ProcMacros,
        Self::InternalDependencies,
        Self::InternalDependencyVersions,
        Self::RestrictedCrates,
        Self::ConfigFiles,
        Self::AgentFiles,
        Self::DocsIndex,
        Self::DocsLayout,
        Self::Readme,
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
            Self::LicenseFiles => "license_files",
            Self::ProcMacros => "proc_macros",
            Self::AgentFiles => "agent_files",
            Self::InternalDependencies => "internal_dependencies",
            Self::InternalDependencyVersions => "internal_dependency_versions",
            Self::DocsIndex => "docs_index",
            Self::DocsLayout => "docs_layout",
            Self::Readme => "readme",
            Self::ReleaseChangelog => "release_changelog",
            Self::DeployScripts => "deploy_scripts",
            Self::RestrictedCrates => "restricted_crates",
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
            Self::LicenseFiles => license_files::check(workspace),
            Self::ProcMacros => Ok(proc_macros::check(workspace)),
            Self::AgentFiles => agent_files::check(workspace),
            Self::InternalDependencies => Ok(internal_dependencies::check(workspace)),
            Self::InternalDependencyVersions => Ok(internal_dependency_versions::check(workspace)),
            Self::DocsIndex => docs_index::check(workspace),
            Self::DocsLayout => docs_layout::check(workspace),
            Self::Readme => readme::check(workspace),
            Self::ReleaseChangelog => release_changelog::check(workspace),
            Self::DeployScripts => deploy_scripts::check(workspace),
            Self::RestrictedCrates => Ok(restricted_crates::check(workspace)),
        }
    }
}
