use clippy_utils::diagnostics::span_lint_and_help;
use clippy_utils::higher::{If, IfLet};
use clippy_utils::is_must_use_func_call;
use clippy_utils::res::MaybeDef as _;
use clippy_utils::source::SpanExt as _;
use clippy_utils::ty::opt_must_use_path;
use rustc_hir::{Expr, ExprKind, LetStmt, LocalSource, PatKind};
use rustc_lint::{LateContext, LateLintPass};
use rustc_lint_defs::{declare_lint, declare_lint_pass};
use rustc_middle::ty::print::with_forced_trimmed_paths;
use rustc_span::sym;

declare_lint! {
    /// Checks for an assignment to `_` that discards a `Result`, another value of a `#[must_use]`
    /// type, or the value of a `#[must_use]` function, as in `_ = save(&amount);`. It also checks
    /// for a `let` that binds a `Result`, or the `.is_ok()`, `.is_err()`, `.ok()` or `.err()` of
    /// one, to a name starting with `_`, as in `let _saved = save(&amount);`, and for an `if` with
    /// an empty body and no `else` whose condition is one of those, as in
    /// `if let Ok(()) = save(&amount) {}` or `if save(&amount).is_ok() {}`.
    ///
    /// rustc's `unused_must_use` catches `save(&amount);` and `clippy::let_underscore_must_use`
    /// catches `let _ = save(&amount);`, but neither looks at `_ = save(&amount);`. A binding and a
    /// condition count as uses, though `clippy::used_underscore_binding` keeps `_saved` from being
    /// read and an empty body does nothing with the condition. Code fails closed, so an error
    /// dropped without a look is a bug.
    ///
    /// A body that is empty only once `cfg` strips its code isn't checked. A `let` or an `if`
    /// statement takes its `expect` on the statement; an assignment can't take an attribute, so
    /// its `expect` goes on the enclosing function.
    ///
    /// Code a macro writes isn't checked. Code a proc macro copies from its caller's tokens, such
    /// as the body of a function under an attribute macro, is: its author wrote it there.
    ///
    /// ```rust,ignore
    /// _ = save(&amount);
    /// let _saved = save(&amount).is_ok();
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// save(&amount)?;
    /// ```
    pub DISCARDED_RESULT,
    Warn,
    "a `Result` or `#[must_use]` value discarded by `_ = ..`, `let _name = ..` or an empty `if`"
}

declare_lint_pass!(DiscardedResult => [DISCARDED_RESULT]);

impl<'tcx> LateLintPass<'tcx> for DiscardedResult {
    /// Reports a `_ = ..` that discards a `Result` or a `#[must_use]` value, and a `let _name = ..`
    /// that binds a `Result`. `_ = expr` lowers to `{ let _ = expr; }`, marked as an assignment's
    /// desugaring.
    fn check_local(&mut self, cx: &LateContext<'tcx>, local: &'tcx LetStmt<'tcx>) {
        let Some(init) = local.init else {
            return;
        };
        if local.span.from_expansion() {
            return;
        }
        let (message, help) = match (local.source, local.pat.kind) {
            (LocalSource::AssignDesugar, PatKind::Wild) => {
                if let Some(part) = result_part(cx, init) {
                    (format!("`_ = ..` discards {part}"), RESULT_HELP)
                } else if let Some(message) = must_use_message(cx, init) {
                    (message, MUST_USE_HELP)
                } else {
                    return;
                }
            }
            (LocalSource::Normal, PatKind::Binding(_, _, ident, None))
                if ident.name.as_str().starts_with('_') =>
            {
                let Some(part) = result_part(cx, init) else {
                    return;
                };
                (
                    format!("`let {ident} = ..` binds {part} and never reads it"),
                    RESULT_HELP,
                )
            }
            _ => return,
        };
        span_lint_and_help(cx, DISCARDED_RESULT, local.span, message, None, help);
    }

    /// Reports an `if` with an empty body and no `else` whose condition discards a `Result`. The
    /// compiler lowers a condition without `let` inside `DropTemps`.
    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        if expr.span.from_expansion() {
            return;
        }
        let (condition, then, otherwise) = if let Some(if_let) = IfLet::hir(cx, expr) {
            (if_let.let_expr, if_let.if_then, if_let.if_else)
        } else if let Some(if_expr) = If::hir(expr) {
            let condition = match if_expr.cond.kind {
                ExprKind::DropTemps(inner) => inner,
                _ => if_expr.cond,
            };
            (condition, if_expr.then, if_expr.r#else)
        } else {
            return;
        };
        if otherwise.is_some() || !is_empty_block(cx, then) {
            return;
        }
        let Some(part) = result_part(cx, condition) else {
            return;
        };
        span_lint_and_help(
            cx,
            DISCARDED_RESULT,
            expr.span,
            format!("an `if` with an empty body and no `else` discards {part}"),
            None,
            RESULT_HELP,
        );
    }
}

const RESULT_HELP: &str = "handle the error, or propagate it with `?`; code fails closed, so \
                           a dropped error is a bug";

const MUST_USE_HELP: &str = "use the value; `#[must_use]` marks a value that is a mistake to \
                             ignore";

/// What `expr` holds of a `Result`: the `Result` itself, or what `.is_ok()`, `.is_err()`, `.ok()`
/// or `.err()` leaves of one.
fn result_part(cx: &LateContext<'_>, expr: &Expr<'_>) -> Option<String> {
    let typeck = cx.typeck_results();
    if typeck.expr_ty(expr).is_diag_item(cx, sym::Result) {
        return Some("a `Result`".to_owned());
    }
    if let ExprKind::MethodCall(method, receiver, [], _) = expr.kind
        && let name @ ("is_ok" | "is_err" | "ok" | "err") = method.ident.as_str()
        && typeck
            .expr_ty(receiver)
            .peel_refs()
            .is_diag_item(cx, sym::Result)
    {
        return Some(format!("the `.{name}()` of a `Result`"));
    }
    None
}

/// Whether `expr` is a block with nothing in it, in the source as well as after `cfg` stripping.
fn is_empty_block(cx: &LateContext<'_>, expr: &Expr<'_>) -> bool {
    let ExprKind::Block(block, None) = expr.kind else {
        return false;
    };
    block.stmts.is_empty()
        && block.expr.is_none()
        && block.span.check_text(cx, |text| {
            text.strip_prefix('{')
                .and_then(|text| text.strip_suffix('}'))
                .is_some_and(|inside| inside.trim().is_empty())
        })
}

/// The message for `_ = expr` when `expr` has a `#[must_use]` type or comes from a `#[must_use]`
/// function.
fn must_use_message(cx: &LateContext<'_>, expr: &Expr<'_>) -> Option<String> {
    let ty = cx.typeck_results().expr_ty(expr);
    if opt_must_use_path(cx, ty).is_some() {
        Some(with_forced_trimmed_paths!(format!(
            "`_ = ..` discards a value of the `#[must_use]` type `{ty}`"
        )))
    } else if is_must_use_func_call(cx, expr) {
        Some("`_ = ..` discards the value of a `#[must_use]` function".to_owned())
    } else {
        None
    }
}

#[test]
fn ui() {
    crate::ui_harness::ui_test(DISCARDED_RESULT);
}
