use clippy_utils::diagnostics::span_lint_and_help;
use clippy_utils::source::snippet;
use rustc_hir::def::{DefKind, Res};
use rustc_hir::{
    Expr, ExprKind, GenericArg, LetStmt, LocalSource, MatchSource, PatKind, PathSegment, QPath,
};
use rustc_lint::{LateContext, LateLintPass};
use rustc_lint_defs::{declare_lint, declare_lint_pass};
use rustc_middle::ty::print::with_forced_trimmed_paths;
use rustc_middle::ty::{self, TyCtxt};
use rustc_span::def_id::DefId;
use rustc_span::{Span, Symbol, sym};

declare_lint! {
    /// Checks for a `let` without a type whose value comes straight from a call with a generic
    /// return type: `.parse()`, `.into()`, `.try_into()`, `Iterator`'s `.collect()`, `.sum()`,
    /// `.product()`, `.unzip()` and `.partition(..)`, or `Default::default()`, possibly through
    /// `?` and `Option` or `Result` methods that pass the value through, such as `.unwrap()`,
    /// `.expect(..)`, `.map_err(..)`, `.ok()`, `.unwrap_or(..)` and `.unwrap_or_default()`. It
    /// also checks for those calls given their type by turbofish when the `let` could carry it.
    ///
    /// A call whose whole return type is generic, such as `.collect()`, is left alone behind
    /// those: `let values: Vec<u64> = it.collect()?` doesn't compile, because the type on the
    /// `let` doesn't say which `Result` or `Option` to collect into.
    ///
    /// The Google Rust Style Guide, "Don't overuse type inference": add types when calling
    /// methods with a generic return type, such as `.into()` or `.parse()`, and prefer putting
    /// them on variable declarations rather than in turbofishes.
    ///
    /// Code a macro writes isn't checked. Code a proc macro copies from its caller's tokens, such
    /// as the body of a function under an attribute macro, is: its author wrote it there.
    ///
    /// ```rust,ignore
    /// let amount = input.parse()?;
    /// let names = customers.iter().map(Customer::name).collect::<Vec<_>>();
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// let amount: u64 = input.parse()?;
    /// let names: Vec<_> = customers.iter().map(Customer::name).collect();
    /// ```
    pub INFERRED_CONVERSION_BINDING,
    Warn,
    "a generic conversion such as `.parse()` bound by `let` without a type, or given one by turbofish"
}

declare_lint_pass!(InferredConversionBinding => [INFERRED_CONVERSION_BINDING]);

const GUIDE: &str = "as the Google Rust Style Guide's \"Don't overuse type inference\" asks";

impl<'tcx> LateLintPass<'tcx> for InferredConversionBinding {
    /// Reports a `let` with no type that binds a generic conversion, and one whose conversion
    /// takes its type by turbofish. A `let ... else` is left alone: its pattern is refutable, so
    /// a type on it would name the whole `Result` or `Option`, and a turbofish reads better there.
    fn check_local(&mut self, cx: &LateContext<'tcx>, local: &'tcx LetStmt<'tcx>) {
        if local.ty.is_some()
            || local.els.is_some()
            || !matches!(local.source, LocalSource::Normal)
            || local.span.from_expansion()
            || matches!(local.pat.kind, PatKind::Wild)
        {
            return;
        }
        let Some(init) = local.init else {
            return;
        };
        let call = peel_unwrapping(cx, init);
        if call.span.from_expansion() {
            return;
        }
        let Some(conversion) = conversion(cx, call) else {
            return;
        };
        if call.hir_id != init.hir_id && returns_type_parameter(cx.tcx, conversion.function) {
            return;
        }
        let name = conversion.name;
        let pattern = snippet(cx, local.pat.span, "..");
        let binding_ty =
            with_forced_trimmed_paths!(cx.typeck_results().pat_ty(local.pat).to_string());
        if let Some(turbofish) = conversion.turbofish {
            span_lint_and_help(
                cx,
                INFERRED_CONVERSION_BINDING,
                turbofish,
                format!("type given by turbofish on `{name}` rather than on the `let`"),
                None,
                format!("drop the turbofish and write `let {pattern}: {binding_ty}`, {GUIDE}"),
            );
        } else {
            span_lint_and_help(
                cx,
                INFERRED_CONVERSION_BINDING,
                local.pat.span,
                format!("`let` binds the result of `{name}` without a type"),
                None,
                format!("write `let {pattern}: {binding_ty}`, {GUIDE}"),
            );
        }
    }
}

/// A call to one of the functions with a generic return type that this lint checks.
struct Conversion {
    /// The function called.
    function: DefId,
    /// How the help names the call, such as `.collect()`.
    name: String,
    /// The span of a turbofish that names a type.
    turbofish: Option<Span>,
}

