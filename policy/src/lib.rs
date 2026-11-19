//! Checks the structure rules of a trellis project that the compiler and the lints can't see.
//! [The style guide](https://github.com/265866/trellis/blob/main/docs/style.md#checked-by-tools)
//! lists them under Checked by tools and Docs checked by just policy.
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
pub mod docs_layout;
mod git;
mod head;
mod html;
pub mod internal_dependencies;
mod lint_expectations;
pub mod lints_tables;
mod markdown;
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
