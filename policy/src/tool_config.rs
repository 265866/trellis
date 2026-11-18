//! The configuration file names that check tools read.

use crate::case_insensitive;

/// A configuration file name a check tool reads, and the file the workspace keeps for that tool.
#[derive(Debug)]
pub(crate) struct ToolConfig {
    /// The file name, or a two-part name such as Cargo's `.cargo/config.toml`.
    name: &'static str,
    /// The tool that reads it.
    tool: &'static str,
    /// The workspace's own file for the tool, relative to the root.
    workspace_file: &'static str,
}

/// Every configuration file name that could change how a check, an editor, git or the release and
/// update bots treat the project. Most of these tools look for their file in the directory they
/// run in and in its ancestors, so a copy anywhere in the tree can replace or override the
/// workspace's file for the code under it.
const TOOL_CONFIGS: [ToolConfig; 27] = [
    ToolConfig::new("clippy.toml", "clippy", "clippy.toml"),
    ToolConfig::new(".clippy.toml", "clippy", "clippy.toml"),
    ToolConfig::new("rustfmt.toml", "rustfmt", "rustfmt.toml"),
    ToolConfig::new(".rustfmt.toml", "rustfmt", "rustfmt.toml"),
    ToolConfig::new("deny.toml", "cargo-deny", "deny.toml"),
    ToolConfig::new(".deny.toml", "cargo-deny", "deny.toml"),
    ToolConfig::new("rust-toolchain", "rustup", "rust-toolchain.toml"),
    ToolConfig::new("rust-toolchain.toml", "rustup", "rust-toolchain.toml"),
    ToolConfig::new("typos.toml", "typos", "_typos.toml"),
    ToolConfig::new("_typos.toml", "typos", "_typos.toml"),
    ToolConfig::new(".typos.toml", "typos", "_typos.toml"),
    ToolConfig::new("taplo.toml", "taplo", ".taplo.toml"),
    ToolConfig::new(".taplo.toml", "taplo", ".taplo.toml"),
    ToolConfig::new("dylint.toml", "cargo-dylint", "dylint.toml"),
    ToolConfig::new(".cargo/config", "Cargo", ".cargo/config.toml"),
    ToolConfig::new(".cargo/config.toml", "Cargo", ".cargo/config.toml"),
    ToolConfig::new(
        ".config/nextest.toml",
        "cargo-nextest",
        ".config/nextest.toml",
    ),
    ToolConfig::new("release-plz.toml", "release-plz", "release-plz.toml"),
    ToolConfig::new(".release-plz.toml", "release-plz", "release-plz.toml"),
    ToolConfig::new("renovate.json", "Renovate", "renovate.json"),
    ToolConfig::new("renovate.json5", "Renovate", "renovate.json"),
    ToolConfig::new(".renovaterc", "Renovate", "renovate.json"),
    ToolConfig::new(".renovaterc.json", "Renovate", "renovate.json"),
    ToolConfig::new(".renovaterc.json5", "Renovate", "renovate.json"),
    ToolConfig::new("lychee.toml", "lychee", "lychee.toml"),
    ToolConfig::new(".editorconfig", "EditorConfig", ".editorconfig"),
    ToolConfig::new(".gitattributes", "git", ".gitattributes"),
];

impl ToolConfig {
    /// The configuration file `name`, which `tool` reads, and which the workspace keeps as
    /// `workspace_file`.
    const fn new(name: &'static str, tool: &'static str, workspace_file: &'static str) -> Self {
        Self {
            name,
            tool,
            workspace_file,
        }
    }

    /// The tool configuration that `relative`, a path relative to the root with `/` separators,
    /// names, if any.
    pub(crate) fn of(relative: &str) -> Option<&'static Self> {
        TOOL_CONFIGS
            .iter()
            .find(|config| config.directory(relative).is_some())
    }

    /// The directory, relative to the root and without a trailing `/`, that holds `relative` if it
    /// has this configuration's name in any case, since a case-insensitive file system finds
    /// `Clippy.toml` for `clippy.toml`. The root is the empty string.
    pub(crate) fn directory<'a>(&self, relative: &'a str) -> Option<&'a str> {
        let directory = case_insensitive::strip_suffix(relative, self.name)?;
        if directory.is_empty() {
            Some(directory)
        } else {
            directory.strip_suffix('/')
        }
    }

    /// The tool that reads the file.
    pub(crate) fn tool(&self) -> &'static str {
        self.tool
    }

    /// The workspace's own file for the tool, relative to the root.
    pub(crate) fn workspace_file(&self) -> &'static str {
        self.workspace_file
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_names_match_at_any_depth_in_any_case() {
        for (path, directory) in [
            ("clippy.toml", ""),
            (".clippy.toml", ""),
            ("crates/app/clippy.toml", "crates/app"),
            ("crates/app/src/rustfmt.toml", "crates/app/src"),
            ("crates/app/.cargo/config", "crates/app"),
            (".cargo/config", ""),
            ("crates/app/rust-toolchain", "crates/app"),
            ("docs/_typos.toml", "docs"),
            ("crates/app_core/Clippy.toml", "crates/app_core"),
            (".config/nextest.toml", ""),
            ("crates/app/.config/nextest.toml", "crates/app"),
            ("crates/app/.editorconfig", "crates/app"),
            ("crates/app/.gitattributes", "crates/app"),
            (".github/renovate.json", ".github"),
            ("crates/app/release-plz.toml", "crates/app"),
            ("docs/lychee.toml", "docs"),
            ("crates/app/RUSTFMT.TOML", "crates/app"),
            ("crates/app/.Cargo/Config.toml", "crates/app"),
            (".CARGO/CONFIG", ""),
            ("Deny.toml", ""),
            ("crates/app/ru\u{17f}tfmt.toml", "crates/app"),
        ] {
            let config = ToolConfig::of(path);
            assert_eq!(
                config.and_then(|config| config.directory(path)),
                Some(directory),
                "{path} should be a tool configuration file in {directory:?}"
            );
        }
    }

    #[test]
    fn other_names_dont_match() {
        for path in [
            "crates/app/Cargo.toml",
            "crates/app/myclippy.toml",
            "crates/app/cargo/config.toml",
            "crates/app/src/config.rs",
            "notes/clippy.toml.md",
            "crates/app/myrenovate.json",
            "crates/app/config/nextest.toml",
        ] {
            assert!(
                ToolConfig::of(path).is_none(),
                "{path} shouldn't be a tool configuration file"
            );
        }
    }
}
