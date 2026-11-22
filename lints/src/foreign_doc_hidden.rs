use std::ptr;

use clippy_utils::diagnostics::span_lint_and_help;
use clippy_utils::is_from_proc_macro;
use rustc_hir::def::{DefKind, Res};
use rustc_hir::def_id::DefId;
use rustc_hir::{
    Expr, ExprKind, HirId, Item, ItemKind, Node, Pat, PatExpr, PatExprKind, PatKind, Path, QPath,
    UseTree,
};
use rustc_lint::{LateContext, LateLintPass, LintContext};
use rustc_lint_defs::{declare_lint, declare_lint_pass};
use rustc_middle::ty::{self, Ty, TyCtxt};
use rustc_span::{Ident, Span};

use crate::derive_output::{caller_wrote_name, derive_wrote, source_runs};

declare_lint! {
    /// Checks for paths, method calls, field accesses and patterns that reach an item or field from
    /// another crate marked `#[doc(hidden)]`, or an associated item of a hidden trait or impl.
    ///
    /// The Google Rust Style Guide, "Do not access items marked `#[doc(hidden)]` that you don't
    /// own": treat hidden items as private. They are public only so the owning crate's macros can
    /// reach them, and can change in any release.
    ///
    /// Names that macros write are skipped, since the macro's crate owns those uses, and so are
    /// names a derive writes at the caller's spans, such as the `AsDisplay` that
    /// `#[derive(thiserror::Error)]` writes for a `{field}` in `#[error(..)]`. A name the caller
    /// wrote is checked wherever a macro puts it: code a proc macro copies, the path serde parses
    /// from `#[serde(with = "...")]` and adds `deserialize` to, and the fields
    /// `#[serde(remote = "...")]` reads from the type it mirrors. A crate's root is never hidden
    /// here, though `windows` and `windows-sys` hide their own to keep their whole API out of the
    /// docs: each item and module under it is checked on its own. So a crate that hides its root to
    /// mark the whole crate private isn't flagged either, only its hidden items and modules. Crates whose names start with a prefix in
    /// `owned_crate_prefixes` are owned, so their hidden items are fair to use:
    ///
    /// ```toml
    /// [trellis_lints]
    /// owned_crate_prefixes = ["trellis", "acme_"]
    /// ```
    ///
    /// ```rust,ignore
    /// let shelves = std::vec::from_elem(Shelf::default(), 5);
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// let shelves = vec![Shelf::default(); 5];
    /// ```
    pub FOREIGN_DOC_HIDDEN,
    Warn,
    "use of a `#[doc(hidden)]` item from another crate"
}

declare_lint_pass!(ForeignDocHidden => [FOREIGN_DOC_HIDDEN]);

impl<'tcx> LateLintPass<'tcx> for ForeignDocHidden {
    /// Checks each segment of `path` that the source wrote. A `use` visits its path once per
    /// namespace it imports from, so a `use` is checked on the first visit only, and its name is
    /// flagged only when it is hidden in every namespace: a `use` of a name that is both a
    /// documented trait and a hidden derive macro imports the trait too.
    fn check_path(&mut self, cx: &LateContext<'tcx>, path: &Path<'tcx>, hir_id: HirId) {
        let Some(last) = path.segments.last() else {
            return;
        };
        let mut name_is_hidden = true;
        let tree = match cx.tcx.hir_node(hir_id) {
            Node::Item(Item {
                kind: ItemKind::Use(tree),
                ..
            })
            | Node::NestedUseTree(tree) => Some(tree),
            _ => None,
        };
        if let Some(UseTree {
            prefix: use_path, ..
        }) = tree
        {
            if use_path.res.present_items().next() != Some(path.res) {
                return;
            }
            name_is_hidden = use_path
                .res
                .present_items()
                .all(|res| hidden_foreign_item(cx.tcx, res).is_some());
        }
        for run in source_runs(path) {
            if derive_wrote(cx, hir_id, &run) {
                continue;
            }
            for segment in run.segments {
                if !ptr::eq(segment, last) {
                    check(cx, segment.res, segment.ident.span);
                } else if name_is_hidden {
                    check(cx, path.res, segment.ident.span);
                }
            }
        }
    }

    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        let written = |name: Ident| {
            caller_wrote_name(cx, expr.hir_id, name.span, name.name, || {
                expr.span.from_expansion() || is_from_proc_macro(cx, expr)
            })
        };
        match expr.kind {
            ExprKind::MethodCall(segment, ..) => {
                if written(segment.ident)
                    && let Some(id) = cx.typeck_results().type_dependent_def_id(expr.hir_id)
                {
                    check(cx, Res::Def(cx.tcx.def_kind(id), id), segment.ident.span);
                }
            }
            ExprKind::Path(ref qpath) => check_type_relative(cx, qpath, expr.hir_id, written),
            ExprKind::Struct(qpath, fields, _) => {
                check_type_relative(cx, qpath, expr.hir_id, written);
                let ty = cx.typeck_results().expr_ty(expr);
                for field in fields.iter().filter(|field| written(field.ident)) {
                    check_field(cx, ty, qpath, expr.hir_id, field.hir_id, field.ident);
                }
            }
            ExprKind::Field(base, name) => {
                let typeck = cx.typeck_results();
                if written(name)
                    && let Some(index) = typeck.opt_field_index(expr.hir_id)
                    && let ty::Adt(adt, _) = typeck.expr_ty_adjusted(base).peel_refs().kind()
                    && !adt.is_enum()
                {
                    let field = adt.non_enum_variant().fields[index].did;
                    check(cx, Res::Def(DefKind::Field, field), name.span);
                }
            }
            _ => {}
        }
    }

    fn check_pat(&mut self, cx: &LateContext<'tcx>, pat: &'tcx Pat<'tcx>) {
        let written = |name: Ident| {
            caller_wrote_name(cx, pat.hir_id, name.span, name.name, || {
                pat.span.from_expansion() || is_from_proc_macro(cx, pat)
            })
        };
        match pat.kind {
            PatKind::Struct(ref qpath, fields, _) => {
                check_type_relative(cx, qpath, pat.hir_id, written);
                let ty = cx.typeck_results().pat_ty(pat);
                for field in fields.iter().filter(|field| written(field.ident)) {
                    check_field(cx, ty, qpath, pat.hir_id, field.hir_id, field.ident);
                }
            }
            PatKind::TupleStruct(ref qpath, ..) => {
                check_type_relative(cx, qpath, pat.hir_id, written);
            }
            PatKind::Expr(&PatExpr {
                kind: PatExprKind::Path(ref qpath),
                hir_id,
                ..
            }) => check_type_relative(cx, qpath, hir_id, written),
            _ => {}
        }
    }
}

