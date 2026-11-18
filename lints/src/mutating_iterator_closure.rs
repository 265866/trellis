use clippy_utils::diagnostics::span_lint_and_then;
use rustc_data_structures::fx::FxIndexMap;
use rustc_hir::def::Res;
use rustc_hir::intravisit::{self, Visitor};
use rustc_hir::{BorrowKind, Expr, ExprKind, HirId, Mutability, QPath, UnOp, Upvar};
use rustc_lint::{LateContext, LateLintPass};
use rustc_lint_defs::{declare_lint, declare_lint_pass};
use rustc_middle::hir::nested_filter;
use rustc_middle::ty::adjustment::{Adjust, AutoBorrow, AutoBorrowMutability};
use rustc_middle::ty::{self, TyCtxt};
use rustc_span::Span;

use crate::iterator_methods::is_iterator_method;

declare_lint! {
    /// Checks for a closure passed to an `Iterator` or `DoubleEndedIterator` method, such as
    /// `map`, `filter`, `for_each`, `inspect`, `any` or `find`, that mutates a variable it
    /// captures from the enclosing code: it assigns to the variable or a place inside it, also
    /// through a `&mut` reference the variable holds, or it calls a `&mut self` method on, or
    /// takes `&mut` of, the variable or a place inside it that isn't behind a reference. A
    /// `move` closure that bumps its own copy of a counter is flagged too, as in the guide's
    /// example.
    ///
    /// The Google Rust Style Guide, "Don't use iterator combinator chains when direct iteration
    /// is clearer": its example of a chain that is hard to read "involves conditionals and
    /// mutates external state", a counter bumped inside `map`; "the corresponding `for` loop is
    /// easier to read."
    ///
    /// Passing a captured `&mut` reference on, as in
    /// `.map(|row| self.write_row(row, table, log))`, or calling a method through one, as in
    /// `log.record()`, uses access the caller lent the enclosing function rather than state the
    /// closure keeps, so it passes. State the iterator hands the closure, such as `fold`'s
    /// accumulator or `scan`'s state, is a parameter rather than a capture, so mutating it is fine.
    /// So is interior mutability through a shared borrow, such as `Cell::set`, which this lint
    /// can't tell from a read.
    ///
    /// Code a macro writes isn't checked. Code a proc macro copies from its caller's tokens, such
    /// as the body of a function under an attribute macro, is: its author wrote it there.
    ///
    /// ```rust,ignore
    /// let mut error_count = 0;
    /// let receipts: Vec<Receipt> = responses
    ///     .into_iter()
    ///     .map(parse_receipt)
    ///     .filter_map(|receipt| {
    ///         if receipt.is_err() {
    ///             error_count += 1;
    ///         }
    ///         receipt.ok()
    ///     })
    ///     .collect();
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// let mut error_count = 0;
    /// let mut receipts = Vec::with_capacity(responses.len());
    /// for response in responses {
    ///     match parse_receipt(response) {
    ///         Ok(receipt) => receipts.push(receipt),
    ///         Err(_) => error_count += 1,
    ///     }
    /// }
    /// ```
    pub MUTATING_ITERATOR_CLOSURE,
    Warn,
    "a closure passed to an iterator method that mutates captured state"
}

declare_lint_pass!(MutatingIteratorClosure => [MUTATING_ITERATOR_CLOSURE]);

impl<'tcx> LateLintPass<'tcx> for MutatingIteratorClosure {
    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        let ExprKind::MethodCall(segment, _, arguments, _) = expr.kind else {
            return;
        };
        if expr.span.from_expansion() {
            return;
        }
        let Some(method) = cx.typeck_results().type_dependent_def_id(expr.hir_id) else {
            return;
        };
        if !is_iterator_method(cx, method) {
            return;
        }
        for argument in arguments {
            let ExprKind::Closure(closure) = argument.kind else {
                continue;
            };
            if argument.span.from_expansion() {
                continue;
            }
            let Some(upvars) = cx.tcx.upvars_mentioned(closure.def_id) else {
                continue;
            };
            let mut mutations = Mutations {
                cx,
                upvars,
                found: None,
            };
            mutations.visit_expr(cx.tcx.hir_body(closure.body).value);
            let Some((span, variable)) = mutations.found else {
                continue;
            };
            let method_name = segment.ident.name;
            let place = cx.tcx.hir_name(variable);
            span_lint_and_then(
                cx,
                MUTATING_ITERATOR_CLOSURE,
                closure.fn_decl_span,
                format!("closure passed to `{method_name}` mutates `{place}`"),
                |diagnostic| {
                    diagnostic.span_note(span, format!("`{place}` is mutated here"));
                    diagnostic.help(
                        "write it as a `for` loop, whose body shows the mutation, as the Google \
                         Rust Style Guide's \"Don't use iterator combinator chains when direct \
                         iteration is clearer\" asks",
                    );
                },
            );
        }
    }
}

