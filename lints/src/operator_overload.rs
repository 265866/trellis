use clippy_utils::diagnostics::span_lint_and_help;
use rustc_attr_ir::lang_items::LangItem;
use rustc_hir::{Item, ItemKind};
use rustc_lint::{LateContext, LateLintPass};
use rustc_lint_defs::{declare_lint, declare_lint_pass};

declare_lint! {
    /// Checks for implementations of the overloadable operator traits in `core::ops`: `Add`,
    /// `Sub`, `Mul`, `Div`, `Rem`, `Neg`, `Not`, `BitAnd`, `BitOr`, `BitXor`, `Shl`, `Shr`, their
    /// `*Assign` forms, `Index` and `IndexMut`. Each one needs an `expect` whose reason says what
    /// the operator means for the type, so review can check the meaning is mathematical or
    /// logical. Impls a macro writes are left alone, but not one a proc macro copies from its
    /// caller's tokens: its author wrote it.
    ///
    /// The Google Rust Style Guide, "Avoid overloading for non-mathematical purposes": use
    /// operator overloads for their original mathematical or logical purpose, not as syntax
    /// sugar.
    ///
    /// ```rust,ignore
    /// impl Add<Order> for History {
    ///     type Output = History;
    ///     fn add(mut self, order: Order) -> History { self.orders.push(order); self }
    /// }
    /// ```
    ///
    /// Use instead a named method, and keep operators for mathematical meanings, which say so:
    ///
    /// ```rust,ignore
    /// impl History {
    ///     fn record(&mut self, order: Order) { self.orders.push(order); }
    /// }
    ///
    /// #[cfg_attr(
    ///     dylint_lib = "trellis_lints",
    ///     expect(operator_overload, reason = "adding two amounts sums their cents")
    /// )]
    /// impl Add for Amount {
    ///     type Output = Amount;
    ///     fn add(self, other: Amount) -> Amount { Amount(self.0 + other.0) }
    /// }
    /// ```
    pub OPERATOR_OVERLOAD,
    Warn,
    "an operator overload without a reason saying its meaning is mathematical"
}

declare_lint_pass!(OperatorOverload => [OPERATOR_OVERLOAD]);

impl<'tcx> LateLintPass<'tcx> for OperatorOverload {
    fn check_item(&mut self, cx: &LateContext<'tcx>, item: &'tcx Item<'tcx>) {
        if item.span.from_expansion() {
            return;
        }
        let ItemKind::Impl(implementation) = item.kind else {
            return;
        };
        let Some(header) = implementation.of_trait else {
            return;
        };
        let Some(operator) = header
            .trait_ref
            .trait_def_id()
            .and_then(|trait_id| cx.tcx.as_lang_item(trait_id))
            .and_then(operator)
        else {
            return;
        };
        let path = header.trait_ref.path;
        let name = path
            .segments
            .last()
            .map_or("", |segment| segment.ident.as_str());
        span_lint_and_help(
            cx,
            OPERATOR_OVERLOAD,
            path.span,
            format!("`{name}` impl overloads `{operator}`"),
            None,
            format!(
                "if `{operator}` keeps its original meaning for this type, say what it means in \
                 `#[cfg_attr(dylint_lib = \"trellis_lints\", expect(operator_overload, reason = \
                 \"...\"))]` on the impl; if it is shorthand for something else, use a named \
                 method, as the Google Rust Style Guide's \"Avoid overloading for \
                 non-mathematical purposes\" asks"
            ),
        );
    }
}

/// The operator an overloadable operator trait implements, or `None` for any other lang item.
fn operator(item: LangItem) -> Option<&'static str> {
    Some(match item {
        LangItem::Add => "+",
        LangItem::Sub => "-",
        LangItem::Mul => "*",
        LangItem::Div => "/",
        LangItem::Rem => "%",
        LangItem::Neg => "unary -",
        LangItem::Not => "!",
        LangItem::BitAnd => "&",
        LangItem::BitOr => "|",
        LangItem::BitXor => "^",
        LangItem::Shl => "<<",
        LangItem::Shr => ">>",
        LangItem::AddAssign => "+=",
        LangItem::SubAssign => "-=",
        LangItem::MulAssign => "*=",
        LangItem::DivAssign => "/=",
        LangItem::RemAssign => "%=",
        LangItem::BitAndAssign => "&=",
        LangItem::BitOrAssign => "|=",
        LangItem::BitXorAssign => "^=",
        LangItem::ShlAssign => "<<=",
        LangItem::ShrAssign => ">>=",
        LangItem::Index | LangItem::IndexMut => "[]",
        _ => return None,
    })
}

#[test]
fn ui() {
    crate::ui_harness::ui_test(OPERATOR_OVERLOAD);
}
