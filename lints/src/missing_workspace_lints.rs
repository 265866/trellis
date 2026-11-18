use rustc_ast::Crate;
use rustc_lint::{EarlyContext, EarlyLintPass, Level, LintContext, LintId};
use rustc_lint_defs::{declare_lint, declare_lint_pass};
use rustc_structures::LintName;

use crate::command_line_lint::span_lint_from_command_line;

declare_lint! {
    /// Checks that the crate was compiled with the workspace's lint levels: that the command
    /// line gives each lint in `required_lint_levels` at least its required level.
    ///
    /// Cargo passes a crate's `[lints]` table to the compiler as `-W`, `-D` and `-A` flags, so a
    /// crate without `[lints] workspace = true` compiles with none of `[workspace.lints]`, and
    /// clippy and every other check pass on it. The lint also catches a level lowered by later
    /// flags, such as `-A clippy::unwrap_used`, `-A warnings` or `--cap-lints` added through
    /// `RUSTFLAGS` or `.cargo/config.toml`. It checks a few sentinels from `[workspace.lints]`
    /// rather than the whole table, which `dylint.toml` lists, as trellis's does:
    ///
    /// ```toml
    /// [trellis_lints.required_lint_levels]
    /// missing_docs = "warn"
    /// unsafe_code = "deny"
    /// "clippy::pedantic" = "warn"
    /// "clippy::unwrap_used" = "warn"
    /// suppression_policy = "warn"
    /// disabled_code = "warn"
    /// ```
    ///
    /// The required level is a minimum, `warn`, `deny` or `forbid`. A flag lowers a lint it
    /// names or a group the compiler knows contains it. clippy's groups aren't known outside
    /// clippy, so the lint also requires clippy's default groups, `clippy::correctness`,
    /// `suspicious`, `style`, `complexity` and `perf`, at `warn` or above, and reads
    /// `-A clippy::all` as lowering each of them. A flag naming one clippy lint that isn't a
    /// sentinel, such as `-A clippy::needless_return`, isn't caught.
    ///
    /// ```toml
    /// [package]
    /// name = "app_rules"
    /// ```
    ///
    /// Use instead:
    ///
    /// ```toml
    /// [package]
    /// name = "app_rules"
    ///
    /// [lints]
    /// workspace = true
    /// ```
    ///
    /// Like `suppression_policy`, it reports from the crate root, so attributes can't silence it;
    /// only `-A missing_workspace_lints` on the command line turns it off.
    pub MISSING_WORKSPACE_LINTS,
    Warn,
    "a crate compiled without the workspace's lint levels"
}

declare_lint_pass!(MissingWorkspaceLints => [MISSING_WORKSPACE_LINTS]);

/// clippy's lint groups that are on by default, which `clippy::all` holds. The compiler doesn't
/// know clippy's groups outside clippy, so this list stands in for them: no flag may turn one
/// below `warn`, whatever `required_lint_levels` says.
const CLIPPY_DEFAULT_GROUPS: &[&str] = &[
    "clippy::complexity",
    "clippy::correctness",
    "clippy::perf",
    "clippy::style",
    "clippy::suspicious",
];

impl EarlyLintPass for MissingWorkspaceLints {
    fn check_crate(&mut self, cx: &EarlyContext<'_>, krate: &Crate) {
        let required = &crate::config::get(cx.sess()).required_lint_levels;
        let clippy_groups = CLIPPY_DEFAULT_GROUPS
            .iter()
            .filter(|&&group| !required.contains_key(group))
            .map(|&group| (group, Level::Warn));
        let shortfalls: Vec<String> = required
            .iter()
            .map(|(name, required)| (name.as_str(), required.level()))
            .chain(clippy_groups)
            .filter_map(|(name, required)| {
                let level = command_line_level(cx, name);
                (level < required).then(|| {
                    format!(
                        "`{name}` is `{}`, and the workspace needs at least `{}`",
                        level.as_str(),
                        required.as_str()
                    )
                })
            })
            .collect();
        if shortfalls.is_empty() {
            return;
        }
        span_lint_from_command_line(
            cx,
            MISSING_WORKSPACE_LINTS,
            krate.spans.inner_span.shrink_to_lo(),
            &format!(
                "crate compiled without the workspace lint levels: {}",
                shortfalls.join("; ")
            ),
            &[
                "add `[lints]` with `workspace = true` to the crate's `Cargo.toml`, and don't lower \
               lints through `RUSTFLAGS` or `.cargo/config.toml`",
            ],
        );
    }
}

