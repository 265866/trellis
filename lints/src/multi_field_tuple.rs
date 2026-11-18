use clippy_utils::diagnostics::span_lint_and_help;
use rustc_hir::intravisit::{Visitor, walk_generic_args, walk_ty};
use rustc_hir::{
    AmbigArg, FieldDef, FnDecl, FnRetTy, GenericArg, GenericArgs, GenericArgsParentheses, ImplItem,
    ImplItemKind, Item, ItemKind, TraitItem, TraitItemKind, Ty, TyKind, Variant, VariantData,
};
use rustc_lint::{LateContext, LateLintPass};
use rustc_lint_defs::{declare_lint, declare_lint_pass};
use rustc_middle::ty;
use rustc_span::Ident;

declare_lint! {
    /// Checks for tuple structs and tuple enum variants with two or more fields, not counting
    /// `PhantomData` markers, and for tuples of two or more elements written anywhere in the type
    /// of a field, a type alias, or a function's parameters or return type: `(u64, u64)` itself,
    /// or inside another type, as in `Result<(u64, u64), Error>`, `Vec<(u32, String)>`,
    /// `HashMap<Id, (usize, usize)>` or `impl Iterator<Item = (u64, bool)>`. An `async fn`'s
    /// return type counts. A closure trait's parameter list, as in `Box<dyn Fn(A, B)>`, isn't a
    /// tuple, but a tuple among its parameters or in its return type is. Trait impls aren't
    /// checked, since the trait fixes their signatures.
    ///
    /// The Google Rust Style Guide, "Prefer named fields over tuples and non-newtype tuple
    /// structs": tuple indexes such as `.0` and `.1` quickly become confusing. Its exceptions are
    /// newtypes, tuple structs with one field, and very local contexts, so closures and `let`
    /// bindings aren't checked.
    ///
    /// Code a macro writes isn't checked. Code a proc macro copies from its caller's tokens, such
    /// as the body of a function under an attribute macro, is: its author wrote it there.
    ///
    /// ```rust,ignore
    /// struct Range(u64, u64);
    ///
    /// fn order() -> Option<(u64, bool)> { ... }
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// struct Range {
    ///     start: u64,
    ///     end: u64,
    /// }
    ///
    /// struct Order {
    ///     price: u64,
    ///     discount: bool,
    /// }
    ///
    /// fn order() -> Option<Order> { ... }
    /// ```
    pub MULTI_FIELD_TUPLE,
    Warn,
    "a tuple struct or variant with two or more fields, or a tuple in a field, alias or signature"
}

declare_lint_pass!(MultiFieldTuple => [MULTI_FIELD_TUPLE]);

impl<'tcx> LateLintPass<'tcx> for MultiFieldTuple {
    fn check_item(&mut self, cx: &LateContext<'tcx>, item: &'tcx Item<'tcx>) {
        if item.span.from_expansion() {
            return;
        }
        match item.kind {
            ItemKind::Struct(ident, _, VariantData::Tuple(fields, ..)) => {
                check_tuple_fields(cx, ident, "tuple struct", fields);
            }
            ItemKind::Fn { sig, .. } => check_signature(cx, sig.decl),
            ItemKind::TyAlias(_, _, ty) => lint_tuples(cx, ty, "type alias"),
            _ => {}
        }
    }

    fn check_variant(&mut self, cx: &LateContext<'tcx>, variant: &'tcx Variant<'tcx>) {
        if variant.span.from_expansion() {
            return;
        }
        if let VariantData::Tuple(fields, ..) = variant.data {
            check_tuple_fields(cx, variant.ident, "tuple variant", fields);
        }
    }

    fn check_field_def(&mut self, cx: &LateContext<'tcx>, field: &'tcx FieldDef<'tcx>) {
        if field.span.from_expansion() {
            return;
        }
        let place = if field.is_positional() {
            "field".to_owned()
        } else {
            format!("field `{}`", field.ident)
        };
        lint_tuples(cx, field.ty, &place);
    }

    fn check_impl_item(&mut self, cx: &LateContext<'tcx>, item: &'tcx ImplItem<'tcx>) {
        let ImplItemKind::Fn(sig, _) = item.kind else {
            return;
        };
        if crate::trait_impls::is_in_trait_impl(item) {
            return;
        }
        check_signature(cx, sig.decl);
    }

