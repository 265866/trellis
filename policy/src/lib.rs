//! Checks the structure rules of a project generated from the trellis template that the compiler
//! and the lints can't see: every crate inherits the workspace lints, holds only the manifest
//! settings the workspace allows, reads its sources only from its own directory, has no build
//! script and uses only the proc macros the workspace lists; no tool reads a configuration file
//! other than the workspace's; the root `AGENTS.md` is the only file of instructions for coding
//! agents; each crate depends only on the workspace crates its entry in `internal_dependencies`
//! allows; the workspace keeps its shape and loads the trellis lints; the root `README.md` links
//! every page in `docs/`; and the main package's changelog in `release-plz.toml` takes in every
//! other crate.
//!
//! The `trellis-policy check [--manifest-path <Cargo.toml>]` command runs every [`Rule`] on a
//! workspace. The `trellis-policy rule-changes <base> [--head <commit>]` command keeps changes to
//! the rules and changes to the code they check in separate pull requests.

pub mod agent_files;
pub mod build_scripts;
mod case_insensitive;
mod changed_files;
pub mod config_files;
pub mod docs_index;
mod git;
mod head;
pub mod internal_dependencies;
mod lint_expectations;
pub mod lints_tables;
pub mod member_manifests;
pub mod member_sources;
mod policy_config;
mod policy_error;
pub mod proc_macros;
pub mod release_changelog;
mod repository_files;
mod rule;
mod source_path;
mod token_context;
mod toml_file;
mod tool_config;
mod upstream;
mod violation;
mod workspace;
pub mod workspace_shape;

pub use changed_files::ChangedFiles;
pub use head::Head;
pub use policy_error::PolicyError;
pub use rule::Rule;
pub use violation::Violation;
pub use workspace::Workspace;