/// The level the command line gives the lint named `name` before attributes apply: the last flag
/// that names it or a group containing it, `-A warnings` and `--cap-lints`, else its default,
/// which for one of clippy's default groups is `warn`.
fn command_line_level(cx: &EarlyContext<'_>, name: &str) -> Level {
    let session = cx.sess();
    let store = rustc_lint::unerased_lint_store(session);
    let lint: Option<LintId> = store
        .find_lints(LintName::from_str(name))
        .and_then(|lints| (lints.len() == 1).then(|| lints[0]));
    let is_clippy_default_group = CLIPPY_DEFAULT_GROUPS.contains(&name);
    let names_lint = |flag: &str| {
        flag == name
            || (is_clippy_default_group && flag == "clippy::all")
            || lint.is_some_and(|lint| {
                store
                    .find_lints(LintName::from_str(flag))
                    .is_some_and(|lints| lints.contains(&lint))
            })
    };
    let mut level = None;
    let mut warnings = None;
    for (flag, flag_level) in &session.opts.lint_opts {
        if flag == "warnings" {
            warnings = Some(*flag_level);
        }
        if names_lint(flag) && !matches!(level, Some(Level::Forbid | Level::ForceWarn)) {
            level = Some(*flag_level);
        }
    }
    let default = if is_clippy_default_group {
        Level::Warn
    } else {
        Level::Allow
    };
    let mut level = level
        .unwrap_or_else(|| lint.map_or(default, |lint| lint.lint.default_level(session.edition())));
    if level == Level::Warn && warnings == Some(Level::Allow) {
        level = Level::Allow;
    }
    level.min(session.opts.lint_cap.unwrap_or(Level::Forbid))
}

/// Without the workspace's flags, every sentinel is missing.
#[test]
fn ui() {
    crate::ui_harness::ui_test(MISSING_WORKSPACE_LINTS);
}

/// The flags Cargo passes for `[workspace.lints]`, in a fixture without expected warnings, so the
/// test passes only if nothing is reported.
#[test]
fn ui_workspace_flags() {
    crate::ui_harness::ui_test_of(
        MISSING_WORKSPACE_LINTS,
        "ui/missing_workspace_lints_present",
    )
    .rustc_flags(crate::ui_harness::workspace_flags())
    .run();
}

/// The workspace's flags, then flags that lower some of them again, and a `dylint.toml` that
/// raises one requirement.
#[test]
fn ui_lowered() {
    let mut flags = crate::ui_harness::workspace_flags();
    flags.extend(
        [
            "-A",
            "clippy::unwrap_used",
            "-A",
            "rust_2018_idioms",
            "-A",
            "clippy::all",
        ]
        .map(str::to_owned),
    );
    crate::ui_harness::ui_test_of(
        MISSING_WORKSPACE_LINTS,
        "ui/missing_workspace_lints_lowered",
    )
    .rustc_flags(flags)
    .dylint_toml(crate::ui_harness::dylint_toml_with(
        "required_lint_levels",
        Some(
            r#"{ missing_docs = "warn", "clippy::unwrap_used" = "warn", elided_lifetimes_in_paths = "warn", unused_qualifications = "deny" }"#,
        ),
    ))
    .run();
}

/// `-A warnings` would silence the lint's own report, so it reports a hard error instead.
#[test]
fn ui_warnings_allowed() {
    let mut flags = crate::ui_harness::workspace_flags();
    flags.extend(["-A", "warnings"].map(str::to_owned));
    crate::ui_harness::ui_test_of(
        MISSING_WORKSPACE_LINTS,
        "ui/missing_workspace_lints_warnings_allowed",
    )
    .rustc_flags(flags)
    .run();
}
