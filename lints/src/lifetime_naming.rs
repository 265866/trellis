use clippy_utils::diagnostics::span_lint_and_help;
use rustc_ast::visit::FnKind;
use rustc_ast::{GenericParam, GenericParamKind, Generics, Item, ItemKind, NodeId};
use rustc_lint::{EarlyContext, EarlyLintPass};
use rustc_lint_defs::{declare_lint, declare_lint_pass};
use rustc_span::Span;

declare_lint! {
    /// Checks for single-letter lifetime parameters declared on a struct, enum, union, trait,
    /// impl or type alias, and for functions and methods that declare more than two
    /// single-letter lifetime parameters.
    ///
    /// The Google Rust Style Guide, "Naming lifetimes": lifetimes should have descriptive names,
    /// especially type lifetimes. Single-letter lifetimes are fine for relating a method's inputs
    /// to its output, up to two of them. Higher-ranked `for<'a>` lifetimes and `'_` are not
    /// checked.
    ///
    /// ```rust,ignore
    /// struct Parser<'a> {
    ///     input: &'a str,
    /// }
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// struct Parser<'input> {
    ///     input: &'input str,
    /// }
    /// ```
    pub LIFETIME_NAMING,
    Warn,
    "single-letter lifetimes on a type, trait or impl, or more than two on a function"
}

declare_lint_pass!(LifetimeNaming => [LIFETIME_NAMING]);

impl EarlyLintPass for LifetimeNaming {
    fn check_item(&mut self, cx: &EarlyContext<'_>, item: &Item) {
        let (generics, kind) = match &item.kind {
            ItemKind::Struct(_, generics, _) => (generics, "a struct"),
            ItemKind::Enum(_, generics, _) => (generics, "an enum"),
            ItemKind::Union(_, generics, _) => (generics, "a union"),
            ItemKind::Trait(definition) => (&definition.generics, "a trait"),
            ItemKind::Impl(implementation) => (&implementation.generics, "an impl"),
            ItemKind::TyAlias(alias) => (&alias.generics, "a type alias"),
            _ => return,
        };
        for lifetime in single_letter_lifetimes(generics) {
            span_lint_and_help(
                cx,
                LIFETIME_NAMING,
                lifetime.ident.span,
                format!("single-letter lifetime `{}` on {kind}", lifetime.ident),
                None,
                "name it after what it borrows from, such as `'input`",
            );
        }
    }

    fn check_fn(&mut self, cx: &EarlyContext<'_>, kind: FnKind<'_>, _: Span, _: NodeId) {
        let FnKind::Fn(_, _, function) = kind else {
            return;
        };
        let count = single_letter_lifetimes(&function.generics).count();
        if count <= 2 {
            return;
        }
        span_lint_and_help(
            cx,
            LIFETIME_NAMING,
            function.generics.span,
            format!(
                "`{}` declares {count} single-letter lifetimes",
                function.ident
            ),
            None,
            "name them after what they borrow from; up to two single-letter lifetimes are fine",
        );
    }
}

/// The lifetime parameters of `generics` named with one letter, written in the source rather
/// than by a macro.
fn single_letter_lifetimes(generics: &Generics) -> impl Iterator<Item = &GenericParam> {
    generics.params.iter().filter(|param| {
        let name = param.ident.name.as_str();
        let mut letters = name.trim_start_matches('\'').chars();
        matches!(param.kind, GenericParamKind::Lifetime)
            && !param.ident.span.from_expansion()
            && letters.next().is_some_and(char::is_alphabetic)
            && letters.next().is_none()
    })
}

#[test]
fn ui() {
    crate::ui_harness::ui_test(LIFETIME_NAMING);
}
