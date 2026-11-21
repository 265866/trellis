//! Tool configuration files exist only where the workspace keeps them, and a TypeScript
//! project's settings reach only other `tsconfig.json` files.

use std::fs;

use serde_json::Value;

use crate::policy_error::PolicyError;
use crate::tool_config::ToolConfig;
use crate::violation::Violation;
use crate::workspace::Workspace;
use crate::{case_insensitive, repository_files};

/// The name of a TypeScript project's settings file, which is a rule wherever it sits.
const TSCONFIG: &str = "tsconfig.json";

/// Checks that no tool configuration file exists in the repository besides the workspace's own
/// file for each tool, at the root, such as a `clippy.toml` in a crate that would replace the root
/// one for that crate, or a `.rustfmt.toml` at the root that would override `rustfmt.toml`. A
/// name matches in any case, since a case-insensitive file system finds `Clippy.toml` for
/// `clippy.toml`, but a kept file keeps its exact name, which every file system finds.
///
/// It also checks that each `tsconfig.json`, in any case, is JSON with comments, and that every
/// path its `extends` and `references` give names, relative to it, another file in the repository
/// called `tsconfig.json`, in any case, outside `node_modules`. Each `tsconfig.json` is a rule, so
/// every file a TypeScript check reads its settings from is one too. Without this, a project's
/// `tsconfig.json` could extend a file of another name, such as `tsconfig.base.json`, which under
/// `crates/` is code, so a code change could set `"strict": false` there. A package name, such as
/// `@tsconfig/strictest`, fails too, since the files it names come from `node_modules`, which the
/// web UI's `package.json` and lockfile pick, and those are code.
///
/// It reads the files git tracks and the untracked ones `.gitignore` doesn't exclude, so a
/// directory's name can't hide a file from it; outside a git checkout it reads every file but
/// Cargo's build output.
///
/// # Errors
///
/// Fails if the repository's files can't be listed, or a `tsconfig.json` can't be read.
pub fn check(workspace: &Workspace) -> Result<Vec<Violation>, PolicyError> {
    let mut violations = Vec::new();
    let files = repository_files::list(workspace.root())?;
    for relative in &files {
        if let Some(config) = ToolConfig::of(relative)
            && *relative != config.workspace_file()
        {
            let explanation = message(relative, config);
            violations.push(Violation::new(relative.clone(), explanation));
        }
        let name = relative.rsplit('/').next().unwrap_or(relative);
        if case_insensitive::eq(name, TSCONFIG) {
            let path = workspace.root().join(relative);
            let text =
                fs::read_to_string(&path).map_err(|source| PolicyError::Read { path, source })?;
            for problem in tsconfig_problems(relative, &text, &files) {
                violations.push(Violation::new(relative.clone(), problem));
            }
        }
    }
    Ok(violations)
}

