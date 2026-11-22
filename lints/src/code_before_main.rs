use clippy_utils::diagnostics::span_lint_and_help;
use rustc_attr_ir::find_attr;
use rustc_hir::{Item, ItemKind};
use rustc_lint::{LateContext, LateLintPass};
use rustc_lint_defs::{declare_lint, declare_lint_pass};
use rustc_span::{ExpnKind, Span};
#[cfg(test)]
use std::path::Path;

declare_lint! {
    /// Checks for a `static` placed in a section the loader runs before `main`: `.init_array` and
    /// `.ctors` on ELF targets, `__DATA,__mod_init_func` on Apple targets and `.CRT$XC*` on
    /// Windows, each with any suffix. Each target's build sees only its own `cfg_attr` branch, so
    /// every platform's names are matched, and a Linux build catches all of them.
    ///
    /// The Google Rust Style Guide: don't run code before `main()`. Such code runs in no order
    /// anyone chose, before logging, arguments or the runtime are set up. The `ctor`, `inventory`
    /// and `linkme` crates are banned in `deny.toml` for this reason; this lint catches the
    /// constructors that a macro from a crate let in past the ban writes into the workspace's own
    /// code, such as the `inventory::submit!` that GPUI's `actions!` and `#[derive(Action)]`
    /// expand to. Cargo runs the lints over workspace members only, so a dependency's own
    /// constructors aren't reported. Statics in other sections, and in none, are left alone.
    ///
    /// A static a macro writes is reported at the outermost macro call in the source, naming that
    /// macro. The `constructor_macros` list in `dylint.toml` names the macros allowed to place
    /// constructors, as the crate that defines them and their name, such as `gpui::actions` or
    /// `gpui_macros::Action` for a derive that `gpui` re-exports. A static passes if any macro it
    /// was expanded from is listed. The list has no default, and the template ships it empty. The
    /// lint has no exceptions, since an `expect` around a macro call would let it place any
    /// constructor, so listing a macro, a rule change, is the only way past it:
    ///
    /// ```toml
    /// [trellis_lints]
    /// constructor_macros = ["gpui::actions", "gpui_macros::Action"]
    /// ```
    ///
    /// ```rust,ignore
    /// #[used]
    /// #[unsafe(link_section = ".init_array")]
    /// static INIT: extern "C" fn() = init;
    /// ```
    ///
    /// Use instead a call at the start of `main`:
    ///
    /// ```rust,ignore
    /// fn main() {
    ///     init();
    /// }
    /// ```
    pub CODE_BEFORE_MAIN,
    Warn,
    "a `static` in a section that runs code before `main`"
}

declare_lint_pass!(CodeBeforeMain => [CODE_BEFORE_MAIN]);

impl<'tcx> LateLintPass<'tcx> for CodeBeforeMain {
    fn check_item(&mut self, cx: &LateContext<'tcx>, item: &'tcx Item<'tcx>) {
        if !matches!(item.kind, ItemKind::Static(..)) {
            return;
        }
        let Some(section) = find_attr!(
            cx.tcx,
            item.owner_id.to_def_id(),
            LinkSection { name } => *name
        ) else {
            return;
        };
        if !is_constructor_section(section.as_str()) {
            return;
        }
        let macros = expanded_from(cx, item.span);
        let allowed = &crate::config::get(cx.tcx.sess).constructor_macros;
        if macros.iter().any(|(_, path)| allowed.contains(path)) {
            return;
        }
        match macros.last() {
            Some((call, path)) => span_lint_and_help(
                cx,
                CODE_BEFORE_MAIN,
                *call,
                format!("`{path}` places a `static` in `{section}`, which runs code before `main`"),
                None,
                format!(
                    "call the code from `main` instead, or, if the framework needs it, list \
                     \"{path}\" in `constructor_macros` under `[trellis_lints]` in `dylint.toml`"
                ),
            ),
            None => span_lint_and_help(
                cx,
                CODE_BEFORE_MAIN,
                item.span,
                format!("a `static` in `{section}`, which runs code before `main`"),
                None,
                "call the code from `main` instead",
            ),
        }
    }
}