/// The conversion `call` makes, if it calls one of the checked functions.
fn conversion(cx: &LateContext<'_>, call: &Expr<'_>) -> Option<Conversion> {
    match call.kind {
        ExprKind::MethodCall(segment, ..) => {
            let method = cx.typeck_results().type_dependent_def_id(call.hir_id)?;
            is_conversion(cx.tcx, method, segment.ident.name).then(|| Conversion {
                function: method,
                name: format!(".{}()", segment.ident.name),
                turbofish: turbofish(segment),
            })
        }
        ExprKind::Call(callee, []) => {
            let ExprKind::Path(QPath::Resolved(None, path)) = callee.kind else {
                return None;
            };
            let Res::Def(DefKind::AssocFn, function) = path.res else {
                return None;
            };
            cx.tcx
                .is_diagnostic_item(sym::default_fn, function)
                .then(|| Conversion {
                    function,
                    name: "Default::default()".to_owned(),
                    turbofish: None,
                })
        }
        _ => None,
    }
}

/// Whether `function`'s declared return type is a bare type parameter, such as the `B` of
/// `Iterator::collect` or the `Self` of `Default::default`. Only a turbofish can name that type
/// when the call's result goes through `?`, `.unwrap()` or `.expect(..)`.
fn returns_type_parameter(tcx: TyCtxt<'_>, function: DefId) -> bool {
    matches!(
        tcx.fn_sig(function)
            .instantiate_identity()
            .skip_normalization()
            .skip_binder()
            .output()
            .kind(),
        ty::Param(_)
    )
}

/// The `Option` and `Result` methods that pass the value inside through, so a conversion
/// before them still leaves its type to inference.
const PASS_THROUGH: &[&str] = &[
    "expect",
    "map_err",
    "ok",
    "ok_or",
    "ok_or_else",
    "unwrap",
    "unwrap_or",
    "unwrap_or_default",
    "unwrap_or_else",
];

/// Looks through `?` and the methods in [`PASS_THROUGH`], such as `.unwrap()`, `.expect(..)`,
/// `.map_err(..)` and `.unwrap_or(..)`, which pass the converted type through.
fn peel_unwrapping<'tcx>(cx: &LateContext<'tcx>, mut expr: &'tcx Expr<'tcx>) -> &'tcx Expr<'tcx> {
    loop {
        match expr.kind {
            ExprKind::Match(scrutinee, _, MatchSource::TryDesugar(_)) => {
                let ExprKind::Call(_, [inner]) = scrutinee.kind else {
                    return expr;
                };
                expr = inner;
            }
            ExprKind::MethodCall(segment, receiver, ..)
                if PASS_THROUGH.contains(&segment.ident.name.as_str())
                    && let ty::Adt(adt, _) = cx.typeck_results().expr_ty(receiver).kind()
                    && (cx.tcx.is_diagnostic_item(sym::Option, adt.did())
                        || cx.tcx.is_diagnostic_item(sym::Result, adt.did())) =>
            {
                expr = receiver;
            }
            _ => return expr,
        }
    }
}

/// Whether `method` is `str::parse`, `Into::into`, `TryInto::try_into`, or `Iterator`'s
/// `collect`, `sum`, `product`, `unzip` or `partition`.
fn is_conversion(tcx: TyCtxt<'_>, method: DefId, name: Symbol) -> bool {
    match name.as_str() {
        "parse" => tcx
            .inherent_impl_of_assoc(method)
            .is_some_and(|implementation| {
                tcx.type_of(implementation)
                    .instantiate_identity()
                    .skip_normalization()
                    .is_str()
            }),
        "into" => trait_is(tcx, method, sym::Into),
        "try_into" => trait_is(tcx, method, sym::TryInto),
        "collect" | "sum" | "product" | "unzip" | "partition" => {
            trait_is(tcx, method, sym::Iterator)
        }
        _ => false,
    }
}

fn trait_is(tcx: TyCtxt<'_>, method: DefId, diagnostic_name: Symbol) -> bool {
    tcx.trait_of_assoc(method)
        .is_some_and(|trait_id| tcx.is_diagnostic_item(diagnostic_name, trait_id))
}

/// The span of the turbofish's arguments, when one names a type rather than `_`.
fn turbofish(segment: &PathSegment<'_>) -> Option<Span> {
    let args = segment.args?;
    args.args
        .iter()
        .any(|arg| matches!(arg, GenericArg::Type(_) | GenericArg::Const(_)))
        .then_some(args.span_ext)
}

#[test]
fn ui() {
    crate::ui_harness::ui_test(INFERRED_CONVERSION_BINDING);
}
