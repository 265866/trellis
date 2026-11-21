//! Each member's sources live under its own directory.

use std::fs;

use proc_macro2::{Delimiter, Ident, Spacing, TokenStream, TokenTree};

use crate::policy_error::PolicyError;
use crate::token_context::TokenContext;
use crate::violation::Violation;
use crate::workspace::Workspace;
use crate::{case_insensitive, repository_files, source_path};

/// Why a crate's sources stay under its directory, and what to do instead.
const REMEDY: &str = "A crate's sources live under its own directory, so that a pull request \
                      that changes them counts as a code change. Move the file into the crate.";

/// Checks that each member's module files lie under its own directory, so a file outside
/// `crates/`, which `trellis-policy rule-changes` might not count as code, can't become a module
/// of a crate:
///
/// - every `path` attribute, such as `#[path = "../../docs/notes.rs"]` or `#![path = "..."]`,
///   and every `path = ...` that a `cfg_attr` applies, such as
///   `#[cfg_attr(unix, path = "...")]`, holds a string literal the check can read, not a macro
///   variable or an expression, that names a `.rs` file inside the crate's directory, so the
///   check also reads that file. It finds these attributes inside macro arguments and inside
///   other attributes' arguments too;
/// - no file in or above a member's directory, nor the directory itself, is a symbolic link;
/// - no other git repository, a submodule or an untracked one, is anywhere in the repository. The
///   checks can't see its files, and `trellis-policy rule-changes` sees only the commit it points
///   at, so under `crates/` its build script, manifest and tool settings could change beside
///   code, and anywhere else a checkout that initializes it holds files, such as a tool's
///   settings, that no check or review read.
///
/// The check doesn't see a `path` attribute that a macro writes, such as `#[$meta] mod a;` called
/// with `path = "..."`, or built from tokens that never spell `path =`. In a macro the crate
/// defines, the lints stop it: `macro_definition` lets only the crates `macro_crates` lists define
/// macros, and `suppression_policy` rejects an attribute whose path is a macro variable. A
/// declarative macro from a dependency that writes one is seen by neither, since
/// `proc_macro_crates` limits only procedural macros.
///
/// `include!` isn't read here, because the `disabled_code` lint rejects it with no exception. A
/// target's `path` in `Cargo.toml` is checked with the rest of the manifest.
///
/// # Errors
///
/// Fails if the repository's files can't be listed, or a member's file can't be read.
pub fn check(workspace: &Workspace) -> Result<Vec<Violation>, PolicyError> {
    let files = repository_files::list(workspace.root())?;
    let mut violations = nested_repositories(workspace)?;
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
                .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'));
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

/// A violation for each git repository nested anywhere in the workspace's repository.
///
/// # Errors
///
/// Fails if git fails.
fn nested_repositories(workspace: &Workspace) -> Result<Vec<Violation>, PolicyError> {
    let repositories = repository_files::nested_repositories(workspace.root())?;
    Ok(repositories
        .into_iter()
        .map(|repository| {
            Violation::new(
                repository,
                "This is another git repository, such as a submodule, so the checks can't see \
                 its files, and `rule-changes` sees only the commit it points at, not which of \
                 its files are rules. A checkout that initializes it holds files no check or \
                 review read. Copy the files you need into the repository, or depend on it \
                 through Cargo as a git dependency."
                    .to_owned(),
            )
        })
        .collect())
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

/// Adds a problem to `problems` for each attribute in `tokens`, which are in `context`, that
/// names a module's file the crate can't read its sources from, or that the check can't read.
/// An attribute's own tokens are scanned as code too, so an attribute written inside another
/// one's arguments is read, while a bare `path = ...` among those arguments isn't.
fn scan(tokens: TokenStream, context: TokenContext, depth: usize, problems: &mut Vec<String>) {
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    for (index, tree) in trees.iter().enumerate() {
        if let TokenTree::Group(group) = tree {
            match context.inside(&trees, index) {
                TokenContext::Attribute => {
                    let attribute: Vec<TokenTree> = group.stream().into_iter().collect();
                    attribute_problems(&attribute, depth, problems);
                    scan(group.stream(), TokenContext::Code, depth, problems);
                }
                inner => scan(group.stream(), inner, depth, problems),
            }
        }
    }
}

/// Adds a problem to `problems` if `attribute`, the tokens inside an attribute's brackets, is a
/// `path` attribute, or a `cfg_attr` that applies one, whose path the crate can't read its
/// sources from, or that the check can't read.
fn attribute_problems(attribute: &[TokenTree], depth: usize, problems: &mut Vec<String>) {
    match attribute {
        [TokenTree::Ident(name), TokenTree::Group(arguments)]
            if is_named(name, "cfg_attr") && arguments.delimiter() == Delimiter::Parenthesis =>
        {
            let arguments: Vec<TokenTree> = arguments.stream().into_iter().collect();
            for applied in arguments.split(is_comma).skip(1) {
                attribute_problems(applied, depth, problems);
            }
        }
        [TokenTree::Ident(name), equals, value @ ..]
            if is_named(name, "path") && is_assignment(equals) =>
        {
            if let Some(problem) = value_problem(value.first(), depth) {
                problems.push(format!("line {}: {problem}", name.span().start().line));
            }
        }
        _ => {}
    }
}

/// Whether `ident` is `name`, written plainly or as a raw identifier.
fn is_named(ident: &Ident, name: &str) -> bool {
    ident.to_string().trim_start_matches("r#") == name
}

/// Whether `tree` is a `,`.
fn is_comma(tree: &TokenTree) -> bool {
    matches!(tree, TokenTree::Punct(punct) if punct.as_char() == ',')
}

/// Whether `tree` is a lone `=`, rather than part of `==` or `=>`.
fn is_assignment(tree: &TokenTree) -> bool {
    matches!(
        tree,
        TokenTree::Punct(punct) if punct.as_char() == '=' && punct.spacing() == Spacing::Alone
    )
}

/// What is wrong with `value`, the token after `path =` in an attribute in a file `depth`
/// directories below its crate's directory, if anything.
fn value_problem(value: Option<&TokenTree>, depth: usize) -> Option<String> {
    let shown = value.map_or_else(|| "nothing".to_owned(), ToString::to_string);
    let path = match value {
        Some(TokenTree::Literal(literal)) => plain_string(&literal.to_string()),
        _ => None,
    };
    let Some(path) = path else {
        return Some(format!(
            "`path = {shown}` isn't a plain string literal, so the check can't tell which file it \
             names. Write the path as a string literal with no escapes but `\\\\`."
        ));
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
            ("m! { #[path = \"../x.rs\"] mod a; }\n", 1),
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
    fn a_path_argument_that_isnt_the_path_attribute_passes() {
        for source in [
            "fn f() { tracing::info!(path = \"/health\", \"served\"); }\n",
            "#[utoipa::path(get, path = \"/users\")]\nfn users() {}\n",
            "fn f() -> String { format!(\"{path}\", path = \"/etc/hosts\") }\n",
            "fn f() { m! { let path = \"x.rs\"; } }\n",
            "fn f() { m! { let path = \"../../../x.md\"; } }\n",
            "#[cfg_attr(test, some_macro(path = \"/x\"))]\nfn f() {}\n",
            "#[template(path = \"hello.html\")]\nstruct Hello;\n",
            "#[tracing::instrument(fields(path = %uri))]\nfn f(uri: &str) {}\n",
            "m!(path = \"../../../x.rs\");\n",
            "crate::m! { mod a; path = \"\\x2e\\x2e/x.rs\" }\n",
        ] {
            assert_eq!(
                problems(source, 0),
                Vec::<String>::new(),
                "{source:?} names no module's file"
            );
        }
    }

    #[test]
    fn paths_outside_the_crate_fail() {
        for (source, depth) in [
            ("#[path = \"../../../docs/notes.rs\"]\nmod notes;\n", 1),
            ("#[path = \"../../docs/notes.md\"]\nmod notes;\n", 0),
            ("#[cfg_attr(all(), path = \"../../x.rs\")]\nmod a;\n", 1),
            (
                "#[cfg_attr(unix, cfg_attr(test, path = \"../x.rs\"))]\nmod a;\n",
                0,
            ),
            ("#[cfg_attr(unix, inline, path = \"../x.rs\")]\nmod a;\n", 0),
            ("mod a {\n    #![path = \"..\\\\..\\\\x.rs\"]\n}\n", 0),
            ("#[path = r#\"/tmp/x.rs\"#]\nmod a;\n", 3),
            ("m! { #[path = \"../../../x.rs\"] mod a; }\n", 1),
            (
                "crate::m! { #[cfg_attr(unix, path = \"../x.rs\")] mod a; }\n",
                0,
            ),
            ("#[r#path = \"../x.rs\"]\nmod a;\n", 0),
            (
                "#[some_attr { #[path = \"../../../x.rs\"] mod a; }]\npub fn f() {}\n",
                1,
            ),
            (
                "#[cfg_attr(test, m(#[path = \"../x.rs\"] mod a;))]\nfn f() {}\n",
                0,
            ),
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
            "m! { #[path = \"notes.md\"] mod notes; }\n",
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
            "#[cfg_attr(unix, path = env!(\"X\"))]\nmod a;\n",
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
