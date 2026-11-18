//! The only file of instructions for coding agents is the root `AGENTS.md`.

use crate::policy_error::PolicyError;
use crate::violation::Violation;
use crate::workspace::Workspace;
use crate::{case_insensitive, repository_files};

/// The one agent instruction file the repository keeps, relative to the root.
const KEPT_FILE: &str = "AGENTS.md";

/// The file names, in any case, that coding agents read as instructions wherever they are.
const NAMES: [&str; 2] = ["GEMINI.md", ".cursorrules"];

/// The stems of Markdown files, in any case, that coding agents read as instructions wherever
/// they are: `AGENTS.md` and `CLAUDE.md`, and their dotted variants such as `AGENTS.override.md`
/// or `CLAUDE.local.md`.
const STEMS: [&str; 2] = ["AGENTS", "CLAUDE"];

/// The directories, in any case, whose every file configures or instructs a coding agent.
const DIRECTORIES: [&str; 2] = [".cursor", ".claude"];

/// The files, relative to a directory and in any case, that coding agents read as instructions.
const NESTED_FILES: [&str; 1] = [".github/copilot-instructions.md"];

/// Whether a coding agent reads `relative`, a path relative to the root with `/` separators, as
/// instructions or settings: an `AGENTS.md`, `AGENTS.*.md`, `CLAUDE.md`, `CLAUDE.*.md`,
/// `GEMINI.md` or `.cursorrules` file, anything in a `.cursor` or `.claude` directory, or
/// `.github/copilot-instructions.md`, at any depth and in any case.
pub(crate) fn is_agent_file(relative: &str) -> bool {
    let name = relative.rsplit('/').next().unwrap_or(relative);
    let markdown_stem = case_insensitive::strip_suffix(name, ".md");
    NAMES.iter().any(|known| case_insensitive::eq(name, known))
        || markdown_stem.is_some_and(|stem| {
            STEMS.iter().any(|known| {
                case_insensitive::strip_prefix(stem, known)
                    .is_some_and(|rest| rest.is_empty() || rest.starts_with('.'))
            })
        })
        || relative.split('/').any(|part| {
            DIRECTORIES
                .iter()
                .any(|directory| case_insensitive::eq(part, directory))
        })
        || NESTED_FILES.iter().any(|file| {
            case_insensitive::strip_suffix(relative, file)
                .is_some_and(|directory| directory.is_empty() || directory.ends_with('/'))
        })
}

/// Checks that the root `AGENTS.md` is the repository's only file of instructions for coding
/// agents. Another one, such as a `CLAUDE.md`, a `.cursor/` directory or an `AGENTS.md` in a
/// crate, would tell an agent something the root file doesn't, where review of `AGENTS.md` doesn't
/// see it.
///
/// It reads the files git tracks and the untracked ones `.gitignore` doesn't exclude.
///
/// # Errors
///
/// Fails if the repository's files can't be listed.
pub fn check(workspace: &Workspace) -> Result<Vec<Violation>, PolicyError> {
    let mut violations = Vec::new();
    for relative in repository_files::list(workspace.root())? {
        if is_agent_file(&relative) && relative != KEPT_FILE {
            violations.push(Violation::new(
                relative,
                format!(
                    "Coding agents read this file as instructions or settings, beside or instead \
                     of the root `{KEPT_FILE}`. The repository keeps every instruction for agents \
                     in the root `{KEPT_FILE}`, so one reviewed file says what they are told. \
                     Move what this file says that still applies into `{KEPT_FILE}`, and delete \
                     it."
                ),
            ));
        }
    }
    Ok(violations)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_files_match_at_any_depth_in_any_case() {
        for path in [
            "AGENTS.md",
            "agents.md",
            "AGENTS.override.md",
            "Agents.Override.MD",
            "crates/app/AGENTS.md",
            "CLAUDE.md",
            "crates/app/claude.local.md",
            "docs/Claude.md",
            "GEMINI.md",
            "crates/app/gemini.MD",
            ".cursorrules",
            "crates/app/.CursorRules",
            ".cursor/rules/style.mdc",
            "crates/app/.cursor/mcp.json",
            ".claude/settings.json",
            "crates/app/.Claude/commands/x.md",
            ".github/copilot-instructions.md",
            ".GitHub/Copilot-Instructions.md",
        ] {
            assert!(is_agent_file(path), "{path} is an agent instruction file");
        }
    }

    #[test]
    fn other_files_are_not_agent_files() {
        for path in [
            "README.md",
            "CONTRIBUTING.md",
            "docs/agents.txt",
            "docs/my-agents.md",
            "docs/agents-overview.md",
            "docs/claude-setup.md",
            "AGENTSX.md",
            "claude_notes.md",
            "AGENTS.md.txt",
            "crates/claude_client/src/lib.rs",
            "crates/app/src/cursor.rs",
            ".github/workflows/ci.yml",
            ".github/my-copilot-instructions.md",
            "GEMINI.md.bak",
        ] {
            assert!(
                !is_agent_file(path),
                "{path} isn't an agent instruction file"
            );
        }
    }
}
