//! Running the UI tests in `ui/`, whose fixtures `dylint_testing` compiles with the library loaded
//! and whose output it compares with each fixture's `.stderr` file.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use dylint_testing::ui::Test;
use rustc_lint_defs::Lint;

/// Runs the UI test in `ui/<lint>`, with every other lint in the library turned off.
pub(crate) fn ui_test(lint: &Lint) {
    ui_test_of(lint, format!("ui/{}", lint.name_lower())).run();
}

/// A UI test of the fixtures in `fixtures`, with every lint in the library but `lint` turned off,
/// for tests that also set a `dylint.toml` or more compiler flags before running it.
pub(crate) fn ui_test_of(lint: &Lint, fixtures: impl AsRef<Path>) -> Test {
    let others: Vec<String> = crate::lints()
        .into_iter()
        .filter(|other| other.name != lint.name)
        .flat_map(|other| ["-A".to_owned(), other.name_lower()])
        .collect();
    let mut test = ui_test_of_all(fixtures);
    test.rustc_flags(others);
    test
}

/// A UI test of the fixtures in `fixtures` with every lint in the library on, and the repository's
/// own `dylint.toml`.
fn ui_test_of_all(fixtures: impl AsRef<Path>) -> Test {
    temp_dir();
    let mut test = Test::src_base(env!("CARGO_PKG_NAME"), fixtures);
    test.dylint_toml(DYLINT_TOML);
    test
}

/// The repository's `dylint.toml`, which every UI test runs with unless it sets its own.
const DYLINT_TOML: &str = include_str!("../../dylint.toml");

/// The repository's `dylint.toml` with the `[trellis_lints]` key `key` set to `value`, a TOML
/// value, or removed if `value` is `None`.
pub(crate) fn dylint_toml_with(key: &str, value: Option<&str>) -> String {
    dylint_toml_with_all(&[(key, value)])
}

/// The repository's `dylint.toml` with each `[trellis_lints]` key of `settings` set to its value,
/// a TOML value, or removed if the value is `None`.
pub(crate) fn dylint_toml_with_all(settings: &[(&str, Option<&str>)]) -> String {
    let mut file: toml::Table = toml::from_str(DYLINT_TOML).expect("`dylint.toml` should parse");
    let table = file
        .get_mut(env!("CARGO_PKG_NAME"))
        .and_then(toml::Value::as_table_mut)
        .expect("`dylint.toml` should have a `[trellis_lints]` table");
    for &(key, value) in settings {
        match value {
            Some(value) => {
                let parsed: toml::Table =
                    toml::from_str(&format!("value = {value}")).expect("the value should be TOML");
                table.insert(key.to_owned(), parsed["value"].clone());
            }
            None => {
                table.remove(key);
            }
        }
    }
    toml::to_string(&file).expect("the table should serialize")
}

/// A directory of the system's temporary directory named after this checkout, which this process
/// uses as its temporary directory.
///
/// compiletest writes each fixture's output to the temporary directory under the fixture's file
/// name, so test runs from two checkouts sharing the system's temporary directory would overwrite
/// each other's output. Every UI test comes through here before `dylint_testing` reads the
/// directory, so they all see the one that belongs to this checkout. It stays outside the
/// checkout because `dylint_testing` builds its driver as a package in the temporary directory
/// whenever no driver for the lints' toolchain is cached, as on a fresh machine or after a nightly
/// bump, and Cargo refuses to build a package inside this workspace that the workspace doesn't
/// list.
pub(crate) fn temp_dir() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let mut checkout = DefaultHasher::new();
        env!("CARGO_MANIFEST_DIR").hash(&mut checkout);
        let dir = std::env::temp_dir().join(format!("trellis_lints-{:016x}", checkout.finish()));
        std::fs::create_dir_all(&dir)
            .expect("the UI tests' temporary directory should be creatable");
        for name in ["TMP", "TEMP", "TMPDIR"] {
            // SAFETY: setting a variable races only with a read of the environment that bypasses
            // Rust's environment lock. Tests that run fixtures wait on `DIR` before reading it,
            // the other tests don't read it, and nothing in this process reads it outside Rust's
            // functions. `dylint_testing` sets `DYLINT_TOML` the same way.
            unsafe { std::env::set_var(name, &dir) };
        }
        dir
    })
}

/// Flags like those Cargo passes for `[workspace.lints]`, then the custom lints the UI tests turn
/// off set back to their default level, as they are in a real run.
pub(crate) fn workspace_flags() -> Vec<String> {
    [
        "--warn=rust_2018_idioms",
        "--warn=missing_docs",
        "--warn=unused_qualifications",
        "--deny=unsafe_code",
        "--warn=clippy::pedantic",
        "--warn=clippy::unwrap_used",
        "-W",
        "suppression_policy",
        "-W",
        "disabled_code",
    ]
    .map(str::to_owned)
    .to_vec()
}

/// Idiomatic code, a library crate with docs, derives, a trait impl, a
/// constructor that can fail and unit tests, draws no report from any lint, in an ordinary build
/// and in a test build.
#[test]
fn ui_clean() {
    for build in [&["--crate-type=lib"][..], &["--test"]] {
        ui_test_of_all("ui/clean")
            .rustc_flags(["--edition=2024"])
            .rustc_flags(build)
            .rustc_flags(workspace_flags())
            .run();
    }
}

/// A key the `[trellis_lints]` table doesn't know, such as a misspelling, stops the compilation
/// with an error that names it and lists the keys there are.
#[test]
fn ui_config_unknown_key() {
    ui_test_of_all("ui/config_unknown_key")
        .dylint_toml(dylint_toml_with("unsafe_crate", Some(r#"["app_sys"]"#)))
        .run();
}

/// A table without one of its keys stops the compilation with an error that names it: the lints
/// have no defaults, so `dylint.toml` is the one place a setting is written.
#[test]
fn ui_config_missing_key() {
    ui_test_of_all("ui/config_missing_key")
        .dylint_toml(dylint_toml_with("unsafe_crates", None))
        .run();
}

/// A `dylint.toml` without the `[trellis_lints]` table stops the compilation once a lint asks for
/// a setting, with an error that says the table is missing.
#[test]
fn ui_config_missing_table() {
    ui_test_of_all("ui/config_missing_table")
        .dylint_toml("")
        .run();
}

/// `trusted_macro_lints` may list neither a lint group nor a crate that `trusted_macro_crates`
/// doesn't list.
#[test]
fn ui_config_trusted_macro_lints() {
    ui_test_of(
        crate::suppression_policy::SUPPRESSION_POLICY,
        "ui/config_trusted_macro_lints",
    )
    .dylint_toml(dylint_toml_with(
        "trusted_macro_lints",
        Some(r#"{ std = ["clippy::style", "dead_code"], quiet = ["dead_code"] }"#),
    ))
    .run();
}
