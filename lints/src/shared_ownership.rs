use clippy_utils::diagnostics::span_lint_and_help;
use rustc_hir::def::Res;
use rustc_hir::intravisit::{Visitor, walk_ty};
use rustc_hir::{
    AmbigArg, FieldDef, FnDecl, ImplItem, ImplItemKind, Item, ItemKind, QPath, TraitItem,
    TraitItemKind, Ty, TyKind,
};
use rustc_lint::{LateContext, LateLintPass};
use rustc_lint_defs::{declare_lint, declare_lint_pass};
use rustc_middle::ty::TyCtxt;
use rustc_span::{Symbol, sym};

declare_lint! {
    /// Checks for `Rc`, `Arc` and their `Weak` pointers in the types of struct, enum and union
    /// fields, in type aliases, in the types of statics and constants, associated ones included,
    /// and in the signatures of functions, inherent methods and trait method declarations. Trait
    /// impls are not checked, because the trait fixes their signatures.
    ///
    /// The Google Rust Style Guide, "Avoid `Rc` and `Arc`": prefer unique ownership and borrowing.
    /// Shared ownership is the exception for when the alternative would be unreasonably complex,
    /// so each use carries an `#[expect]` whose reason says why.
    ///
    /// Code a macro writes isn't checked. Code a proc macro copies from its caller's tokens, such
    /// as the body of a function under an attribute macro, is: its author wrote it there.
    ///
    /// ```rust,ignore
    /// struct Session {
    ///     settings: Rc<Settings>,
    /// }
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// struct Session<'settings> {
    ///     settings: &'settings Settings,
    /// }
    /// ```
    ///
    /// Or, where shared ownership is the simpler design:
    ///
    /// ```rust,ignore
    /// struct Session {
    ///     #[cfg_attr(
    ///         dylint_lib = "trellis_lints",
    ///         expect(shared_ownership, reason = "every open window reads the same settings")
    ///     )]
    ///     settings: Rc<Settings>,
    /// }
    /// ```
    pub SHARED_OWNERSHIP,
    Warn,
    "`Rc` or `Arc` in a struct field or function signature"
}

declare_lint_pass!(SharedOwnership => [SHARED_OWNERSHIP]);

impl<'tcx> LateLintPass<'tcx> for SharedOwnership {
    fn check_field_def(&mut self, cx: &LateContext<'tcx>, field: &'tcx FieldDef<'tcx>) {
        lint_pointers(cx, |finder| finder.visit_ty_unambig(field.ty), "field type");
    }

    fn check_item(&mut self, cx: &LateContext<'tcx>, item: &'tcx Item<'tcx>) {
        match item.kind {
            ItemKind::Fn { sig, .. } => lint_signature(cx, sig.decl),
            ItemKind::TyAlias(_, _, ty) => {
                lint_pointers(cx, |finder| finder.visit_ty_unambig(ty), "type alias");
            }
            ItemKind::Static(_, _, ty, _) => {
                lint_pointers(cx, |finder| finder.visit_ty_unambig(ty), "static");
            }
            ItemKind::Const(_, _, ty, _) => {
                lint_pointers(cx, |finder| finder.visit_ty_unambig(ty), "constant");
            }
            _ => {}
        }
    }

    fn check_impl_item(&mut self, cx: &LateContext<'tcx>, item: &'tcx ImplItem<'tcx>) {
        if crate::trait_impls::is_in_trait_impl(item) {
            return;
        }
        match item.kind {
            ImplItemKind::Fn(sig, _) => lint_signature(cx, sig.decl),
            ImplItemKind::Const(ty, _) => {
                lint_pointers(cx, |finder| finder.visit_ty_unambig(ty), "constant");
            }
            ImplItemKind::Type(_) => {}
        }
    }

    fn check_trait_item(&mut self, cx: &LateContext<'tcx>, item: &'tcx TraitItem<'tcx>) {
        match item.kind {
            TraitItemKind::Fn(sig, _) => lint_signature(cx, sig.decl),
            TraitItemKind::Const(ty, _) => {
                lint_pointers(cx, |finder| finder.visit_ty_unambig(ty), "constant");
            }
            TraitItemKind::Type(..) => {}
        }
    }
}

fn lint_signature<'tcx>(cx: &LateContext<'tcx>, decl: &'tcx FnDecl<'tcx>) {
    lint_pointers(
        cx,
        |finder| finder.visit_fn_decl(decl),
        "function signature",
    );
}

/// Lints each shared-ownership pointer that `walk` finds, saying it appears in a `place`.
fn lint_pointers<'tcx>(
    cx: &LateContext<'tcx>,
    walk: impl FnOnce(&mut PointerFinder<'tcx>),
    place: &str,
) {
    let mut finder = PointerFinder {
        tcx: cx.tcx,
        found: Vec::new(),
    };
    walk(&mut finder);
    for (ty, pointer) in finder.found {
        if ty.span.from_expansion() {
            continue;
        }
        span_lint_and_help(
            cx,
            SHARED_OWNERSHIP,
            ty.span,
            format!("`{pointer}` in a {place}"),
            None,
            "prefer unique ownership and borrows; if shared ownership is simpler, say why with \
             `#[cfg_attr(dylint_lib = \"trellis_lints\", expect(shared_ownership, reason = \
             \"...\"))]`",
        );
    }
}

/// Collects the written types that name `Rc`, `Arc` or one of their `Weak` pointers.
struct PointerFinder<'tcx> {
    tcx: TyCtxt<'tcx>,
    found: Vec<(&'tcx Ty<'tcx>, &'static str)>,
}

impl<'tcx> Visitor<'tcx> for PointerFinder<'tcx> {
    fn visit_ty(&mut self, ty: &'tcx Ty<'tcx, AmbigArg>) {
        if let TyKind::Path(QPath::Resolved(_, path)) = ty.kind
            && let Res::Def(_, def_id) = path.res
            && let Some(pointer) = self.tcx.get_diagnostic_name(def_id).and_then(pointer_name)
        {
            self.found.push((ty.as_unambig_ty(), pointer));
        }
        walk_ty(self, ty);
    }
}

fn pointer_name(diagnostic_item: Symbol) -> Option<&'static str> {
    match diagnostic_item {
        sym::Rc => Some("Rc"),
        sym::Arc => Some("Arc"),
        sym::RcWeak => Some("rc::Weak"),
        sym::ArcWeak => Some("sync::Weak"),
        _ => None,
    }
}

#[test]
fn ui() {
    crate::ui_harness::ui_test(SHARED_OWNERSHIP);
}
