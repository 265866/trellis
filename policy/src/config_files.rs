//! Tool configuration files exist only where the workspace keeps them.

use crate::policy_error::PolicyError;
use crate::repository_files;
use crate::tool_config::ToolConfig;
use crate::violation::Violation;
use crate::workspace::Workspace;

/// The configuration files a trellis project keeps, all at the root.
const KEPT_FILES: [&str; 14] = [
    "rustfmt.toml",
    "clippy.toml",
    "deny.toml",
    "_typos.toml",
    ".taplo.toml",
    "dylint.toml",
    "rust-toolchain.toml",
    ".config/nextest.toml",
    "release-plz.toml",
    "renovate.json",
    "lychee.toml",
    ".cargo/config.toml",
    ".editorconfig",
    ".gitattributes",
];

/// Checks that no tool configuration file exists in the repository besides the ones it keeps at
/// the root,
/// such as a `clippy.toml` in a crate that would replace the root one for that crate, or a
/// `.rustfmt.toml` at the root that would override `rustfmt.toml`. A name matches in any case,
/// since a case-insensitive file system finds `Clippy.toml` for `clippy.toml`, but a kept file
/// keeps its exact name, which every file system finds.
///
/// It reads the files git tracks and the untracked ones `.gitignore` doesn't exclude, so a
/// directory's name can't hide a file from it; outside a git checkout it reads every file but
/// Cargo's build output.
///
/// # Errors
///
/// Fails if the repository's files can't be listed.
pub fn check(workspace: &Workspace) -> Result<Vec<Violation>, PolicyError> {
    let mut violations = Vec::new();
    for relative in repository_files::list(workspace.root())? {
        if let Some(config) = ToolConfig::of(&relative)
            && !KEPT_FILES.contains(&relative.as_str())
        {
            let explanation = message(&relative, config);
            violations.push(Violation::new(relative, explanation));
        }
    }
    Ok(violations)
}

/// What to do about the configuration file at `relative`, which `config` describes.
fn message(relative: &str, config: &ToolConfig) -> String {
    let reach = match config.directory(relative) {
        None | Some("") => "the whole workspace".to_owned(),
        Some(directory) => format!("everything under `{directory}/`"),
    };
    format!(
        "{tool} reads this file in place of, or on top of, the workspace's `{kept}` for {reach}, \
         so it can change the rules there. Delete it. To change a setting, change `{kept}` for \
         the whole workspace, in a pull request of its own.",
        tool = config.tool(),
        kept = config.workspace_file(),
    )
}
