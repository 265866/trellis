use clippy_utils::diagnostics::span_lint_and_help;
use rustc_hir::def::{DefKind, Res};
use rustc_hir::def_id::DefId;
use rustc_hir::{Expr, ExprKind, ItemId, ItemKind, Node, QPath, StmtKind};
use rustc_lint::{LateContext, LateLintPass};
use rustc_lint_defs::{declare_lint, declare_lint_pass};
use rustc_span::Symbol;

use crate::imports::imports;

declare_lint! {
    /// Checks for calls to a free function from another module that a `use` brought into scope
    /// by name, such as `swap(..)` after `use std::mem::swap`.
    ///
    /// The Google Rust Style Guide, "Qualify calls to top-level functions with a module name":
    /// import the module and call `modulename::functionname()`. Readers can then assume that an
    /// unqualified call is to a function of the same module.
    ///
    /// Only explicit, single-item imports count, renamed ones included. Calls to functions of the
    /// same module, prelude functions such as `drop`, functions reached through a glob import
    /// (`use super::*` in tests), and foreign (`extern`) functions aren't checked.
    ///
    /// Code a macro writes isn't checked. Code a proc macro copies from its caller's tokens, such
    /// as the body of a function under an attribute macro, is: its author wrote it there.
    ///
    /// ```rust,ignore
    /// use std::mem::swap;
    ///
    /// swap(&mut left, &mut right);
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// use std::mem;
    ///
    /// mem::swap(&mut left, &mut right);
    /// ```
    pub QUALIFIED_FUNCTION_CALLS,
    Warn,
    "a call to an imported top-level function without its module"
}

declare_lint_pass!(QualifiedFunctionCalls => [QUALIFIED_FUNCTION_CALLS]);

impl<'tcx> LateLintPass<'tcx> for QualifiedFunctionCalls {
    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        if expr.span.from_expansion() {
            return;
        }
        let ExprKind::Call(callee, _) = expr.kind else {
            return;
        };
        let ExprKind::Path(QPath::Resolved(None, path)) = callee.kind else {
            return;
        };
        let ([segment], Res::Def(DefKind::Fn, function)) = (path.segments, path.res) else {
            return;
        };
        if callee.span.from_expansion() || cx.tcx.is_foreign_item(function) {
            return;
        }
        if let Some(local) = function.as_local()
            && cx.tcx.parent_module_from_def_id(local) == cx.tcx.parent_module(expr.hir_id)
        {
            return;
        }
        let Some(module) = importing_module(cx, expr, segment.ident.name, function) else {
            return;
        };
        let name = segment.ident.name;
        span_lint_and_help(
            cx,
            QUALIFIED_FUNCTION_CALLS,
            callee.span,
            format!("call to imported function `{name}` without its module"),
            None,
            format!(
                "import its module and call it as `{module}::{}(..)`, as the Google Rust Style \
                 Guide's \"Qualify calls to top-level functions with a module name\" asks",
                cx.tcx.item_name(function)
            ),
        );
    }
}

/// Finds the `use` that brought `function` into scope as `name`, searching the blocks around
/// `expr` and then its module, and returns the module the import names (`mem` in
/// `use std::mem::swap`). Returns `None` if no such single-item import is in scope.
fn importing_module(
    cx: &LateContext<'_>,
    expr: &Expr<'_>,
    name: Symbol,
    function: DefId,
) -> Option<Symbol> {
    for (_, node) in cx.tcx.hir_parent_iter(expr.hir_id) {
        let items: Vec<ItemId> = match node {
            Node::Block(block) => block
                .stmts
                .iter()
                .filter_map(|stmt| match stmt.kind {
                    StmtKind::Item(item) => Some(item),
                    _ => None,
                })
                .collect(),
            Node::Item(item) => match item.kind {
                ItemKind::Mod(_, module) => module.item_ids.to_vec(),
                _ => continue,
            },
            Node::Crate(module) => module.item_ids.to_vec(),
            _ => continue,
        };
        let module = items.into_iter().find_map(|id| {
            let item = cx.tcx.hir_item(id);
            if item.span.from_expansion() {
                return None;
            }
            imports(cx.tcx, item).into_iter().find_map(|import| {
                if import.name.is_none_or(|ident| ident.name != name)
                    || import.res.value_ns != Some(Res::Def(DefKind::Fn, function))
                {
                    return None;
                }
                let [.., module, _] = import.segments.as_slice() else {
                    return None;
                };
                Some(module.ident.name)
            })
        });
        if module.is_some() || matches!(node, Node::Crate(_) | Node::Item(_)) {
            return module;
        }
    }
    None
}

#[test]
fn ui() {
    crate::ui_harness::ui_test(QUALIFIED_FUNCTION_CALLS);
}
