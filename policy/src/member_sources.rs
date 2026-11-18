//! Each member's sources live under its own directory.

use std::fs;

use proc_macro2::{Spacing, TokenStream, TokenTree};

use crate::policy_error::PolicyError;
use crate::token_context::TokenContext;
use crate::violation::Violation;
use crate::workspace::Workspace;
use crate::{case_insensitive, repository_files, source_path};

/// Why a crate's sources stay under its directory, and what to do instead.
const REMEDY: &str = "A crate's sources live under its own directory, so that a pull request \
                      that changes them counts as a code change. Move the file into the crate.";

/// Checks that each member reads its Rust sources only from under its own directory, so a file
/// outside `crates/`, which `trellis-policy rule-changes` might not count as code, can't become
/// part of a crate:
///
/// - every `path = ...` in an attribute, such as `#[path = "../../docs/notes.rs"]` or
///   `#[cfg_attr(unix, path = "...")]`, is a string literal the check can read, not a macro
///   variable or an expression, that names a `.rs` file inside the crate's directory, so the
///   check also reads that file;
/// - every `path = "..."` string in a macro's arguments, which the macro could turn into an
///   attribute, does the same;
/// - no file in or above a member's directory is a symbolic link.
///
/// A target's `path` in `Cargo.toml` is checked with the rest of the manifest.
///
/// # Errors
///
/// Fails if the repository's files can't be listed, or a member's file can't be read.
pub fn check(workspace: &Workspace) -> Result<Vec<Violation>, PolicyError> {
    let files = repository_files::list(workspace.root())?;
    let mut violations = Vec::new();
    for member in workspace.members() {
        let directory = workspace.relative(member.dir());
        for file in &files {
            let inside = if directory.is_empty() {
                Some(file.as_str())
            } else {
                case_insensitive::strip_prefix(file, &directory)
                    .and_then(|rest| rest.strip_prefix('/'))
            };
            let encloses = case_insensitive::strip_prefix(&directory, file)
                .is_some_and(|rest| rest.starts_with('/'));
            if inside.is_none() && !encloses {
                continue;
            }
            let path = workspace.root().join(file);
            let read_error = |source| PolicyError::Read {
                path: path.clone(),
                source,
            };
            if fs::symlink_metadata(&path)
                .map_err(read_error)?
                .is_symlink()
            {
                violations.push(Violation::new(
                    file.clone(),
                    format!(
                        "This is a symbolic link in or above the crate `{}`, so the crate could \
                         read a file from anywhere. {REMEDY}",
                        member.name()
                    ),
                ));
                continue;
            }
            let Some(inside) = inside.filter(|inside| source_path::is_rust(inside)) else {
                continue;
            };
            let text = fs::read_to_string(&path).map_err(read_error)?;
            let depth = inside.matches('/').count();
            for problem in problems(&text, depth) {
                violations.push(Violation::new(file.clone(), problem));
            }
        }
    }
    Ok(violations)
}

/// What is wrong with the Rust source `text`, a file `depth` directories below its crate's
/// directory.
fn problems(text: &str, depth: usize) -> Vec<String> {
    use std::str::FromStr as _;

    let tokens = match TokenStream::from_str(text) {
        Ok(tokens) => tokens,
        Err(error) => {
            return vec![format!(
                "line {}: the file doesn't lex as Rust ({error}), so the check can't see which \
                 files it reads. Fix its syntax.",
                error.span().start().line
            )];
        }
    };
    let mut problems = Vec::new();
    scan(tokens, TokenContext::Code, depth, &mut problems);
    problems
}

/// Adds a problem to `problems` for each `path = ...` in `tokens`, which are in `context`, that
/// names a file the crate can't read its sources from, or that the check can't read.
fn scan(tokens: TokenStream, context: TokenContext, depth: usize, problems: &mut Vec<String>) {
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    for (index, tree) in trees.iter().enumerate() {
        match tree {
            TokenTree::Group(group) => {
                scan(
                    group.stream(),
                    context.inside(&trees, index),
                    depth,
                    problems,
                );
            }
            TokenTree::Ident(ident)
                if context != TokenContext::Code
                    && ident.to_string().trim_start_matches("r#") == "path"
                    && is_assignment(trees.get(index + 1)) =>
            {
                if let Some(problem) = value_problem(trees.get(index + 2), context, depth) {
                    problems.push(format!("line {}: {problem}", ident.span().start().line));
                }
            }
            TokenTree::Ident(_) | TokenTree::Punct(_) | TokenTree::Literal(_) => {}
        }
    }
}

/// Whether `tree` is a lone `=`, rather than part of `==` or `=>`.
fn is_assignment(tree: Option<&TokenTree>) -> bool {
    matches!(
        tree,
        Some(TokenTree::Punct(punct)) if punct.as_char() == '=' && punct.spacing() == Spacing::Alone
    )
}

/// What is wrong with `value`, the token after `path =` in `context` in a file `depth`
/// directories below its crate's directory, if anything. In an attribute the value must be a
/// plain string literal; in macro arguments, which a macro can turn into an attribute, only a
/// string literal is checked.
fn value_problem(value: Option<&TokenTree>, context: TokenContext, depth: usize) -> Option<String> {
    let shown = value.map_or_else(|| "nothing".to_owned(), ToString::to_string);
    let literal = match value {
        Some(TokenTree::Literal(literal)) => literal.to_string(),
        _ => String::new(),
    };
    let Some(path) = plain_string(&literal) else {
        let is_string = literal.starts_with('"') || literal.starts_with('r');
        return (context == TokenContext::Attribute || is_string).then(|| {
            format!(
                "`path = {shown}` isn't a plain string literal, so the check can't tell which \
                 file it names. Write the path as a string literal with no escapes but `\\\\`."
            )
        });
    };
    if source_path::escapes(depth, &path) {
        Some(format!(
            "`path = {shown}` names a file outside the crate's directory. {REMEDY}"
        ))
    } else if source_path::is_rust(&path) {
        None
    } else {
        Some(format!(
            "`path = {shown}` names a file that doesn't end in `.rs`. The check reads only `.rs` \
             files for the files they read in turn, so a module's file ends in `.rs`."
        ))
    }
}