/// Checks the associated item a type-relative path such as `u8::MAX` reaches, if `written` says
/// the caller wrote its name; `check_path` sees only paths that resolve without types. `hir_id` is
/// where typeck recorded the path's resolution, which for a path pattern is its `PatExpr`, not the
/// `Pat` around it.
fn check_type_relative(
    cx: &LateContext<'_>,
    qpath: &QPath<'_>,
    hir_id: HirId,
    written: impl FnOnce(Ident) -> bool,
) {
    if let QPath::TypeRelative(_, segment) = qpath
        && written(segment.ident)
    {
        check(
            cx,
            cx.typeck_results().qpath_res(qpath, hir_id),
            segment.ident.span,
        );
    }
}

/// Checks the field named `name` in a struct expression or pattern, of type `ty`, that `qpath`
/// names.
fn check_field(
    cx: &LateContext<'_>,
    ty: Ty<'_>,
    qpath: &QPath<'_>,
    hir_id: HirId,
    field_hir_id: HirId,
    name: Ident,
) {
    let typeck = cx.typeck_results();
    let ty::Adt(adt, _) = ty.peel_refs().kind() else {
        return;
    };
    let variant = match typeck.qpath_res(qpath, hir_id) {
        Res::Def(DefKind::Variant, id) => adt.variant_with_id(id),
        _ if !adt.is_enum() => adt.non_enum_variant(),
        _ => return,
    };
    if let Some(index) = typeck.opt_field_index(field_hir_id) {
        check(
            cx,
            Res::Def(DefKind::Field, variant.fields[index].did),
            name.span,
        );
    }
}

fn check(cx: &LateContext<'_>, res: Res, span: Span) {
    let Some(hidden) = hidden_foreign_item(cx.tcx, res) else {
        return;
    };
    let tcx = cx.tcx;
    let krate = tcx.crate_name(hidden.krate);
    if crate::config::get(cx.sess())
        .owned_crate_prefixes
        .iter()
        .any(|prefix| krate.as_str().starts_with(prefix.as_str()))
    {
        return;
    }
    span_lint_and_help(
        cx,
        FOREIGN_DOC_HIDDEN,
        span,
        format!(
            "`{}` is `#[doc(hidden)]` in `{krate}`",
            tcx.def_path_str(hidden)
        ),
        None,
        "use the crate's documented API instead, as the Google Rust Style Guide's \"Do not access items \
         marked `#[doc(hidden)]` that you don't own\" asks",
    );
}

/// The hidden item `res` reaches in another crate: the item itself, the type or variant a
/// constructor builds, or the trait or impl an associated item belongs to. A crate root is never
/// one: a crate such as `windows` hides its own root so docs.rs doesn't build its whole API, and
/// its items still say on their own whether they are hidden.
fn hidden_foreign_item(tcx: TyCtxt<'_>, res: Res) -> Option<DefId> {
    let Res::Def(kind, id) = res else {
        return None;
    };
    if id.is_local() {
        return None;
    }
    let item = if matches!(kind, DefKind::Ctor(..)) {
        tcx.parent(id)
    } else {
        id
    };
    if item.is_crate_root() {
        return None;
    }
    if tcx.is_doc_hidden(item) {
        return Some(item);
    }
    let container = matches!(
        kind,
        DefKind::AssocFn | DefKind::AssocConst | DefKind::AssocTy
    )
    .then(|| tcx.parent(item))
    .filter(|&parent| tcx.is_doc_hidden(parent));
    container.map(|_| item)
}

#[test]
fn ui() {
    crate::ui_harness::ui_test_of(FOREIGN_DOC_HIDDEN, "ui/foreign_doc_hidden")
        .dylint_toml(crate::ui_harness::dylint_toml_with(
            "owned_crate_prefixes",
            Some(r#"["alloc"]"#),
        ))
        .run();
}
