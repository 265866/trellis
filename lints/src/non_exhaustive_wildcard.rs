use clippy_utils::diagnostics::span_lint_and_help;
use rustc_data_structures::fx::FxHashSet;
use rustc_hir::def::{DefKind, Res};
use rustc_hir::def_id::DefId;
use rustc_hir::{Arm, Expr, ExprKind, MatchSource, Pat, PatExpr, PatExprKind, PatKind};
use rustc_lint::{LateContext, LateLintPass};
use rustc_lint_defs::{declare_lint, declare_lint_pass};
use rustc_middle::ty::{self, AdtDef, Ty, TyCtxt};
use rustc_span::Span;

declare_lint! {
    /// Checks for a `match` on another crate's `#[non_exhaustive]` enum, at the top or nested
    /// in another pattern, where some variants appear in no arm and fall to a wildcard: `_` or a
    /// binding whose type is or holds the enum, as in `Some(_)` or a `_` arm on an
    /// `Option<IntErrorKind>`, or a `..` that leaves out fields holding it, as in `Ok(..)`. A
    /// match that names none of the enum's variants, such as `Some(kind) => describe(kind)`,
    /// passes the value on whole and isn't checked.
    ///
    /// The Google Rust Style Guide, "Don't omit patterns on `non_exhaustive` enums when
    /// matching": list every variant, placing the ones you don't care about in the wildcard arm.
    /// The reader then sees which known cases fall through, apart from variants added later. If
    /// you only care about some variants, use `if let` or `let ... else`.
    ///
    /// Variants that are unstable, deprecated or `#[doc(hidden)]` are left out of the list.
    ///
    /// Code a macro writes isn't checked. Code a proc macro copies from its caller's tokens, such
    /// as the body of a function under an attribute macro, is: its author wrote it there.
    ///
    /// ```rust,ignore
    /// match error.kind() {
    ///     IntErrorKind::Empty => "empty",
    ///     _ => "invalid",
    /// }
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// match error.kind() {
    ///     IntErrorKind::Empty => "empty",
    ///     IntErrorKind::InvalidDigit
    ///     | IntErrorKind::PosOverflow
    ///     | IntErrorKind::NegOverflow
    ///     | IntErrorKind::Zero
    ///     | _ => "invalid",
    /// }
    /// ```
    pub NON_EXHAUSTIVE_WILDCARD,
    Warn,
    "a wildcard arm on a foreign `#[non_exhaustive]` enum that doesn't list the ignored variants"
}

declare_lint_pass!(NonExhaustiveWildcard => [NON_EXHAUSTIVE_WILDCARD]);

/// How many unnamed variants the help lists before summarizing the rest.
const LISTED_VARIANTS: usize = 6;

impl<'tcx> LateLintPass<'tcx> for NonExhaustiveWildcard {
    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        let ExprKind::Match(_, arms, MatchSource::Normal) = expr.kind else {
            return;
        };
        if expr.span.from_expansion() {
            return;
        }
        let tcx = cx.tcx;
        let mut named = FxHashSet::default();
        for arm in arms {
            arm.pat
                .walk_always(|pat| insert_variant(cx, pat, &mut named));
        }
        let mut checked = FxHashSet::default();
        for adt in named
            .iter()
            .map(|&variant| tcx.adt_def(tcx.parent(variant)))
        {
            if !adt.is_enum()
                || adt.did().is_local()
                || !adt.is_variant_list_non_exhaustive()
                || !checked.insert(adt.did())
            {
                continue;
            }
            let Some(catch_all) = catch_all_of(cx, arms, adt) else {
                continue;
            };
            report(cx, adt, &named, catch_all);
        }
    }
}

