//! Checks the structure rules of a trellis project that the compiler and the lints can't see.
//! [The style guide](https://github.com/265866/trellis/blob/main/template/docs/style.md#checked-by-tools)
//! lists them under Checked by tools and Docs checked by just policy.
//!
//! The `trellis-policy check [--manifest-path <Cargo.toml>]` command runs every [`Rule`] on a
//! workspace. The `trellis-policy rule-changes <base> [--head <commit>]` command keeps changes to
//! the rules and changes to the code they check in separate pull requests. The
//! `trellis-policy web-notices <about.toml> <web-licenses.toml> <node_modules> <output>` command
//! writes the license notices of the npm packages a web UI installs, as [`WebNotices`], and fails
//! when a package's license isn't one of the [`AcceptedLicenses`] or of the [`WebLicenses`]
//! accepted for that package. The `trellis-policy stale-web-licenses <web-licenses.toml>
//! <node_modules>` command fails on an entry of [`WebLicenses`] for a package that isn't
//! installed.

mod accepted_licenses;
pub mod agent_files;
pub mod build_scripts;
mod case_insensitive;
mod changed_files;
pub mod config_files;
mod deploy_scripts;
pub mod docs_index;
pub mod docs_layout;
mod git;
mod git_attributes;
mod head;
mod html;
pub mod internal_dependencies;
mod lint_expectations;
pub mod lints_tables;
mod markdown;
pub mod member_manifests;
pub mod member_sources;
mod npm_package;
mod policy_config;
mod policy_error;
pub mod proc_macros;
pub mod release_changelog;
mod repository_files;
mod restricted_crate;
pub mod restricted_crates;
mod rule;
mod source_path;
mod token_context;
mod toml_file;
mod tool_config;
mod unidentified_package;
mod upstream;
mod violation;
mod web_licenses;
mod web_notices;
mod workspace;
pub mod workspace_shape;

pub use accepted_licenses::{AcceptedLicenses, LicenseVerdict};
pub use changed_files::ChangedFiles;
pub use head::Head;
pub use npm_package::NpmPackage;
pub use policy_error::PolicyError;
pub use rule::Rule;
pub use unidentified_package::UnidentifiedPackage;
pub use violation::Violation;
pub use web_licenses::WebLicenses;
pub use web_notices::WebNotices;
pub use workspace::Workspace;
