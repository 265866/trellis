use std::collections::HashMap;

use clippy_utils::diagnostics::span_lint_and_help;
use rustc_ast::visit::{self, Visitor};
use rustc_ast::{AttrId, Attribute, Crate};
use rustc_attr_ir::find_attr;
use rustc_hir::def_id::LOCAL_CRATE;
use rustc_hir::{Item, ItemKind};
use rustc_lint::{LateContext, LateLintPass};
use rustc_lint_defs::{declare_lint, declare_lint_pass};
use rustc_middle::ty::TyCtxt;
use rustc_span::{ExpnId, ExpnKind, Span, sym};
#[cfg(test)]
use std::path::Path;

declare_lint! {
    /// Checks for a `static` placed in a section the loader runs before `main`: `.preinit_array`,
    /// `.init_array` and `.ctors` on ELF targets; on Apple targets a `__mod_init_func` section, or
    /// a section of type `mod_init_funcs`, in any segment; and on Windows the CRT's `.CRT$XI*` and
    /// `.CRT$XC*` initializers, its `.CRT$XD*` dynamic TLS initializers and its `.CRT$XL*` TLS
    /// callbacks, which run as the process starts. Each may have any suffix. A `cfg_attr` gives
    /// each target only its own branch, so every platform's names are matched: each target's run
    /// of `just dylint`, which the template's CI makes on every target's runner, catches the
    /// branch that target compiles.
    ///
    /// The Google Rust Style Guide: don't run code before `main()`. Such code runs in no order
    /// anyone chose, before logging, arguments or the runtime are set up. The `ctor`, `inventory`
    /// and `linkme` crates are banned in `deny.toml` for this reason; this lint catches the
    /// constructors that a macro from a crate let in past the ban writes into the workspace's own
    /// code, such as the `inventory::submit!` that GPUI's `actions!` and `#[derive(Action)]`
    /// expand to for an action without `#[action(no_register)]`. Cargo runs the lints over
    /// workspace members only, so a dependency's own constructors aren't reported. Statics in
    /// other sections, and in none, are left alone.
    ///
    /// A static a macro writes is reported at the outermost macro call in the source, naming that
    /// macro. The `constructor_macros` list in `dylint.toml` names the macros allowed to place
    /// constructors, as the crate that defines them and their name, such as `gpui::actions` or
    /// `gpui_macros::Action` for a derive that `gpui` re-exports. A static passes if a macro it
    /// was expanded from is listed and no macro inside the first listed one is defined in the
    /// workspace, in the crate itself or one `macro_crates` lists: a workspace macro there could
    /// write any constructor into a listed macro that takes tokens. The help names the outermost
    /// macro inside every workspace macro, so a wrapper around `gpui::actions!` is told to list
    /// `gpui::actions`. The list has no default, and the template ships it empty. The lint has no
    /// exceptions, since an `expect` around a macro call would let it place any constructor, so
    /// listing a macro, a rule change, is the only way past it:
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
    ///
    /// The lint levels that the macro writing the static writes on the items around it, such as
    /// the `allow` on the `const _` that `inventory::submit!` writes, are this lint's to judge too,
    /// as long as none can turn it off: each names only lints, none of them a group or this lint,
    /// and the lint is on at the crate root. `suppression_policy` takes them as a trusted crate's
    /// once the static passes, so a listed macro needs no entry in `trusted_macro_crates`, and
    /// otherwise leaves them to this lint's report. It checks any other as the macro's.
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
        let config = crate::config::get(cx.tcx.sess);
        let macros = expanded_from(cx.tcx, item.span, &config.macro_crates);
        if is_allowed(&macros, &config.constructor_macros) {
            return;
        }
        let Some(outermost) = macros.last() else {
            span_lint_and_help(
                cx,
                CODE_BEFORE_MAIN,
                item.span,
                format!("a `static` in `{section}`, which runs code before `main`"),
                None,
                "call the code from `main` instead",
            );
            return;
        };
        let own = macros.iter().position(|expansion| expansion.in_workspace);
        let listable = &macros[..own.unwrap_or(macros.len())];
        let help = match (listable.last(), own) {
            (Some(framework), _) => format!(
                "call the code from `main` instead, or, if the framework needs it, list \
                 \"{}\" in `constructor_macros` under `[trellis_lints]` in `dylint.toml`",
                framework.path
            ),
            (None, Some(own)) => format!(
                "call the code from `main` instead: the workspace's own macro `{}` writes it",
                macros[own].path
            ),
            (None, None) => "call the code from `main` instead".to_owned(),
        };
        span_lint_and_help(
            cx,
            CODE_BEFORE_MAIN,
            outermost.call_site,
            format!(
                "`{}` places a `static` in `{section}`, which runs code before `main`",
                outermost.path
            ),
            None,
            help,
        );
    }
}