/// The first pattern in a guardless arm that matches every value of the enum `adt` where it
/// stands: `_` or a plain binding whose type is or holds `adt`, as in `Some(_)` or a `_` arm on
/// an `Option` of it, or a `..` that leaves out fields holding `adt`, as in `Some(..)`.
fn catch_all_of<'tcx>(
    cx: &LateContext<'tcx>,
    arms: &[Arm<'tcx>],
    adt: AdtDef<'tcx>,
) -> Option<Span> {
    let typeck = cx.typeck_results();
    let holds = |ty: Ty<'tcx>, skip: usize| {
        ty.walk().skip(skip).any(|argument| {
            argument
                .as_type()
                .is_some_and(|inner| matches!(inner.kind(), ty::Adt(found, _) if *found == adt))
        })
    };
    let mut found = None;
    for arm in arms.iter().filter(|arm| arm.guard.is_none()) {
        arm.pat.walk_always(|pat| {
            if found.is_some() {
                return;
            }
            let ty = typeck.pat_ty(pat);
            let covers = match pat.kind {
                PatKind::Wild | PatKind::Binding(.., None) => holds(ty, 0),
                PatKind::TupleStruct(_, _, rest) | PatKind::Tuple(_, rest) => {
                    rest.as_opt_usize().is_some() && holds(ty, 1)
                }
                PatKind::Struct(_, _, Some(_)) => holds(ty, 1),
                _ => false,
            };
            if covers {
                found = Some(pat.span);
            }
        });
    }
    found
}

fn report(cx: &LateContext<'_>, adt: AdtDef<'_>, named: &FxHashSet<DefId>, catch_all: Span) {
    let tcx = cx.tcx;
    let enum_name = tcx.item_name(adt.did());
    let unnamed: Vec<String> = adt
        .variants()
        .iter()
        .filter(|variant| !named.contains(&variant.def_id) && is_nameable(tcx, variant.def_id))
        .map(|variant| format!("{enum_name}::{}", variant.name))
        .collect();
    if unnamed.is_empty() {
        return;
    }
    span_lint_and_help(
        cx,
        NON_EXHAUSTIVE_WILDCARD,
        catch_all,
        format!(
            "wildcard hides {} of non-exhaustive `{enum_name}`",
            if unnamed.len() == 1 {
                "1 variant".to_owned()
            } else {
                format!("{} variants", unnamed.len())
            }
        ),
        None,
        format!(
            "name {} in an arm, or as alternatives next to the wildcard, as the Google Rust Style \
             Guide's \"Don't omit patterns on `non_exhaustive` enums when matching\" asks",
            summarize(&unnamed)
        ),
    );
}

/// Adds the variant `pat` names, if it is a variant pattern.
fn insert_variant(cx: &LateContext<'_>, pat: &Pat<'_>, named: &mut FxHashSet<DefId>) {
    let (PatKind::Struct(qpath, ..)
    | PatKind::TupleStruct(qpath, ..)
    | PatKind::Expr(PatExpr {
        kind: PatExprKind::Path(qpath),
        ..
    })) = &pat.kind
    else {
        return;
    };
    match cx.typeck_results().qpath_res(qpath, pat.hir_id) {
        Res::Def(DefKind::Variant, id) => {
            named.insert(id);
        }
        Res::Def(DefKind::Ctor(..), id) => {
            if let Some(variant) = cx.tcx.opt_parent(id) {
                named.insert(variant);
            }
        }
        _ => {}
    }
}

/// Whether a user can write the variant on stable Rust without a warning.
fn is_nameable(tcx: TyCtxt<'_>, variant: DefId) -> bool {
    !tcx.is_doc_hidden(variant)
        && tcx.lookup_deprecation(variant).is_none()
        && tcx
            .lookup_stability(variant)
            .is_none_or(|stability| stability.is_stable())
}

fn summarize(variants: &[String]) -> String {
    if variants.len() <= LISTED_VARIANTS {
        return format!("`{}`", variants.join(" | "));
    }
    format!(
        "`{} | ...` and {} more",
        variants[..LISTED_VARIANTS].join(" | "),
        variants.len() - LISTED_VARIANTS
    )
}

#[test]
fn ui() {
    crate::ui_harness::ui_test(NON_EXHAUSTIVE_WILDCARD);
}
