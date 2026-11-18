use clippy_utils::diagnostics::span_lint_and_help;
use clippy_utils::get_parent_expr;
use clippy_utils::higher::ForLoop;
use rustc_hir::{Expr, ExprKind, MatchSource};
use rustc_lint::{LateContext, LateLintPass};
use rustc_lint_defs::{declare_lint, declare_lint_pass};
use rustc_span::Symbol;

use crate::iterator_methods::is_iterator_method;

declare_lint! {
    /// Checks for a `for` loop whose header builds an iterator chain with a closure-taking
    /// combinator such as `map` or `filter`, and for any chain of more than four closure-taking
    /// `Iterator` methods.
    ///
    /// The Google Rust Style Guide, "Don't use iterator combinator chains when direct iteration
    /// is clearer": "Don't construct combinator chains in the header of a `for` loop. Instead,
    /// assign the combinator chain to a variable, then consume that variable in the `for` loop",
    /// and "Avoid long and complex combinator chains."
    ///
    /// Methods that take no closure, such as `iter`, `enumerate`, `rev`, `zip`, `skip` or
    /// `copied`, are fine in a `for` header and don't count toward a chain's length. Methods
    /// count only when they resolve to `Iterator` or `DoubleEndedIterator`.
    ///
    /// Code a macro writes isn't checked. Code a proc macro copies from its caller's tokens, such
    /// as the body of a function under an attribute macro, is: its author wrote it there.
    ///
    /// ```rust,ignore
    /// for bug in bugs.iter().filter(|bug| bug.priority <= Priority::P1) {
    ///     assign_bug(bug);
    /// }
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// let urgent_bugs = bugs.iter().filter(|bug| bug.priority <= Priority::P1);
    /// for bug in urgent_bugs {
    ///     assign_bug(bug);
    /// }
    /// ```
    pub ITERATOR_CHAIN_IN_FOR,
    Warn,
    "an iterator adapter chain in a `for` loop header, or a long adapter chain"
}

declare_lint_pass!(IteratorChainInFor => [ITERATOR_CHAIN_IN_FOR]);

/// The most closure-taking combinators one chain may have.
const MAX_COMBINATORS: usize = 4;

const GUIDE: &str = "the Google Rust Style Guide's \"Don't use iterator combinator chains when direct \
                     iteration is clearer\"";

impl<'tcx> LateLintPass<'tcx> for IteratorChainInFor {
    /// A chain is reported once, at its outermost call. A chain in a `for` header is reported only
    /// as a header, never also for its length.
    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        if let Some(for_loop) = ForLoop::hir(expr) {
            if for_loop.span.from_expansion() || for_loop.arg.span.from_expansion() {
                return;
            }
            let combinators = closure_combinators(cx, for_loop.arg);
            if let Some(first) = combinators.last() {
                span_lint_and_help(
                    cx,
                    ITERATOR_CHAIN_IN_FOR,
                    for_loop.arg.span,
                    format!("`for` loop header builds a combinator chain with `{first}`"),
                    None,
                    format!(
                        "assign the chain to a variable whose name says what it holds, then loop over that \
                         variable, as {GUIDE} asks"
                    ),
                );
            }
            return;
        }
        if !matches!(expr.kind, ExprKind::MethodCall(..)) || expr.span.from_expansion() {
            return;
        }
        if let Some(parent) = get_parent_expr(cx, expr) {
            if let ExprKind::MethodCall(_, receiver, ..) = parent.kind
                && receiver.hir_id == expr.hir_id
            {
                return;
            }
            if is_for_loop_iterable(cx, parent) {
                return;
            }
        }
        let combinators = closure_combinators(cx, expr);
        if combinators.len() > MAX_COMBINATORS {
            span_lint_and_help(
                cx,
                ITERATOR_CHAIN_IN_FOR,
                expr.span,
                format!(
                    "iterator chain with {} closure-taking combinators",
                    combinators.len()
                ),
                None,
                format!(
                    "split it into named steps, or write it as a `for` loop, as {GUIDE} asks for long and \
                     complex chains"
                ),
            );
        }
    }
}

/// The names of the closure-taking `Iterator` methods along the chain's receivers, outermost
/// first.
fn closure_combinators(cx: &LateContext<'_>, mut expr: &Expr<'_>) -> Vec<Symbol> {
    let tcx = cx.tcx;
    let mut names = Vec::new();
    while let ExprKind::MethodCall(segment, receiver, ..) = expr.kind {
        if let Some(method) = cx.typeck_results().type_dependent_def_id(expr.hir_id)
            && is_iterator_method(cx, method)
            && tcx
                .explicit_clauses_of(method)
                .clauses
                .iter()
                .any(|(clause, _)| {
                    clause
                        .as_trait_clause()
                        .is_some_and(|bound| tcx.is_fn_trait(bound.def_id()))
                })
        {
            names.push(segment.ident.name);
        }
        expr = receiver;
    }
    names
}

/// Whether `parent` is the `IntoIterator::into_iter` call a `for` loop wraps its header in.
fn is_for_loop_iterable(cx: &LateContext<'_>, parent: &Expr<'_>) -> bool {
    matches!(parent.kind, ExprKind::Call(..))
        && get_parent_expr(cx, parent).is_some_and(|grandparent| {
            matches!(
                grandparent.kind,
                ExprKind::Match(_, _, MatchSource::ForLoopDesugar)
            )
        })
}

#[test]
fn ui() {
    crate::ui_harness::ui_test(ITERATOR_CHAIN_IN_FOR);
}