/// Whether `constructor_macros`, as `allowed`, lets a static expanded from `macros`, innermost
/// first, pass: it must list one of them, and no macro inside the first one it lists may be
/// defined in the workspace. A workspace macro there may write any tokens, so a listed framework
/// macro that takes tokens, such as one that wraps an app's items, can't pass a constructor the
/// workspace's own macro writes inside it.
fn is_allowed(macros: &[Expansion], allowed: &[String]) -> bool {
    macros
        .iter()
        .position(|expansion| allowed.contains(&expansion.path))
        .is_some_and(|listed| {
            !macros[..listed]
                .iter()
                .any(|expansion| expansion.in_workspace)
        })
}

/// For each lint-level attribute of `krate` on an item that holds a static in a constructor
/// section, the item itself included, the expansion that wrote each such static and whether
/// `constructor_macros` lets it pass. `suppression_policy` leaves a lint level that the static's
/// own expansion wrote to this lint, as [`crate::attribute_origin::origin`] says.
pub(crate) fn constructors_under(krate: &Crate) -> HashMap<(AttrId, Span), Vec<(ExpnId, bool)>> {
    rustc_middle::ty::tls::with(|tcx| {
        let mut under = ConstructorsUnder {
            tcx,
            found: HashMap::new(),
        };
        visit::walk_crate(&mut under, krate);
        under.found
    })
}

/// Walks the expanded crate, finding the constructors under each item's lint levels.
struct ConstructorsUnder<'tcx> {
    tcx: TyCtxt<'tcx>,
    found: HashMap<(AttrId, Span), Vec<(ExpnId, bool)>>,
}

impl<'ast> Visitor<'ast> for ConstructorsUnder<'_> {
    fn visit_item(&mut self, item: &'ast rustc_ast::Item) {
        let levels: Vec<&Attribute> = item
            .attrs
            .iter()
            .filter(|attribute| is_lint_level(attribute))
            .collect();
        if !levels.is_empty() {
            let mut statics = ConstructorStatics(Vec::new());
            statics.visit_item(item);
            let config = crate::config::get(self.tcx.sess);
            let constructors: Vec<(ExpnId, bool)> = statics
                .0
                .into_iter()
                .map(|span| {
                    let macros = expanded_from(self.tcx, span, &config.macro_crates);
                    (
                        span.ctxt().outer_expn(),
                        is_allowed(&macros, &config.constructor_macros),
                    )
                })
                .collect();
            if !constructors.is_empty() {
                for attribute in levels {
                    self.found
                        .insert((attribute.id, attribute.span), constructors.clone());
                }
            }
        }
        visit::walk_item(self, item);
    }
}

/// The span of each static in a constructor section in the items a walk visits.
struct ConstructorStatics(Vec<Span>);

impl<'ast> Visitor<'ast> for ConstructorStatics {
    fn visit_item(&mut self, item: &'ast rustc_ast::Item) {
        if matches!(item.kind, rustc_ast::ItemKind::Static(..))
            && item
                .attrs
                .iter()
                .filter(|attribute| attribute.has_name(sym::link_section))
                .filter_map(Attribute::value_str)
                .any(|section| is_constructor_section(section.as_str()))
        {
            self.0.push(item.span);
        }
        visit::walk_item(self, item);
    }
}

/// Whether `attribute` sets lint levels or is one of clippy's own attributes.
fn is_lint_level(attribute: &Attribute) -> bool {
    let path = crate::attribute_origin::path_text(attribute);
    matches!(
        path.as_str(),
        "allow" | "warn" | "expect" | "deny" | "forbid"
    ) || path.starts_with("clippy::")
}

