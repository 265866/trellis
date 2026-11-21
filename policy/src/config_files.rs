//! Tool configuration files exist only where the workspace keeps them, and a TypeScript
//! project's settings reach only other TypeScript settings files.

use std::fs;

use serde_json::{Map, Value};

use crate::policy_error::PolicyError;
use crate::tool_config::ToolConfig;
use crate::violation::Violation;
use crate::workspace::Workspace;
use crate::{case_insensitive, repository_files};

/// Whether `name`, a file's name, is a TypeScript settings file's: one that starts with
/// `tsconfig` and ends in `.json`, in any case, such as `tsconfig.json`, `tsconfig.base.json` or
/// Vite's `tsconfig.app.json`. Such a file is a rule wherever it sits.
pub(crate) fn is_tsconfig(name: &str) -> bool {
    case_insensitive::strip_prefix(name, "tsconfig")
        .and_then(|rest| case_insensitive::strip_suffix(rest, ".json"))
        .is_some()
}

/// Checks that no tool configuration file exists in the repository besides the workspace's own
/// file for each tool, at the root, such as a `clippy.toml` in a crate that would replace the root
/// one for that crate, or a `.rustfmt.toml` at the root that would override `rustfmt.toml`. A
/// name matches in any case, since a case-insensitive file system finds `Clippy.toml` for
/// `clippy.toml`, but a kept file keeps its exact name, which every file system finds.
///
/// It also checks each TypeScript settings file, a `.json` file whose name starts with `tsconfig`
/// in any case: that it isn't a symbolic link, that TypeScript can read it only one way, and that every path its `extends` and
/// `references` give names another settings file in the repository. A path names one when it
/// starts with `./` or `../`, holds no `\`, stays inside the repository and outside
/// `node_modules`, and leads to such a file that is there; a `references` path that doesn't end in
/// `.json` names the `tsconfig.json` in that directory, as TypeScript reads it. Each settings file
/// is a rule, so every file a `tsconfig.json` reaches is a rule too, and a code change can't set
/// `"strict": false` in a file it extends. A package name, such as `@tsconfig/strictest`, fails,
/// since the files it names come from `node_modules`, which the web UI's `package.json` and
/// lockfile pick, and those are code. The web UI's `check` script is code as well, and it chooses
/// which settings file `tsc` reads and which flags it adds.
///
/// TypeScript reads a settings file it finds a syntax error in, and ends a `//` comment at a
/// carriage return, a line separator or a paragraph separator, so a file it could read otherwise
/// fails: one that holds such a character outside a `\r\n`, a block comment never closed, a comma
/// with nothing before it, or a key twice in one object.
///
/// It reads the files git tracks and the untracked ones `.gitignore` doesn't exclude, so a
/// directory's name can't hide a file from it; outside a git checkout it reads every file but
/// Cargo's build output.
///
/// # Errors
///
/// Fails if the repository's files can't be listed, or a TypeScript settings file can't be read.
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
        if !is_tsconfig(name) {
            continue;
        }
        let path = workspace.root().join(relative);
        let read_error = |source| PolicyError::Read {
            path: path.clone(),
            source,
        };
        if fs::symlink_metadata(&path)
            .map_err(read_error)?
            .is_symlink()
        {
            violations.push(Violation::new(
                relative.clone(),
                "This is a symbolic link, so TypeScript reads its settings from a file whose \
                 changes may not count as rules. Replace the link with the file itself."
                    .to_owned(),
            ));
            continue;
        }
        let text = fs::read_to_string(&path).map_err(read_error)?;
        for problem in tsconfig_problems(relative, &text, &files) {
            violations.push(Violation::new(relative.clone(), problem));
        }
    }
    Ok(violations)
}

