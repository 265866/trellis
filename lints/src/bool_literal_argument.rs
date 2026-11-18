use clippy_utils::diagnostics::span_lint_and_help;
use rustc_ast::LitKind;
use rustc_hir::def::{DefKind, Res};
use rustc_hir::def_id::DefId;
use rustc_hir::{Expr, ExprKind};
use rustc_lint::{LateContext, LateLintPass};
use rustc_lint_defs::{declare_lint, declare_lint_pass};
use rustc_middle::ty;

declare_lint! {
    /// Checks for calls to a function or method defined in this crate that pass a literal
    /// `true` or `false` to a parameter declared as `bool`.
    ///
    /// The Google Rust Style Guide, "Prefer `enum` over `bool` in APIs": use an `enum`
    /// parameter where the purpose of a `bool` isn't obvious, or else document the `bool` with
    /// an inline comment at the call. `code_comment` bans code comments, so the `enum` is the way
    /// to make the call read.
    ///
    /// Left alone, because the call already says what the `bool` means or the API isn't
    /// the workspace's to change: functions and methods from other crates, such as std's
    /// `OpenOptions::new().read(true)`; the value of a setter, a method named `set_*` whose first
    /// argument is its only `bool`, whatever context arguments follow, as in
    /// `self.set_visible(false, cx)`; parameters of a generic type that a `bool` fills; tuple
    /// struct and variant constructors; closures; and calls a macro writes, including from the
    /// caller's tokens.
    ///
    /// ```rust,ignore
    /// fn order(quantity: u64, express: bool) -> Receipt { /* ... */ }
    ///
    /// order(100, true);
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// enum Animation {
    ///     Full,
    ///     Express,
    /// }
    ///
    /// fn order(quantity: u64, animation: Animation) -> Receipt { /* ... */ }
    ///
    /// order(100, Animation::Express);
    /// ```
    pub BOOL_LITERAL_ARGUMENT,
    Warn,
    "a literal `true` or `false` passed to a `bool` parameter of this crate's function"
}

declare_lint_pass!(BoolLiteralArgument => [BOOL_LITERAL_ARGUMENT]);

impl<'tcx> LateLintPass<'tcx> for BoolLiteralArgument {
    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        if expr.span.from_expansion() {
            return;
        }
        let (callee, arguments, receiver_offset) = match expr.kind {
            ExprKind::Call(function, arguments) => {
                let ExprKind::Path(ref path) = function.kind else {
                    return;
                };
                let Res::Def(DefKind::Fn | DefKind::AssocFn, callee) =
                    cx.qpath_res(path, function.hir_id)
                else {
                    return;
                };
                (callee, arguments, 0)
            }
            ExprKind::MethodCall(_, _, arguments, _) => {
                let Some(callee) = cx.typeck_results().type_dependent_def_id(expr.hir_id) else {
                    return;
                };
                (callee, arguments, 1)
            }
            _ => return,
        };
        if !callee.is_local() {
            return;
        }
        for (index, argument) in arguments.iter().enumerate() {
            let ExprKind::Lit(literal) = argument.kind else {
                continue;
            };
            let LitKind::Bool(value) = literal.node else {
                continue;
            };
            let parameter = index + receiver_offset;
            if argument.span.from_expansion()
                || !declared_bool(cx, callee, parameter)
                || is_setter_value(cx, callee, parameter)
            {
                continue;
            }
            span_lint_and_help(
                cx,
                BOOL_LITERAL_ARGUMENT,
                argument.span,
                format!("`{value}` passed as a `bool` argument"),
                None,
                "make the parameter an `enum` whose variants name both choices, as the Google Rust \
                 Style Guide's \"Prefer `enum` over `bool` in APIs\" asks",
            );
        }
    }
}

/// Whether `parameter`, counting `self`, is the value of a setter: the first parameter after
/// `self` of a method named `set_*` whose only `bool` parameter it is. Parameters after it, such
/// as `cx: &mut Context`, pass context rather than say what to set.
fn is_setter_value(cx: &LateContext<'_>, callee: DefId, parameter: usize) -> bool {
    let tcx = cx.tcx;
    if parameter != 1
        || !tcx.item_name(callee).as_str().starts_with("set_")
        || tcx.def_kind(callee) != DefKind::AssocFn
        || !tcx.associated_item(callee).is_method()
    {
        return false;
    }
    let signature = tcx.fn_sig(callee).instantiate_identity().skip_binder();
    signature
        .inputs()
        .iter()
        .filter(|input| matches!(input.kind(), ty::Bool))
        .count()
        == 1
}

/// Whether the parameter at `index` of `callee`'s declared signature, counting `self`, has type
/// `bool`, rather than a generic type that a `bool` fills.
fn declared_bool(cx: &LateContext<'_>, callee: DefId, index: usize) -> bool {
    let signature = cx.tcx.fn_sig(callee).instantiate_identity().skip_binder();
    signature
        .inputs()
        .get(index)
        .is_some_and(|input| matches!(input.kind(), ty::Bool))
}

#[test]
fn ui() {
    crate::ui_harness::ui_test(BOOL_LITERAL_ARGUMENT);
}
