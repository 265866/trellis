use clippy_utils::ast_utils::is_cfg_test;
use clippy_utils::diagnostics::span_lint_and_help;
use rustc_ast::{Inline, Item, ItemKind, ModKind, VisibilityKind};
use rustc_lint::{EarlyContext, EarlyLintPass};
use rustc_lint_defs::{declare_lint, declare_lint_pass};
use rustc_span::Ident;

use crate::test_code::is_test_item;

declare_lint! {
    /// Checks for leaf modules loaded from their own file (`mod foo;`) that have a primary type
    /// or trait the module isn't named after.
    ///
    /// The Google Rust Style Guide, "Name source files and leaf modules after the primary
    /// user-defined item": a leaf module about `Foo` is `mod foo` in `foo.rs`, so the file name
    /// says what is inside.
    ///
    /// The primary item is the module's only visible struct, enum, union, trait or type alias, or,
    /// when it has several, the one whose name starts every other one's name (`Price` beside
    /// `PriceError`). Private types are helpers and don't count. A module with no clear primary
    /// item, such as `Point` beside `Size`, or public functions beside a private helper type, can
    /// be named as desired and isn't checked. Neither is a module with a type, trait or function
    /// named after it, a module with child modules other than `#[cfg(test)]` ones, nor a test
    /// module.
    ///
    /// The guide also lets a module that isn't primarily about any one abstraction be named as
    /// desired, such as helpers that return one public type among others, which the lint can't
    /// tell apart. Such a module takes an `expect` on its `mod` declaration, where the lint
    /// reports. `suppression_policy` rejects one on a module with another module beneath it, other
    /// than a `#[cfg(test)]` module that holds no module, so it can't cover another module's report.
    ///
    /// ```rust,ignore
    /// // lib.rs
    /// mod settings;
    ///
    /// // settings.rs
    /// pub struct Config { /* ... */ }
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// // lib.rs
    /// mod config;
    ///
    /// // config.rs
    /// pub struct Config { /* ... */ }
    /// ```
    pub FILE_NAMED_AFTER_ITEM,
    Warn,
    "a module whose file isn't named after its primary item"
}

declare_lint_pass!(FileNamedAfterItem => [FILE_NAMED_AFTER_ITEM]);

impl EarlyLintPass for FileNamedAfterItem {
    fn check_item(&mut self, cx: &EarlyContext<'_>, item: &Item) {
        if item.span.from_expansion() || is_cfg_test(item) {
            return;
        }
        let ItemKind::Mod(_, module, ModKind::Loaded(children, Inline::No { .. }, _)) = &item.kind
        else {
            return;
        };
        if matches!(module.name.as_str(), "test" | "tests") {
            return;
        }
        let children: Vec<&Item> = children
            .iter()
            .map(|child| &**child)
            .filter(|child| !child.span.from_expansion() && !is_test_item(child, children))
            .collect();
        if children
            .iter()
            .any(|child| matches!(child.kind, ItemKind::Mod(..)))
        {
            return;
        }
        let module_key = name_key(module.name.as_str());
        let mut types = Vec::new();
        for child in &children {
            let is_type = match child.kind {
                ItemKind::Struct(..)
                | ItemKind::Enum(..)
                | ItemKind::Union(..)
                | ItemKind::Trait(..)
                | ItemKind::TraitAlias(..)
                | ItemKind::TyAlias(..) => true,
                ItemKind::Fn(..) => false,
                _ => continue,
            };
            let Some(name) = child.kind.ident() else {
                continue;
            };
            if name_key(name.name.as_str()) == module_key {
                return;
            }
            if is_type {
                types.push(*child);
            }
        }
        let Some(primary) = primary_item(&types) else {
            return;
        };
        let primary = primary.name.as_str();
        span_lint_and_help(
            cx,
            FILE_NAMED_AFTER_ITEM,
            module.span,
            format!("module `{module}` isn't named after its primary item `{primary}`"),
            None,
            format!(
                "rename the module and its file to `{}`, as the Google Rust Style Guide's \"Name \
                 source files and leaf modules after the primary user-defined item\" asks",
                snake_case(primary)
            ),
        );
    }
}

/// The module's primary type, if it has a clear one: the only visible type, or the visible type
/// whose name starts every other visible type's name. Private types are helpers, so a module
/// whose types are all private has none.
fn primary_item(types: &[&Item]) -> Option<Ident> {
    let names: Vec<Ident> = types
        .iter()
        .filter(|item| !matches!(item.vis.kind, VisibilityKind::Inherited))
        .filter_map(|item| item.kind.ident())
        .collect();
    names.iter().copied().find(|primary| {
        names
            .iter()
            .all(|name| name.name.as_str().starts_with(primary.name.as_str()))
    })
}

/// Compares names across case conventions: `AppView`, `app_view` and `APP_VIEW` share a key.
fn name_key(name: &str) -> String {
    name.chars()
        .filter(|&c| c != '_')
        .flat_map(char::to_lowercase)
        .collect()
}

/// `AppView` to `app_view`, keeping acronyms together: `HTTPServer` to `http_server`.
fn snake_case(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    let mut snake = String::with_capacity(name.len() + 4);
    for (i, &c) in chars.iter().enumerate() {
        if c.is_uppercase() && i > 0 {
            let previous = chars[i - 1];
            let next_is_lower = chars.get(i + 1).is_some_and(|next| next.is_lowercase());
            if previous.is_lowercase()
                || previous.is_ascii_digit()
                || (previous.is_uppercase() && next_is_lower)
            {
                snake.push('_');
            }
        }
        snake.extend(c.to_lowercase());
    }
    snake
}

#[test]
fn ui() {
    crate::ui_harness::ui_test(FILE_NAMED_AFTER_ITEM);
}

#[test]
fn snake_case_keeps_acronyms_together() {
    assert_eq!(
        snake_case("AppView"),
        "app_view",
        "camel case should split at each word"
    );
    assert_eq!(
        snake_case("HTTPServer"),
        "http_server",
        "an acronym should stay one word"
    );
    assert_eq!(
        snake_case("Vec3Builder"),
        "vec3_builder",
        "a digit should end a word"
    );
    assert_eq!(
        snake_case("Order"),
        "order",
        "one word should only be lowercased"
    );
}