/// What is wrong with the TypeScript settings file at `relative`, whose text is `text`, in a
/// repository that holds `files`, sorted: each `extends` or `references` entry that doesn't name
/// another settings file among `files`, or that TypeScript could read it otherwise.
fn tsconfig_problems(relative: &str, text: &str, files: &[String]) -> Vec<String> {
    let remedy = "TypeScript would read settings from a file that isn't a rule, so a code change \
                  could loosen the check. Name a file in the repository whose name starts with \
                  `tsconfig` and ends in `.json`, by a path that starts with `./` or `../` and \
                  stays outside `node_modules`, and keep the settings there; in `references`, a \
                  directory that holds a `tsconfig.json` will do too.";
    let Some(settings) = settings(text) else {
        return vec![
            "TypeScript could read this file otherwise than as JSON with comments, so no check \
             can tell which files it reaches through `extends` and `references`. Fix its syntax: \
             end lines only with `\\n` or `\\r\\n`, close every comment, leave no comma with \
             nothing before it, and give each key once."
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
        #[expect(
            clippy::case_sensitive_file_extension_comparisons,
            reason = "TypeScript reads a `references` path as a file only when it ends in `.json` \
                      in these letters, and as a directory otherwise"
        )]
        let file = if key == "references" && !path.ends_with(".json") {
            format!("{path}/tsconfig.json")
        } else {
            path.to_owned()
        };
        let reaches = (path.starts_with("./") || path.starts_with("../"))
            && !path.contains('\\')
            && joined(directory, &file).is_some_and(|file| {
                let name = file.rsplit('/').next().unwrap_or(&file);
                is_tsconfig(name)
                    && !file
                        .split('/')
                        .any(|part| case_insensitive::eq(part, "node_modules"))
                    && files.binary_search(&file).is_ok()
            });
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

/// The settings in `text`, a TypeScript settings file, or `None` if it isn't an object of JSON
/// with comments or TypeScript could read it otherwise. TypeScript reads past a syntax error, so
/// anything it reads differently from the strict reading here fails: a line end other than
/// `\n` or `\r\n`, since TypeScript ends a `//` comment at a lone `\r`, U+2028 or U+2029 and some
/// readers at U+0085, a block comment never closed, a comma with nothing before it, and a key
/// given twice in one object.
fn settings(text: &str) -> Option<Map<String, Value>> {
    let text = text.trim_start_matches('\u{feff}');
    let other_line_end = text.contains(['\u{85}', '\u{2028}', '\u{2029}'])
        || text
            .match_indices('\r')
            .any(|(at, _)| !text[at + 1..].starts_with('\n'));
    if other_line_end {
        return None;
    }
    let json = without_comments(text)?;
    let value: Value = serde_json::from_str(&json).ok()?;
    if keys_in_text(&json) != keys_in_value(&value) {
        return None;
    }
    match value {
        Value::Object(settings) => Some(settings),
        _ => None,
    }
}

/// `text`, JSON with the comments and trailing commas TypeScript allows in its settings, as plain
/// JSON: each comment outside a string becomes a space, and each comma before a closing `}` or
/// `]` is dropped. `None` if a block comment is never closed or a comma has nothing before it.
fn without_comments(text: &str) -> Option<String> {
    let mut json = String::with_capacity(text.len());
    let mut characters = text.chars().peekable();
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
                loop {
                    let next = characters.next()?;
                    if previous == '*' && next == '/' {
                        break;
                    }
                    previous = next;
                }
                json.push(' ');
            }
            ',' if json.trim_end().ends_with(['{', '[', ',']) => return None,
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
    Some(json)
}

/// How many keys `json`, text `serde_json` reads as JSON, gives: each string followed by a `:`.
fn keys_in_text(json: &str) -> usize {
    let mut keys = 0;
    let mut characters = json.chars().peekable();
    while let Some(character) = characters.next() {
        if character != '"' {
            continue;
        }
        while let Some(character) = characters.next() {
            match character {
                '\\' => {
                    characters.next();
                }
                '"' => break,
                _ => {}
            }
        }
        while characters.next_if(char::is_ascii_whitespace).is_some() {}
        if characters.peek() == Some(&':') {
            keys += 1;
        }
    }
    keys
}

/// How many keys the objects in `value` hold, so fewer than its text gives means one was given
/// twice.
fn keys_in_value(value: &Value) -> usize {
    match value {
        Value::Object(object) => object.len() + object.values().map(keys_in_value).sum::<usize>(),
        Value::Array(list) => list.iter().map(keys_in_value).sum(),
        _ => 0,
    }
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
        let text = "\u{feff}{\r\n  // \"extends\": \"./x.json\",\r\n  /* a\n block */ \"a\": \"//not \\\"/*\",\n  \
                    \"b\": [1, 2, ],\n  \"c\": { \"d\": \"*/\", },\n}\n";
        assert_eq!(
            settings(text).map(Value::Object),
            Some(serde_json::json!({ "a": "//not \"/*", "b": [1, 2], "c": { "d": "*/" } })),
            "comments go, strings that look like them stay, and trailing commas go"
        );
    }

    #[test]
    fn text_typescript_could_read_otherwise_has_no_settings() {
        for text in [
            "{ // a\r\"a\": 1 }",
            "{ // a\u{2028}\"a\": 1 }",
            "{ // a\u{2029}\"a\": 1 }",
            "{ // a\u{85}\"a\": 1 }",
            "{ \"a\": \"\u{2028}\" }",
            "{ \"a\": 1, \"a\": 2 }",
            "{ \"a\": { \"b\": 1, \"b\" : 1 } }",
            "{ \"a\": [{ \"b\": 1 }, { \"b\": 1, \"b\": 2 }] }",
            "{ \"a\": 1 } /* open",
            "{ \"a\": 1 /*/ }",
            "{ , }",
            "{ \"a\": [ , ] }",
            "{ \"a\": [1, , ] }",
            "{ \"a\": 1 } { }",
            "[]",
            "{ a: 1 }",
            "{ 'a': 1 }",
            "",
        ] {
            assert_eq!(settings(text), None, "{text:?} could read otherwise");
        }
        assert_eq!(
            keys_in_text("{\"a\": \"b:\", \"c\\\":\" : {\"d\":1}}"),
            3,
            "a key is a string followed by a colon"
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
