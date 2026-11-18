use clippy_utils::diagnostics::span_lint_and_help;
use clippy_utils::get_parent_expr;
use rustc_attr_ir::lang_items::LangItem;
use rustc_hir::def::Res;
use rustc_hir::{AssignOpKind, BinOpKind, Block, Expr, ExprKind, HirId, QPath, Stmt, StmtKind};
use rustc_lint::{LateContext, LateLintPass};
use rustc_lint_defs::{declare_lint, declare_lint_pass};
use rustc_span::Span;

declare_lint! {
    /// Checks for three or more strings joined with `+`, as in `a + b + c` where `a` is a
    /// `String`, and for three or more `+=` statements in a row appending to one local `String`.
    ///
    /// The Google Rust Style Guide, "Prefer `format!` / `write!` for concatenation and
    /// substitution": use `format!("{a}{b}{c}")` instead of `a + b + c` to concatenate more than
    /// two strings. `format!` needs fewer allocations and shows the result's shape at a glance.
    ///
    /// Code a macro writes isn't checked. Code a proc macro copies from its caller's tokens, such
    /// as the body of a function under an attribute macro, is: its author wrote it there.
    ///
    /// ```rust,ignore
    /// let line = name.to_owned() + ": " + &score.to_string();
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// let line = format!("{name}: {score}");
    /// ```
    pub STRING_CONCAT_CHAIN,
    Warn,
    "three or more strings joined with `+`"
}

declare_lint_pass!(StringConcatChain => [STRING_CONCAT_CHAIN]);

impl<'tcx> LateLintPass<'tcx> for StringConcatChain {
    /// Reports a chain of two or more string `+`s at its outermost `+`, which `a + b + c` parses
    /// as `(a + b) + c`.
    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        if expr.span.from_expansion() || !is_string_add(cx, expr) {
            return;
        }
        if let Some(parent) = get_parent_expr(cx, expr)
            && let ExprKind::Binary(_, left, _) = parent.kind
            && left.hir_id == expr.hir_id
            && is_string_add(cx, parent)
        {
            return;
        }
        let mut operators = 0;
        let mut left = expr;
        while is_string_add(cx, left) {
            operators += 1;
            let ExprKind::Binary(_, inner, _) = left.kind else {
                break;
            };
            left = inner;
        }
        if operators < 2 {
            return;
        }
        span_lint_and_help(
            cx,
            STRING_CONCAT_CHAIN,
            expr.span,
            format!("{} strings joined with `+`", operators + 1),
            None,
            "build the string with `format!`, or `write!` into an existing one, as the Google Rust Style \
             Guide's \"Prefer `format!` / `write!` for concatenation and substitution\" asks",
        );
    }

    fn check_block(&mut self, cx: &LateContext<'tcx>, block: &'tcx Block<'tcx>) {
        let mut run: Option<(HirId, Span, usize)> = None;
        for statement in block.stmts {
            let target = appended_string(cx, statement);
            run = match (run, target) {
                (Some((local, first, count)), Some((next, _))) if local == next => {
                    Some((local, first.to(statement.span), count + 1))
                }
                (previous, next) => {
                    if let Some((_, span, count)) = previous {
                        report_appends(cx, span, count);
                    }
                    next.map(|(local, span)| (local, span, 1))
                }
            };
        }
        if let Some((_, span, count)) = run {
            report_appends(cx, span, count);
        }
    }
}

/// The local `String` that `statement` appends to with `+=`, and the statement's span.
fn appended_string<'tcx>(cx: &LateContext<'tcx>, statement: &Stmt<'tcx>) -> Option<(HirId, Span)> {
    let StmtKind::Semi(expr) = statement.kind else {
        return None;
    };
    let ExprKind::AssignOp(operator, target, _) = expr.kind else {
        return None;
    };
    if operator.node != AssignOpKind::AddAssign || statement.span.from_expansion() {
        return None;
    }
    let ExprKind::Path(QPath::Resolved(None, path)) = target.kind else {
        return None;
    };
    let Res::Local(local) = path.res else {
        return None;
    };
    let string = cx.tcx.lang_items().get(LangItem::String)?;
    cx.typeck_results()
        .expr_ty(target)
        .ty_adt_def()
        .is_some_and(|adt| adt.did() == string)
        .then_some((local, statement.span))
}

fn report_appends(cx: &LateContext<'_>, span: Span, count: usize) {
    if count < 3 {
        return;
    }
    span_lint_and_help(
        cx,
        STRING_CONCAT_CHAIN,
        span,
        format!("{count} strings appended with `+=` in a row"),
        None,
        "build the string with one `format!`, or `write!` into it, as the Google Rust Style \
         Guide's \"Prefer `format!` / `write!` for concatenation and substitution\" asks",
    );
}

/// Whether the expression is `String + ...`, the `Add<&str>` impl on `String`.
fn is_string_add<'tcx>(cx: &LateContext<'tcx>, expr: &Expr<'tcx>) -> bool {
    let ExprKind::Binary(operator, left, _) = expr.kind else {
        return false;
    };
    if operator.node != BinOpKind::Add || left.span.from_expansion() {
        return false;
    }
    let Some(string) = cx.tcx.lang_items().get(LangItem::String) else {
        return false;
    };
    let typeck = cx.typeck_results();
    [typeck.expr_ty(expr), typeck.expr_ty(left)]
        .iter()
        .all(|ty| ty.ty_adt_def().is_some_and(|adt| adt.did() == string))
}

#[test]
fn ui() {
    crate::ui_harness::ui_test(STRING_CONCAT_CHAIN);
}
