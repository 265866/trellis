use clippy_utils::diagnostics::span_lint_and_help;
use clippy_utils::res::MaybeDef as _;
use rustc_ast::LitKind;
use rustc_attr_ir::lang_items::LangItem;
use rustc_hir::def::{DefKind, Res};
use rustc_hir::{Expr, ExprKind};
use rustc_lint::{LateContext, LateLintPass};
use rustc_lint_defs::{declare_lint, declare_lint_pass};
use rustc_span::Symbol;

declare_lint! {
    /// Checks for `str::replace` or `str::replacen` called on a fixed template: a string literal,
    /// or a `const` string or a `String` made from a literal, as in `String::from("..")` or
    /// `"..".to_owned()`, whose replaced pattern is a placeholder such as `"{amount}"`,
    /// `"$amount"` or `"%amount%"`. Replacing other text in a `const`, as a test does when it
    /// edits a fixture document, isn't templating, so it passes.
    ///
    /// The Google Rust Style Guide, "Prefer `format!` / `write!` for concatenation and
    /// substitution": use `format!` to perform substitutions on a literal string instead of
    /// `str::replace`, and prefer standard format strings to inventing your own templating
    /// syntax.
    ///
    /// `format!` needs the literal itself, so a `const` template moves into the `format!` call,
    /// or into a function that returns it.
    ///
    /// Code a macro writes isn't checked. Code a proc macro copies from its caller's tokens, such
    /// as the body of a function under an attribute macro, is: its author wrote it there.
    ///
    /// ```rust,ignore
    /// let banner = "You won {amount}!".replace("{amount}", &amount.to_string());
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// let banner = format!("You won {amount}!");
    /// ```
    pub STR_REPLACE_ON_LITERAL,
    Warn,
    "`str::replace` on a string literal or constant used as a template"
}

declare_lint_pass!(StrReplaceOnLiteral => [STR_REPLACE_ON_LITERAL]);

impl<'tcx> LateLintPass<'tcx> for StrReplaceOnLiteral {
    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        let ExprKind::MethodCall(segment, receiver, arguments, _) = expr.kind else {
            return;
        };
        let name = segment.ident.name;
        if !matches!(name.as_str(), "replace" | "replacen")
            || expr.span.from_expansion()
            || receiver.span.from_expansion()
        {
            return;
        }
        let Some(template) = template(cx, receiver) else {
            return;
        };
        if template != Template::Literal && !arguments.first().is_some_and(is_placeholder) {
            return;
        }
        let tcx = cx.tcx;
        let is_str_method = cx
            .typeck_results()
            .type_dependent_def_id(expr.hir_id)
            .and_then(|method| tcx.inherent_impl_of_assoc(method))
            .is_some_and(|implementation| {
                tcx.type_of(implementation)
                    .instantiate_identity()
                    .skip_normalization()
                    .is_str()
            });
        if !is_str_method {
            return;
        }
        span_lint_and_help(
            cx,
            STR_REPLACE_ON_LITERAL,
            expr.span,
            format!("`str::{name}` on {}", template.description()),
            None,
            "write the template as a `format!` string with named arguments, as the Google Rust \
             Style Guide's \"Prefer `format!` / `write!` for concatenation and substitution\" asks",
        );
    }
}

/// A fixed string that `replace` fills in like a template.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Template {
    Literal,
    Const,
    StringFromLiteral,
}

impl Template {
    fn description(self) -> &'static str {
        match self {
            Self::Literal => "a string literal",
            Self::Const => "a `const` string",
            Self::StringFromLiteral => "a `String` made from a literal",
        }
    }
}

/// What kind of fixed template `receiver` is, if it is one: a string literal, a `const`, or a
/// `String` made from a literal.
fn template(cx: &LateContext<'_>, receiver: &Expr<'_>) -> Option<Template> {
    match receiver.kind {
        ExprKind::Lit(literal) if matches!(literal.node, LitKind::Str(..)) => {
            Some(Template::Literal)
        }
        ExprKind::Path(ref path)
            if matches!(
                cx.qpath_res(path, receiver.hir_id),
                Res::Def(DefKind::Const | DefKind::AssocConst, _)
            ) =>
        {
            Some(Template::Const)
        }
        ExprKind::Call(_, [argument]) | ExprKind::MethodCall(_, argument, [], _)
            if string_literal(argument).is_some()
                && cx
                    .typeck_results()
                    .expr_ty(receiver)
                    .is_lang_item(cx, LangItem::String) =>
        {
            Some(Template::StringFromLiteral)
        }
        _ => None,
    }
}

/// Whether `pattern` is a string literal shaped like a template's placeholder: `{name}`,
/// `{{name}}`, `${name}`, `$name` or `%name%`. A `const` or a `String` may be a document a test
/// edits, such as a manifest, rather than a template, so only a placeholder marks it as one.
fn is_placeholder(pattern: &Expr<'_>) -> bool {
    let Some(text) = string_literal(pattern) else {
        return false;
    };
    let text = text.as_str();
    let name = text
        .strip_prefix("{{")
        .and_then(|rest| rest.strip_suffix("}}"))
        .or_else(|| {
            text.strip_prefix("${")
                .and_then(|rest| rest.strip_suffix('}'))
        })
        .or_else(|| {
            text.strip_prefix('{')
                .and_then(|rest| rest.strip_suffix('}'))
        })
        .or_else(|| {
            text.strip_prefix('%')
                .and_then(|rest| rest.strip_suffix('%'))
        })
        .or_else(|| text.strip_prefix('$'));
    name.is_some_and(|name| {
        !name.is_empty()
            && name
                .chars()
                .all(|character| character.is_alphanumeric() || character == '_')
    })
}

fn string_literal(expr: &Expr<'_>) -> Option<Symbol> {
    match expr.kind {
        ExprKind::Lit(literal) => match literal.node {
            LitKind::Str(text, _) => Some(text),
            _ => None,
        },
        _ => None,
    }
}

#[test]
fn ui() {
    crate::ui_harness::ui_test(STR_REPLACE_ON_LITERAL);
}