/// Finds the first place in a closure's body that mutates a variable the closure captures.
struct Mutations<'a, 'tcx> {
    cx: &'a LateContext<'tcx>,
    upvars: &'tcx FxIndexMap<HirId, Upvar>,
    /// The mutation, and the variable it mutates.
    found: Option<(Span, HirId)>,
}

/// How a place may reach the captured variable it starts from.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Reach {
    /// Through fields, indexing and dereferences: any write to the place changes state outside
    /// the closure.
    Any,
    /// Only through fields and indexing of the variable itself, not through a reference it
    /// holds: borrowing such a place `&mut` lends out the closure's captured state, while
    /// reborrowing through a captured `&mut` reference passes on access the closure was given.
    Owned,
}

impl<'tcx> Mutations<'_, 'tcx> {
    /// The captured variable that `place` starts from, if it reaches it as `reach` allows.
    fn captured(&self, mut place: &Expr<'tcx>, reach: Reach) -> Option<HirId> {
        loop {
            if reach == Reach::Owned && self.is_reference(place) {
                return None;
            }
            match place.kind {
                ExprKind::Field(base, _) | ExprKind::Index(base, ..) => place = base,
                ExprKind::Unary(UnOp::Deref, base) if reach == Reach::Any => place = base,
                ExprKind::Path(QPath::Resolved(None, path)) => {
                    return match path.res {
                        Res::Local(id) if self.upvars.contains_key(&id) => Some(id),
                        _ => None,
                    };
                }
                _ => return None,
            }
        }
    }

    /// Whether `expr` is a reference, which a place built on it reaches through.
    fn is_reference(&self, expr: &Expr<'tcx>) -> bool {
        matches!(
            self.cx.typeck_results().expr_ty(expr).kind(),
            ty::Ref(..) | ty::RawPtr(..)
        )
    }

    /// Whether the receiver `receiver` is borrowed `&mut` for the method it is called with.
    fn borrowed_mutably(&self, receiver: &Expr<'tcx>) -> bool {
        self.cx
            .typeck_results()
            .expr_adjustments(receiver)
            .iter()
            .any(|adjustment| {
                matches!(
                    adjustment.kind,
                    Adjust::Borrow(AutoBorrow::Ref(AutoBorrowMutability::Mut { .. }))
                )
            })
    }

    fn record(&mut self, span: Span, variable: Option<HirId>) {
        if self.found.is_none()
            && let Some(variable) = variable
        {
            self.found = Some((span, variable));
        }
    }
}

impl<'tcx> Visitor<'tcx> for Mutations<'_, 'tcx> {
    type NestedFilter = nested_filter::OnlyBodies;

    fn maybe_tcx(&mut self) -> TyCtxt<'tcx> {
        self.cx.tcx
    }

    fn visit_expr(&mut self, expr: &'tcx Expr<'tcx>) {
        match expr.kind {
            ExprKind::Assign(target, ..) | ExprKind::AssignOp(_, target, _) => {
                self.record(expr.span, self.captured(target, Reach::Any));
            }
            ExprKind::AddrOf(BorrowKind::Ref, Mutability::Mut, place) => {
                self.record(expr.span, self.captured(place, Reach::Owned));
            }
            ExprKind::MethodCall(_, receiver, ..) if self.borrowed_mutably(receiver) => {
                self.record(expr.span, self.captured(receiver, Reach::Owned));
            }
            _ => {}
        }
        intravisit::walk_expr(self, expr);
    }
}

#[test]
fn ui() {
    crate::ui_harness::ui_test(MUTATING_ITERATOR_CLOSURE);
}