/// Whether a static in `section` runs before `main` on some target: ELF's `.preinit_array`,
/// `.init_array` and `.ctors`; on Apple targets a `__mod_init_func` section, or any section of
/// type `mod_init_funcs`, in any segment; and on Windows the CRT's `.CRT$XI*` and `.CRT$XC*`
/// initializers, its `.CRT$XD*` dynamic TLS initializers and its `.CRT$XL*` TLS callbacks. Each
/// may have any priority or other suffix the linker accepts. A Mach-O name is
/// `segment,section[,type[,attributes]]`, and LLVM trims the space around each part.
fn is_constructor_section(section: &str) -> bool {
    let parts: Vec<&str> = section.split(',').map(str::trim).collect();
    let mach_o =
        parts.get(1) == Some(&"__mod_init_func") || parts.get(2) == Some(&"mod_init_funcs");
    let has_prefix = |prefix: &str| {
        section
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('.'))
    };
    mach_o
        || has_prefix(".preinit_array")
        || has_prefix(".init_array")
        || has_prefix(".ctors")
        || [".CRT$XI", ".CRT$XC", ".CRT$XD", ".CRT$XL"]
            .iter()
            .any(|prefix| section.starts_with(prefix))
}

/// A macro a static was expanded from.
struct Expansion {
    /// Where it was called.
    call_site: Span,
    /// Its path as `constructor_macros` spells it: the name of the crate that defines it, then its
    /// own name. A re-export doesn't change either, and neither does the module a
    /// `#[macro_export]` macro is written in.
    path: String,
    /// Whether a workspace crate defines it: the crate being compiled, or one `macro_crates`
    /// lists. A macro whose definition the compiler doesn't name counts as one, so it never lets
    /// a listed macro outside it pass.
    in_workspace: bool,
}

/// The macros `span` was expanded from, innermost first. The last one's call site is in the
/// source.
fn expanded_from(tcx: TyCtxt<'_>, span: Span, macro_crates: &[String]) -> Vec<Expansion> {
    span.macro_backtrace()
        .filter_map(|expansion| {
            let ExpnKind::Macro(_, written) = expansion.kind else {
                return None;
            };
            let defined = expansion.macro_def_id.and_then(|id| {
                let krate = tcx.crate_name(id.krate);
                let in_workspace = id.krate == LOCAL_CRATE
                    || macro_crates
                        .iter()
                        .any(|listed| listed.replace('-', "_") == krate.as_str());
                Some((format!("{krate}::{}", tcx.opt_item_name(id)?), in_workspace))
            });
            let (path, in_workspace) = defined.unwrap_or_else(|| (written.to_string(), true));
            Some(Expansion {
                call_site: expansion.call_site,
                path,
                in_workspace,
            })
        })
        .collect()
}

#[test]
fn ui() {
    crate::ui_harness::ui_test(CODE_BEFORE_MAIN);
}

/// The fixture of statics that a dependency's macros write, several items deep as GPUI's
/// `actions!` and `#[derive(Action)]` write them, under the `allow` that `inventory::submit!`
/// writes around them.
#[cfg(test)]
const EXTERNAL_MACRO: &str = "ui/code_before_main_external_macro";

/// A UI test of `fixtures` with `suppression_policy` on as well as this lint, which a project
/// runs together: the lint levels a constructor macro writes around its static are this lint's to
/// judge.
#[cfg(test)]
fn ui_test_with_suppression_policy(fixtures: impl AsRef<Path>) -> dylint_testing::ui::Test {
    crate::ui_harness::ui_test_of_lints(
        &[
            CODE_BEFORE_MAIN,
            crate::suppression_policy::SUPPRESSION_POLICY,
        ],
        fixtures,
    )
}

/// A static that a macro defined in another crate writes is reported at the macro call in the
/// source, naming the macro as `constructor_macros` spells it, and by this lint alone: the
/// `allow` the macro writes around it goes once the call does.
#[test]
fn ui_external_macro() {
    ui_test_with_suppression_policy(EXTERNAL_MACRO).run();
}

