//! The workspace the checks read.

use std::path::{Component, Path, PathBuf};

use cargo_metadata::{CargoOpt, Metadata, MetadataCommand, PackageId};
use toml::Table;

use crate::policy_config::PolicyConfig;
use crate::policy_error::PolicyError;
use crate::toml_file;

/// The workspace the checks read: what `cargo metadata` says about it, with every feature on, and
/// the parsed manifests of the workspace and of each member.
#[derive(Debug)]
pub struct Workspace {
    /// The directory of the root `Cargo.toml`.
    root: PathBuf,
    /// The root `Cargo.toml`.
    manifest: Table,
    /// The `[workspace.metadata.policy]` settings.
    config: PolicyConfig,
    /// The members, in the order `cargo metadata` lists them.
    members: Vec<WorkspaceMember>,
    /// The output of `cargo metadata`, with the resolved dependency graph.
    metadata: Metadata,
}

impl Workspace {
    /// Loads the workspace that holds the manifest `manifest_path`, the root `Cargo.toml` or a
    /// member's, or, without one, the workspace that Cargo finds from the current directory.
    ///
    /// # Errors
    ///
    /// Fails if `cargo metadata` fails, which it does when `Cargo.lock` is out of date, if a
    /// manifest can't be read or parsed, or if the `[workspace.metadata.policy]` table is
    /// malformed.
    pub fn load(manifest_path: Option<&Path>) -> Result<Self, PolicyError> {
        let mut command = MetadataCommand::new();
        if let Some(manifest_path) = manifest_path {
            command.manifest_path(manifest_path);
        }
        let metadata = command
            .features(CargoOpt::AllFeatures)
            .other_options(vec!["--locked".to_owned()])
            .exec()
            .map_err(PolicyError::Metadata)?;
        let root = metadata.workspace_root.clone().into_std_path_buf();
        let manifest = toml_file::read(&root.join("Cargo.toml"))?;
        let config = PolicyConfig::from_manifest(&manifest)?;
        let mut members = Vec::new();
        for package in metadata.workspace_packages() {
            let manifest_path = package.manifest_path.clone().into_std_path_buf();
            members.push(WorkspaceMember {
                name: package.name.to_string(),
                id: package.id.clone(),
                manifest: toml_file::read(&manifest_path)?,
                manifest_path,
            });
        }
        Ok(Self {
            root,
            manifest,
            config,
            members,
            metadata,
        })
    }

    /// The directory of the root `Cargo.toml`.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The root `Cargo.toml`.
    pub(crate) fn manifest(&self) -> &Table {
        &self.manifest
    }

    /// The `[workspace.metadata.policy]` settings.
    pub(crate) fn config(&self) -> &PolicyConfig {
        &self.config
    }

    /// The members.
    pub(crate) fn members(&self) -> &[WorkspaceMember] {
        &self.members
    }

    /// The output of `cargo metadata`.
    pub(crate) fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    /// `path` relative to the workspace root, with `/` separators, as violations name files. A
    /// path outside the root is written in full.
    pub(crate) fn relative(&self, path: &Path) -> String {
        let Ok(relative) = path.strip_prefix(&self.root) else {
            return path.display().to_string();
        };
        let mut parts = Vec::new();
        for component in relative.components() {
            if let Component::Normal(part) = component {
                parts.push(part.to_string_lossy());
            }
        }
        parts.join("/")
    }
}

/// A member of the workspace.
#[derive(Debug)]
pub(crate) struct WorkspaceMember {
    /// The package name.
    name: String,
    /// The package's id in `cargo metadata`.
    id: PackageId,
    /// The path of its `Cargo.toml`.
    manifest_path: PathBuf,
    /// Its parsed `Cargo.toml`.
    manifest: Table,
}

impl WorkspaceMember {
    /// The package name.
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    /// The package's id in `cargo metadata`.
    pub(crate) fn id(&self) -> &PackageId {
        &self.id
    }

    /// The path of its `Cargo.toml`.
    pub(crate) fn manifest_path(&self) -> &Path {
        &self.manifest_path
    }

    /// The directory of its `Cargo.toml`.
    pub(crate) fn dir(&self) -> &Path {
        self.manifest_path.parent().unwrap_or(&self.manifest_path)
    }

    /// Its parsed `Cargo.toml`.
    pub(crate) fn manifest(&self) -> &Table {
        &self.manifest
    }
}
