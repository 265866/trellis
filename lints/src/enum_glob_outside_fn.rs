use clippy_utils::diagnostics::span_lint_and_help;
use rustc_hir::Item;
use rustc_hir::def::{DefKind, Res};
use rustc_hir::def_id::DefId;
use rustc_lint::{LateContext, LateLintPass};
use rustc_lint_defs::{declare_lint, declare_lint_pass};
use rustc_span::Span;

use crate::imports::imports;

declare_lint! {
    /// Checks for glob imports of an enum's variants (`use Enum::*`) outside a function body.
    ///
    /// The Google Rust Style Guide, "Don't use glob imports", allows `use enum::*` only "in
    /// method bodies of enums, and functions that are primarily dealing with that enum". At
    /// module scope the variants' names spread through the whole file and hide where they come
    /// from. Clippy's `enum_glob_use` flags every enum glob, including the allowed ones, so this
    /// lint replaces it.
    ///
    /// Code a macro writes isn't checked. Code a proc macro copies from its caller's tokens, such
    /// as the body of a function under an attribute macro, is: its author wrote it there.
    ///
    /// ```rust,ignore
    /// use Fruit::*;
    ///
    /// fn price(fruit: Fruit) -> u64 {
    ///     match fruit {
    ///         Cherry => 2,
    ///         Plum => 50,
    ///     }
    /// }
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// fn price(fruit: Fruit) -> u64 {
    ///     use Fruit::*;
    ///
    ///     match fruit {
    ///         Cherry => 2,
    ///         Plum => 50,
    ///     }
    /// }
    /// ```
    pub ENUM_GLOB_OUTSIDE_FN,
    Warn,
    "`use Enum::*` outside a function body"
}

declare_lint_pass!(EnumGlobOutsideFn => [ENUM_GLOB_OUTSIDE_FN]);

impl<'tcx> LateLintPass<'tcx> for EnumGlobOutsideFn {
    /// Reports each enum glob import of `item`, a `use`, unless it is in a body. A `use` inside a
    /// function, method or constant body belongs to that body.
    fn check_item(&mut self, cx: &LateContext<'tcx>, item: &'tcx Item<'tcx>) {
        if item.span.from_expansion() {
            return;
        }
        let owner = cx.tcx.hir_get_parent_item(item.hir_id());
        if cx.tcx.hir_maybe_body_owned_by(owner.def_id).is_some() {
            return;
        }
        for import in imports(cx.tcx, item) {
            if import.name.is_some() {
                continue;
            }
            let Some(enum_id) = import.res.iter().flatten().find_map(|res| match res {
                Res::Def(DefKind::Enum, id) => Some(*id),
                _ => None,
            }) else {
                continue;
            };
            report(cx, import.path_span, enum_id);
        }
    }
}

/// Reports the glob import at `span` of the variants of the enum `enum_id`.
fn report(cx: &LateContext<'_>, span: Span, enum_id: DefId) {
    let name = cx.tcx.item_name(enum_id);
    span_lint_and_help(
        cx,
        ENUM_GLOB_OUTSIDE_FN,
        span,
        format!("glob import of `{name}`'s variants outside a function"),
        None,
        format!(
            "move the `use` into the functions that match on `{name}`, or name the variants as \
             `{name}::Variant`, as the Google Rust Style Guide's \"Don't use glob imports\" asks"
        ),
    );
}

#[test]
fn ui() {
    crate::ui_harness::ui_test(ENUM_GLOB_OUTSIDE_FN);
}