    fn check_trait_item(&mut self, cx: &LateContext<'tcx>, item: &'tcx TraitItem<'tcx>) {
        if let TraitItemKind::Fn(sig, _) = item.kind {
            check_signature(cx, sig.decl);
        }
    }
}

/// Checks the fields of a tuple struct or tuple variant named `ident`. `PhantomData` markers carry
/// no data, so they don't count, and `Id<T>(u64, PhantomData<T>)` is a newtype.
fn check_tuple_fields(cx: &LateContext<'_>, ident: Ident, kind: &str, fields: &[FieldDef<'_>]) {
    let data_fields = fields
        .iter()
        .filter(|field| {
            let ty = cx
                .tcx
                .type_of(field.def_id)
                .instantiate_identity()
                .skip_norm_wip();
            !matches!(ty.kind(), ty::Adt(adt, _) if adt.is_phantom_data())
        })
        .count();
    if data_fields >= 2 {
        span_lint_and_help(
            cx,
            MULTI_FIELD_TUPLE,
            ident.span,
            format!("{kind} `{ident}` has {data_fields} fields"),
            None,
            "name the fields, as the Google Rust Style Guide's \"Prefer named fields over tuples \
             and non-newtype tuple structs\" asks",
        );
    }
}

fn check_signature<'tcx>(cx: &LateContext<'tcx>, decl: &'tcx FnDecl<'tcx>) {
    for input in decl.inputs {
        lint_tuples(cx, input, "parameter");
    }
    if let FnRetTy::Return(output) = decl.output {
        lint_tuples(cx, output, "return type");
    }
}

/// Lints each tuple of two or more elements written in `ty`, which is a `place` such as a field
/// or a return type.
fn lint_tuples<'tcx>(cx: &LateContext<'tcx>, ty: &'tcx Ty<'tcx>, place: &str) {
    let mut finder = TupleFinder { found: Vec::new() };
    finder.visit_ty_unambig(ty);
    for (ty, elements) in finder.found {
        if ty.span.from_expansion() {
            continue;
        }
        span_lint_and_help(
            cx,
            MULTI_FIELD_TUPLE,
            ty.span,
            format!("{place} holds a tuple of {elements}"),
            None,
            "use a struct with named fields, as the Google Rust Style Guide's \"Prefer named \
             fields over tuples and non-newtype tuple structs\" asks",
        );
    }
}

/// Collects the outermost tuples of two or more elements in a type, wherever they sit: in an
/// `Option`, a `Vec`, a map's values, a reference, or an `impl Iterator<Item = ..>`.
struct TupleFinder<'tcx> {
    found: Vec<(&'tcx Ty<'tcx>, usize)>,
}

impl<'tcx> Visitor<'tcx> for TupleFinder<'tcx> {
    fn visit_ty(&mut self, ty: &'tcx Ty<'tcx, AmbigArg>) {
        if let TyKind::Tup(elements) = ty.kind
            && elements.len() >= 2
        {
            self.found.push((ty.as_unambig_ty(), elements.len()));
            return;
        }
        walk_ty(self, ty);
    }

    /// `Fn(A, B) -> C`, in `dyn`, `impl` or a bound, lowers to `Fn<(A, B), Output = C>`. That
    /// tuple is the closure's parameter list rather than a tuple the code wrote, so only its
    /// elements are checked, as a function's parameters are; `Fn((A, B))` still has one.
    fn visit_generic_args(&mut self, args: &'tcx GenericArgs<'tcx>) {
        if args.parenthesized != GenericArgsParentheses::ParenSugar {
            walk_generic_args(self, args);
            return;
        }
        for arg in args.args {
            if let GenericArg::Type(inputs) = arg
                && let TyKind::Tup(inputs) = inputs.kind
            {
                for input in inputs {
                    self.visit_ty_unambig(input);
                }
            } else {
                self.visit_generic_arg(arg);
            }
        }
        for constraint in args.constraints {
            self.visit_assoc_item_constraint(constraint);
        }
    }
}

#[test]
fn ui() {
    crate::ui_harness::ui_test(MULTI_FIELD_TUPLE);
}