/// Paths that name the macros some other way don't match: the derive by the facade that re-exports
/// it, and the outer macro without its crate. The statics are still reported.
#[test]
fn ui_external_macro_unmatched() {
    ui_test_with_suppression_policy(EXTERNAL_MACRO)
        .dylint_toml(crate::ui_harness::dylint_toml_with(
            "constructor_macros",
            Some(r#"["gui::Action", "actions"]"#),
        ))
        .run();
}

/// Listing the macros the source calls passes their statics, as does listing the macro inside
/// them that writes the static, since any macro a static was expanded from counts, until a
/// workspace macro is inside it: the statics the workspace's own macro writes are still reported,
/// even inside a listed framework macro that takes them as tokens. The listing alone passes the
/// `allow` the registry writes around its static too, with `trusted_macro_crates` unchanged, but
/// not over code written in the source unless `trusted_macro_lints` lists its lint. Each fixture's
/// expected output for a variant here is its `.<variant>.stderr`, and none means nothing is
/// reported.
#[test]
fn ui_external_macro_listed() {
    for (variant, listed) in [
        (
            "listed",
            r#"["gui::actions", "actions_derive::Action", "fw::app", "fw_macros::app"]"#,
        ),
        ("submit", r#"["registry::submit", "fw_macros::app"]"#),
    ] {
        let fixture = crate::ui_harness::temp_dir().join("code_before_main_listed");
        copy_fixture(Path::new(EXTERNAL_MACRO), &fixture, variant);
        ui_test_with_suppression_policy(&fixture)
            .dylint_toml(crate::ui_harness::dylint_toml_with(
                "constructor_macros",
                Some(listed),
            ))
            .run();
        std::fs::remove_dir_all(&fixture).expect("the fixture copy should be removable");
    }
}

/// A dependency's lint level that could turn this lint off, by a group such as `warnings` or by
/// its name, isn't left to it: `suppression_policy` flags each, so the macro call is always
/// reported, as it is when the level hides the caller's own code too.
#[test]
fn ui_constructor_probes() {
    ui_test_with_suppression_policy("ui/code_before_main_probes").run();
}

/// With this lint turned off on the command line, nothing would report the statics, so
/// `suppression_policy` flags the `allow` the registry writes around them, as for any untrusted
/// crate's macro. Each fixture's expected output here is its `.off.stderr`.
#[test]
fn ui_external_macro_turned_off() {
    let fixture = crate::ui_harness::temp_dir().join("code_before_main_off");
    copy_fixture(Path::new(EXTERNAL_MACRO), &fixture, "off");
    ui_test_with_suppression_policy(&fixture)
        .rustc_flags(["-A", "code_before_main"])
        .run();
    std::fs::remove_dir_all(&fixture).expect("the fixture copy should be removable");
}

/// A lint level that a macro from a crate `macro_crates` lists writes around its constructor is
/// the workspace's own, which `suppression_policy` flags as it flags any: only a dependency's is
/// left to this lint. Each fixture's expected output here is its `.workspace.stderr`.
#[test]
fn ui_external_macro_workspace_crate() {
    let fixture = crate::ui_harness::temp_dir().join("code_before_main_workspace");
    copy_fixture(Path::new(EXTERNAL_MACRO), &fixture, "workspace");
    ui_test_with_suppression_policy(&fixture)
        .dylint_toml(crate::ui_harness::dylint_toml_with(
            "macro_crates",
            Some(r#"["registry"]"#),
        ))
        .run();
    std::fs::remove_dir_all(&fixture).expect("the fixture copy should be removable");
}

/// Copies the fixture directory `from`, its `auxiliary` directory included, to `to`, with each
/// fixture's `<name>.<variant>.stderr` as its expected output, `<name>.stderr`, in place of the
/// one it has.
#[cfg(test)]
fn copy_fixture(from: &Path, to: &Path, variant: &str) {
    std::fs::create_dir_all(to).expect("the temporary directory should be writable");
    for entry in std::fs::read_dir(from).expect("the fixture should be readable") {
        let path = entry.expect("the fixture should be readable").path();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("a fixture file has a UTF-8 name");
        if path.is_dir() {
            copy_fixture(&path, &to.join(name), variant);
        } else if let Some(stem) = name.strip_suffix(&format!(".{variant}.stderr")) {
            std::fs::copy(&path, to.join(format!("{stem}.stderr")))
                .expect("the fixture should be copyable");
        } else if !name.ends_with(".stderr") {
            std::fs::copy(&path, to.join(name)).expect("the fixture should be copyable");
        }
    }
}