/// What is wrong with the `tsconfig.json` at `relative`, whose text is `text`, in a repository
/// that holds `files`, sorted: each `extends` or `references` entry that doesn't name another
/// `tsconfig.json` among `files`, or that it doesn't parse.
fn tsconfig_problems(relative: &str, text: &str, files: &[String]) -> Vec<String> {
    let remedy = "TypeScript would read settings from a file that isn't a rule, so a code change \
                  could loosen the check. Name another `tsconfig.json` in the repository by a path \
                  that starts with `./` or `../`, and keep the settings there.";
    let Ok(Value::Object(settings)) = serde_json::from_str::<Value>(&without_comments(text)) else {
        return vec![
            "This file isn't JSON with comments, as TypeScript reads it, so no check can tell \
             which files it reaches through `extends` and `references`. Fix its syntax."
                .to_owned(),
        ];
    };
    let mut problems = Vec::new();
    let mut malformed = |key: &str, value: &Value| {
        problems.push(format!(
            "`{key}` is `{value}`, which isn't a path or a list of them as TypeScript reads it. \
             {remedy}"
        ));
    };
    let mut paths = Vec::new();
    match settings.get("extends") {
        None => {}
        Some(Value::String(path)) => paths.push(("extends", path.as_str())),
        Some(Value::Array(list)) if list.iter().all(Value::is_string) => {
            paths.extend(
                list.iter()
                    .filter_map(Value::as_str)
                    .map(|path| ("extends", path)),
            );
        }
        Some(other) => malformed("extends", other),
    }
    match settings.get("references") {
        None => {}
        Some(Value::Array(list))
            if list
                .iter()
                .all(|reference| reference.get("path").is_some_and(Value::is_string)) =>
        {
            let references = list
                .iter()
                .filter_map(|reference| reference.get("path").and_then(Value::as_str));
            paths.extend(references.map(|path| ("references", path)));
        }
        Some(other) => malformed("references", other),
    }
    let directory = relative
        .rsplit_once('/')
        .map_or("", |(directory, _)| directory);
    for (key, path) in paths {
        let reaches = (path.starts_with("./") || path.starts_with("../"))
            && path
                .rsplit('/')
                .next()
                .is_some_and(|name| case_insensitive::eq(name, TSCONFIG))
            && !path
                .split('/')
                .any(|part| case_insensitive::eq(part, "node_modules"))
            && joined(directory, path).is_some_and(|path| files.binary_search(&path).is_ok());
        if !reaches {
            problems.push(format!("`{key}` names `{path}`. {remedy}"));
        }
    }
    problems
}

/// `path`, relative to the directory `directory` of the repository, as a path relative to the
/// repository's root, or `None` if it climbs above the root.
fn joined(directory: &str, path: &str) -> Option<String> {
    let mut parts: Vec<&str> = directory
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            _ => parts.push(part),
        }
    }
    Some(parts.join("/"))
}

/// `text`, JSON with the comments and trailing commas TypeScript allows in its settings, as plain
/// JSON: each comment outside a string becomes a space, and each comma before a closing `}` or
/// `]` is dropped.
fn without_comments(text: &str) -> String {
    let mut json = String::with_capacity(text.len());
    let mut characters = text.trim_start_matches('\u{feff}').chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '"' => {
                json.push(character);
                while let Some(character) = characters.next() {
                    json.push(character);
                    match character {
                        '\\' => json.extend(characters.next()),
                        '"' => break,
                        _ => {}
                    }
                }
            }
            '/' if characters.peek() == Some(&'/') => {
                while characters.next_if(|&next| next != '\n').is_some() {}
                json.push(' ');
            }
            '/' if characters.peek() == Some(&'*') => {
                characters.next();
                let mut previous = ' ';
                for next in characters.by_ref() {
                    if previous == '*' && next == '/' {
                        break;
                    }
                    previous = next;
                }
                json.push(' ');
            }
            '}' | ']' => {
                let end = json.trim_end().len();
                if json[..end].ends_with(',') {
                    json.remove(end - 1);
                }
                json.push(character);
            }
            _ => json.push(character),
        }
    }
    json
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comments_and_trailing_commas_leave_plain_json() {
        let text = "\u{feff}{\n  // \"extends\": \"./x.json\",\n  /* a\n block */ \"a\": \"//not \\\"/*\",\n  \
                    \"b\": [1, 2, ],\n  \"c\": { \"d\": \"*/\", },\n}\n";
        let value: Value =
            serde_json::from_str(&without_comments(text)).expect("the text should parse");
        assert_eq!(
            value,
            serde_json::json!({ "a": "//not \"/*", "b": [1, 2], "c": { "d": "*/" } }),
            "comments go, strings that look like them stay, and trailing commas go"
        );
    }

    #[test]
    fn paths_join_inside_the_repository() {
        assert_eq!(
            joined("crates/demo/web", "../../demo_core/./web/tsconfig.json"),
            Some("crates/demo_core/web/tsconfig.json".to_owned()),
            "a path climbs and descends"
        );
        assert_eq!(
            joined("", "./tsconfig.json"),
            Some("tsconfig.json".to_owned()),
            "a path at the root"
        );
        assert_eq!(
            joined("web", "../../tsconfig.json"),
            None,
            "a path above the root leaves the repository"
        );
    }
}