/// The text of the string literal written `literal`, if it is a raw string or a string whose
/// only escape is `\\`.
fn plain_string(literal: &str) -> Option<String> {
    if let Some(raw) = literal.strip_prefix('r') {
        let unhashed = raw.trim_start_matches('#');
        let hashes = "#".repeat(raw.len() - unhashed.len());
        let text = unhashed
            .strip_prefix('"')?
            .strip_suffix(hashes.as_str())?
            .strip_suffix('"')?;
        return Some(text.to_owned());
    }
    let quoted = literal.strip_prefix('"')?.strip_suffix('"')?;
    let mut text = String::with_capacity(quoted.len());
    let mut characters = quoted.chars();
    while let Some(character) = characters.next() {
        if character == '\\' && characters.next()? != '\\' {
            return None;
        }
        text.push(character);
    }
    Some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_inside_the_crate_pass() {
        for (source, depth) in [
            ("#[path = \"other.rs\"]\nmod a;\n", 1),
            ("#[path = \"../tests/x.rs\"]\nmod a;\n", 1),
            ("#[cfg_attr(unix, path = r\"unix/x.RS\")]\nmod a;\n", 0),
            ("m!(path = \"../x.rs\");\n", 1),
            ("fn f() { let path = \"../../../notes.txt\"; }\n", 0),
            (
                "fn f(path: &str) -> bool { let path = path.len(); path == 1 }\n",
                0,
            ),
            (
                "fn f(name: &str) -> String { format!(\"{path}\", path = name) }\n",
                0,
            ),
            (
                "/// `#[path = \"../../../x.rs\"]` in a doc comment\nfn f() {}\n",
                0,
            ),
            (
                "fn f() -> &'static str { \"#[path = \\\"../../x.rs\\\"]\" }\n",
                0,
            ),
            (
                "macro_rules! m { (path = $p:expr) => {}; (path => $p:expr) => {} }\n",
                0,
            ),
        ] {
            assert_eq!(
                problems(source, depth),
                Vec::<String>::new(),
                "{source:?} at depth {depth} reads nothing from outside the crate"
            );
        }
    }

    #[test]
    fn paths_outside_the_crate_fail() {
        for (source, depth) in [
            ("#[path = \"../../../docs/notes.rs\"]\nmod notes;\n", 1),
            ("#[path = \"../../docs/notes.md\"]\nmod notes;\n", 0),
            ("#[cfg_attr(all(), path = \"../../x.rs\")]\nmod a;\n", 1),
            ("mod a {\n    #![path = \"..\\\\..\\\\x.rs\"]\n}\n", 0),
            ("#[path = r#\"/tmp/x.rs\"#]\nmod a;\n", 3),
            ("m!(path = \"../../../x.rs\");\n", 1),
            ("crate::m! { mod a; path = \"../x.rs\" }\n", 0),
            ("#[r#path = \"../x.rs\"]\nmod a;\n", 0),
        ] {
            let problems = problems(source, depth);
            assert!(
                problems.len() == 1 && problems[0].contains("outside the crate's directory"),
                "{source:?} at depth {depth} should fail, found {problems:?}"
            );
        }
    }

    #[test]
    fn paths_to_files_the_check_doesnt_read_fail() {
        for source in [
            "#[path = \"data.txt\"]\nmod data;\n",
            "#[path = \"sub\"]\nmod sub {\n    mod a;\n}\n",
            "m!(path = \"notes.md\");\n",
        ] {
            let problems = problems(source, 0);
            assert!(
                problems.len() == 1 && problems[0].contains("doesn't end in `.rs`"),
                "{source:?} should fail, found {problems:?}"
            );
        }
    }

    #[test]
    fn paths_the_check_cant_read_fail() {
        for source in [
            "macro_rules! m { ($p:literal) => { #[path = $p] mod a; } }\n",
            "#[path = concat!(\"..\", \"/x.rs\")]\nmod a;\n",
            "#[path = \"\\x2e\\x2e/\\x2e\\x2e/x.rs\"]\nmod a;\n",
            "m!(path = \"\\x2e\\x2e/x.rs\");\n",
        ] {
            let problems = problems(source, 0);
            assert!(
                problems.len() == 1 && problems[0].contains("isn't a plain string literal"),
                "{source:?} should fail, found {problems:?}"
            );
        }
    }

    #[test]
    fn a_problem_names_its_line() {
        let problems = problems("//! A crate.\n\n#[path = \"../../x.rs\"]\nmod a;\n", 0);
        assert!(
            problems.len() == 1 && problems[0].starts_with("line 3: "),
            "the problem should name line 3, found {problems:?}"
        );
    }

    #[test]
    fn a_file_that_doesnt_lex_fails() {
        let problems = problems("fn f() { \"unclosed }\n", 0);
        assert!(
            problems.len() == 1 && problems[0].contains("doesn't lex"),
            "an unclosed string should fail, found {problems:?}"
        );
    }
}
