use clippy_utils::diagnostics::span_lint_and_help;
use clippy_utils::res::MaybeDef as _;
use clippy_utils::source::snippet;
use rustc_attr_ir::lang_items::LangItem;
use rustc_hir::def::Res;
use rustc_hir::{Item, ItemKind, Mutability, PrimTy, QPath, Ty, TyKind};
use rustc_lint::{LateContext, LateLintPass};
use rustc_lint_defs::{declare_lint, declare_lint_pass};

declare_lint! {
    /// Checks for type aliases without generic parameters that directly name `bool`, `char`, an
    /// integer or float type, `String` or `&'static str`.
    ///
    /// The Google Rust Style Guide, "Newtypes versus type aliases": use a newtype when the type
    /// has its own meaning or API, may change later, or is easy to mix up with another use of the
    /// same primitive. An alias like `type Cents = u64` lets any `u64` pass as `Cents`.
    ///
    /// Aliases named in `snake_case`, the C convention for FFI typedefs such as `c_int`, aren't
    /// checked, nor are aliases of other aliases.
    ///
    /// Code a macro writes isn't checked. Code a proc macro copies from its caller's tokens, such
    /// as the body of a function under an attribute macro, is: its author wrote it there.
    ///
    /// ```rust,ignore
    /// type Cents = u64;
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// struct Cents(u64);
    /// ```
    pub PRIMITIVE_TYPE_ALIAS,
    Warn,
    "a type alias for a primitive or `String` where a newtype fits"
}

declare_lint_pass!(PrimitiveTypeAlias => [PRIMITIVE_TYPE_ALIAS]);

impl<'tcx> LateLintPass<'tcx> for PrimitiveTypeAlias {
    fn check_item(&mut self, cx: &LateContext<'tcx>, item: &'tcx Item<'tcx>) {
        if item.span.from_expansion() {
            return;
        }
        let ItemKind::TyAlias(ident, generics, ty) = item.kind else {
            return;
        };
        if !generics.params.is_empty()
            || ident
                .name
                .as_str()
                .starts_with(|first: char| first.is_ascii_lowercase())
            || !is_primitive_like(cx, ty)
        {
            return;
        }
        let aliased = snippet(cx, ty.span, "..");
        span_lint_and_help(
            cx,
            PRIMITIVE_TYPE_ALIAS,
            item.span,
            format!("type alias `{ident}` for `{aliased}`"),
            None,
            format!(
                "use a newtype such as `struct {ident}({aliased});`, as the Google Rust Style \
                 Guide's \"Newtypes versus type aliases\" asks"
            ),
        );
    }
}

/// Whether the alias names a scalar primitive, `String`, or a shared `str` reference itself,
/// rather than through another alias.
fn is_primitive_like(cx: &LateContext<'_>, ty: &Ty<'_>) -> bool {
    match ty.kind {
        TyKind::Path(QPath::Resolved(None, path)) => match path.res {
            Res::PrimTy(primitive) => !matches!(primitive, PrimTy::Str),
            res => res.is_lang_item(cx, LangItem::String),
        },
        TyKind::Ref(_, inner, Mutability::Not) => matches!(
            inner.kind,
            TyKind::Path(QPath::Resolved(None, path)) if path.res == Res::PrimTy(PrimTy::Str)
        ),
        _ => false,
    }
}

#[test]
fn ui() {
    crate::ui_harness::ui_test(PRIMITIVE_TYPE_ALIAS);
}