/// Whether a static in `section` runs before `main` on some target: ELF's `.init_array` and
/// `.ctors`, Apple's `__DATA,__mod_init_func` and the Windows CRT's `.CRT$XC*` initializers, with
/// any priority or other suffix the linker accepts.
fn is_constructor_section(section: &str) -> bool {
    let has_prefix = |prefix: &str, separator: char| {
        section
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with(separator))
    };
    has_prefix(".init_array", '.')
        || has_prefix(".ctors", '.')
        || has_prefix("__DATA,__mod_init_func", ',')
        || section.starts_with(".CRT$XC")
}

/// The macros `span` was expanded from, innermost first, each with its call site and its path as
/// `constructor_macros` spells it: the name of the crate that defines it, then its own name. A
/// re-export doesn't change either, and neither does the module a `#[macro_export]` macro is
/// written in. The last one's call site is in the source.
fn expanded_from(cx: &LateContext<'_>, span: Span) -> Vec<(Span, String)> {
    span.macro_backtrace()
        .filter_map(|expansion| {
            let ExpnKind::Macro(_, written) = expansion.kind else {
                return None;
            };
            let path = expansion
                .macro_def_id
                .and_then(|id| Some((cx.tcx.crate_name(id.krate), cx.tcx.opt_item_name(id)?)))
                .map_or_else(
                    || written.to_string(),
                    |(krate, name)| format!("{krate}::{name}"),
                );
            Some((expansion.call_site, path))
        })
        .collect()
}

#[test]
fn ui() {
    crate::ui_harness::ui_test(CODE_BEFORE_MAIN);
}

/// The fixture of statics that a dependency's macros write, several items deep as GPUI's
/// `actions!` and `#[derive(Action)]` write them.
#[cfg(test)]
const EXTERNAL_MACRO: &str = "ui/code_before_main_external_macro";

/// A static that a macro defined in another crate writes is reported at the macro call in the
/// source, naming the macro as `constructor_macros` spells it.
#[test]
fn ui_external_macro() {
    crate::ui_harness::ui_test_of(CODE_BEFORE_MAIN, EXTERNAL_MACRO).run();
}

/// Paths that name the macros some other way don't match: the derive by the facade that re-exports
/// it, and the outer macro without its crate. The statics are still reported.
#[test]
fn ui_external_macro_unmatched() {
    crate::ui_harness::ui_test_of(CODE_BEFORE_MAIN, EXTERNAL_MACRO)
        .dylint_toml(crate::ui_harness::dylint_toml_with(
            "constructor_macros",
            Some(r#"["gui::Action", "actions"]"#),
        ))
        .run();
}

/// Listing the macros the source calls passes their statics, as does listing the macro inside
/// them that writes the static, since any macro a static was expanded from counts.
#[test]
fn ui_external_macro_listed() {
    for listed in [
        r#"["gui::actions", "actions_derive::Action"]"#,
        r#"["registry::submit"]"#,
    ] {
        let fixture = crate::ui_harness::temp_dir().join("code_before_main_listed");
        copy_without_stderr(Path::new(EXTERNAL_MACRO), &fixture);
        crate::ui_harness::ui_test_of(CODE_BEFORE_MAIN, &fixture)
            .dylint_toml(crate::ui_harness::dylint_toml_with(
                "constructor_macros",
                Some(listed),
            ))
            .run();
        std::fs::remove_dir_all(&fixture).expect("the fixture copy should be removable");
    }
}

/// Copies the fixture directory `from`, its `auxiliary` directory included, to `to`, leaving out
/// the expected output so that the test passes only if nothing is reported.
#[cfg(test)]
fn copy_without_stderr(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("the temporary directory should be writable");
    for entry in std::fs::read_dir(from).expect("the fixture should be readable") {
        let path = entry.expect("the fixture should be readable").path();
        let target = to.join(path.file_name().expect("a directory entry has a name"));
        if path.is_dir() {
            copy_without_stderr(&path, &target);
        } else if path
            .extension()
            .is_none_or(|extension| extension != "stderr")
        {
            std::fs::copy(&path, &target).expect("the fixture should be copyable");
        }
    }
}
