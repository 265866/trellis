//! The `[trellis_lints]` table of `dylint.toml`, the one place the lints read their settings.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use rustc_lint::Level;
use rustc_session::Session;
use serde::Deserialize;

/// The table of `dylint.toml` the lints read, named after the library like every dylint library's.
const TABLE: &str = env!("CARGO_PKG_NAME");

/// The settings in the `[trellis_lints]` table. Every key is required and a key the table doesn't
/// know is an error, so `dylint.toml` is the only place a setting's value is written, and a
/// missing or misspelled setting fails the run rather than leaving a lint on a value nobody chose.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Config {
    /// Crates allowed to hold unsafe code (`unsafe_outside_unsafe_crate`), by crate name; `-` and
    /// `_` are interchangeable.
    pub(crate) unsafe_crates: Vec<String>,
    /// Type names that may always be written with their module (`qualified_type_path`).
    pub(crate) ambiguous_type_names: Vec<String>,
    /// Prefixes of the crate names whose `#[doc(hidden)]` items the workspace owns
    /// (`foreign_doc_hidden`).
    pub(crate) owned_crate_prefixes: Vec<String>,
    /// Words a doc comment may start with in lowercase (`doc_starts_lowercase`).
    pub(crate) lowercase_doc_words: Vec<String>,
    /// Crates that may define macros (`macro_definition`), by crate name; `-` and `_` are
    /// interchangeable.
    pub(crate) macro_crates: Vec<String>,
    /// Crates whose macros may write lint-level attributes and `#[ignore]` into the crate's code
    /// (`suppression_policy`, `disabled_code`), by crate name.
    pub(crate) trusted_macro_crates: Vec<String>,
    /// For a crate in `trusted_macro_crates`, the lints its macros may set the level of over code
    /// written in the crate's source (`suppression_policy`), with tool prefixes such as `clippy::`.
    pub(crate) trusted_macro_lints: BTreeMap<String, Vec<String>>,
    /// Lint names, with tool prefixes such as `clippy::`, and the lowest level each needs
    /// (`missing_workspace_lints`).
    pub(crate) required_lint_levels: BTreeMap<String, RequiredLevel>,
    /// The operating systems CI builds the workspace for (`disabled_code`).
    pub(crate) ci_targets: Vec<CiTarget>,
}

/// An operating system CI builds for, as `target_os` names it.
#[derive(Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum CiTarget {
    Linux,
    Macos,
    Windows,
}

impl CiTarget {
    /// Its value of `target_os`.
    pub(crate) fn os(self) -> &'static str {
        match self {
            Self::Linux => "linux",
            Self::Macos => "macos",
            Self::Windows => "windows",
        }
    }

    /// Its value of `target_family`, which is also the `cfg` name it sets, `unix` or `windows`.
    pub(crate) fn family(self) -> &'static str {
        match self {
            Self::Linux | Self::Macos => "unix",
            Self::Windows => "windows",
        }
    }
}

/// The lowest level `required_lint_levels` may ask of a lint.
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum RequiredLevel {
    Warn,
    Deny,
    Forbid,
}

impl RequiredLevel {
    pub(crate) fn level(self) -> Level {
        match self {
            Self::Warn => Level::Warn,
            Self::Deny => Level::Deny,
            Self::Forbid => Level::Forbid,
        }
    }
}

/// The table, parsed once per process. Each compiler process loads the library once and compiles
/// one crate, and `dylint_linting` reads `dylint.toml` (or `DYLINT_TOML`, which `dylint_testing`
/// sets per test) once per process too, so parsing it once per process sees every change.
/// `dylint_testing` compiles each UI fixture in a process of its own, so tests with different
/// tables don't share this.
static CONFIG: OnceLock<Option<Config>> = OnceLock::new();

/// Reads and checks the table, stopping the compilation with an error if it doesn't parse. The
/// library calls this when the compiler loads it, before any lint runs.
///
/// A missing table is only an error once a lint asks for a setting, in [`get`]: cargo also runs
/// the compiler, with the library loaded, to print the target's configuration, outside any
/// package, where no `dylint.toml` is found.
pub(crate) fn init(session: &Session) {
    dylint_linting::init_config(session);
    CONFIG.get_or_init(|| match dylint_linting::config::<Config>(TABLE) {
        Ok(config) => config,
        Err(error) => session.dcx().fatal(format!(
            "invalid `[{TABLE}]` table in `dylint.toml`: {}",
            error.to_string().trim_end()
        )),
    });
}

/// The settings, which [`init`] has read, stopping the compilation with an error if `dylint.toml`
/// has no `[trellis_lints]` table.
pub(crate) fn get(session: &Session) -> &'static Config {
    let config = CONFIG
        .get()
        .expect("`register_lints` reads the config before any lint runs");
    match config {
        Some(config) => config,
        None => session.dcx().fatal(format!(
            "no `[{TABLE}]` table in `dylint.toml`: the lints have no settings of their own, so \
             the workspace's `dylint.toml` must set every key"
        )),
    }
}

/// Whether `crates`, a list of crate names from the table, holds the crate being compiled. Cargo
/// always passes `--crate-name`; otherwise rustc names the crate after its root file. `-` and `_`
/// are interchangeable.
pub(crate) fn lists_current_crate(session: &Session, crates: &[String]) -> bool {
    let crate_name = session
        .opts
        .crate_name
        .clone()
        .unwrap_or_else(|| session.io.input.filestem().to_owned());
    crates
        .iter()
        .any(|listed| listed.replace('-', "_") == crate_name.replace('-', "_"))
}
