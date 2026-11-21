use clippy_utils::ast_utils::is_cfg_test;
use clippy_utils::diagnostics::span_lint_and_help;
use rustc_ast::{Crate, Inline, Item, ItemKind, ModKind};
use rustc_lint::{EarlyContext, EarlyLintPass, LintContext};
use rustc_lint_defs::{declare_lint, impl_lint_pass};
use rustc_span::{Span, sym};
use rustc_structures::CrateType;

use crate::test_code::{is_test_item, test_functions};

declare_lint! {
    /// Checks for items other than `mod` declarations, `use` imports and re-exports, and
    /// `extern crate` in a module that has child modules. A binary crate root may also hold
    /// `fn main`.
    ///
    /// The Google Rust Style Guide, "Don't implement APIs in non-leaf modules": "The only items in
    /// non-leaf modules should be module declarations (not definitions) and re-exports." Keeping
    /// code in leaf modules makes it easy to find and keeps structure apart from contents.
    ///
    /// `#[cfg(test)]` modules and items are ignored, so a test module doesn't make a module a
    /// non-leaf. An inline child module that holds definitions is reported too, because its code
    /// lives in the non-leaf module's file; one holding only re-exports is fine. `macro_rules!`
    /// definitions are allowed, since a parent module must define a macro before the child
    /// modules that use it. Each item is reported at the item itself.
    ///
    /// The guide allows a small amount of core "ties things together" implementation code in a
    /// non-leaf module, which the lint can't tell from an API, so such an item takes an `expect`
    /// saying what it ties together. It goes on the item the lint reports: a function, a type, an
    /// impl block, a trait, or an inline leaf module such as a sealed trait's `mod sealed`.
    /// `suppression_policy` allows an `expect` of this lint on an impl block, a trait and a module
    /// too, and rejects one on an item with a module beneath it, other than a `#[cfg(test)]` module
    /// that holds no module, so the `expect` covers only that one report.
    ///
    /// ```rust,ignore
    /// // lib.rs
    /// mod shelf;
    ///
    /// pub struct Machine { /* ... */ }
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// // lib.rs
    /// mod machine;
    /// mod shelf;
    ///
    /// pub use machine::Machine;
    /// ```
    pub NON_LEAF_MODULE_ITEMS,
    Warn,
    "a module with submodules that holds more than declarations and re-exports"
}

/// Reports each item from its own `check_item`, where its lint attributes apply, so the pass
/// tracks what encloses the item being visited.
#[derive(Default)]
pub struct NonLeafModuleItems {
    /// One entry per item the visit is inside, innermost last, with the crate root first.
    scopes: Vec<Scope>,
}

impl_lint_pass!(NonLeafModuleItems => [NON_LEAF_MODULE_ITEMS]);

/// Whether the direct children of an enclosing item are checked.
enum Scope {
    /// A module with child modules. The binary crate root may also hold `fn main`.
    NonLeafModule {
        is_binary_root: bool,
        /// The spans of the module's `#[test]` functions.
        tests: Vec<Span>,
    },
    /// A leaf module, a test-only or macro-generated module, or an item other than a module.
    Unchecked,
}

impl EarlyLintPass for NonLeafModuleItems {
    /// Opens the scope of the crate root, which is a binary's if the crate types include an
    /// executable or are empty. Cargo passes `--crate-type`; a bare `rustc main.rs` builds a
    /// binary.
    fn check_crate(&mut self, cx: &EarlyContext<'_>, krate: &Crate) {
        let crate_types = &cx.sess().opts.crate_types;
        let is_binary = crate_types.is_empty() || crate_types.contains(&CrateType::Executable);
        self.scopes = vec![module_scope(&krate.items, is_binary)];
    }

    fn check_item(&mut self, cx: &EarlyContext<'_>, item: &Item) {
        let (is_binary_root, tests) = match self.scopes.last() {
            Some(Scope::NonLeafModule {
                is_binary_root,
                tests,
            }) => (Some(*is_binary_root), tests.as_slice()),
            _ => (None, [].as_slice()),
        };
        let is_written =
            !item.span.from_expansion() && !is_cfg_test(item) && !tests.contains(&item.span);
        if is_written && let Some(is_binary_root) = is_binary_root {
            check_child(cx, item, is_binary_root);
        }
        self.scopes.push(match &item.kind {
            ItemKind::Mod(_, _, ModKind::Loaded(items, ..)) if is_written => {
                module_scope(items, false)
            }
            _ => Scope::Unchecked,
        });
    }

    fn check_item_post(&mut self, _: &EarlyContext<'_>, _: &Item) {
        self.scopes.pop();
    }
}

fn module_scope(items: &[Box<Item>], is_binary_root: bool) -> Scope {
    if has_child_modules(&written_items(items)) {
        Scope::NonLeafModule {
            is_binary_root,
            tests: test_functions(items),
        }
    } else {
        Scope::Unchecked
    }
}

fn has_child_modules(items: &[&Item]) -> bool {
    items
        .iter()
        .any(|item| matches!(item.kind, ItemKind::Mod(..)))
}

/// Checks `item`, written directly in a module with child modules. An inline module that has
/// child modules itself isn't reported, since its own items are reported when they are checked.
fn check_child(cx: &EarlyContext<'_>, item: &Item, is_binary_root: bool) {
    match &item.kind {
        ItemKind::Mod(_, ident, ModKind::Loaded(children, Inline::Yes, _)) => {
            let children = written_items(children);
            let only_reexports = children
                .iter()
                .all(|child| matches!(child.kind, ItemKind::Use(..) | ItemKind::ExternCrate(..)));
            if !has_child_modules(&children) && !only_reexports {
                span_lint_and_help(
                    cx,
                    NON_LEAF_MODULE_ITEMS,
                    ident.span,
                    format!("inline module `{ident}` defines items in a non-leaf module"),
                    None,
                    "move the module to its own file, as the Google Rust Style Guide's \"Don't \
                     implement APIs in non-leaf modules\" asks",
                );
            }
        }
        ItemKind::Use(..)
        | ItemKind::ExternCrate(..)
        | ItemKind::MacroDef(..)
        | ItemKind::Mod(..) => {}
        ItemKind::Fn(function) if is_binary_root && function.ident.name == sym::main => {}
        kind => {
            let what = kind.ident().map_or_else(
                || kind.descr().to_owned(),
                |ident| format!("{} `{ident}`", kind.descr()),
            );
            span_lint_and_help(
                cx,
                NON_LEAF_MODULE_ITEMS,
                item_head(item),
                format!("{what} in a module with child modules"),
                None,
                "move it into a child module and re-export it if needed; a non-leaf module should \
                 hold only `mod` declarations and re-exports, as the Google Rust Style Guide's \
                 \"Don't implement APIs in non-leaf modules\" asks",
            );
        }
    }
}

/// The items written in the module itself, leaving out macro output and test-only items.
fn written_items(items: &[Box<Item>]) -> Vec<&Item> {
    items
        .iter()
        .map(|item| &**item)
        .filter(|item| !item.span.from_expansion() && !is_test_item(item, items))
        .collect()
}

/// A short span to report an item at: its name, or an `impl` block's header.
fn item_head(item: &Item) -> Span {
    if let Some(ident) = item.kind.ident() {
        return ident.span;
    }
    match &item.kind {
        ItemKind::Impl(implementation) => item.span.with_hi(implementation.self_ty.span.hi()),
        ItemKind::ForeignMod(foreign) => foreign.extern_span,
        _ => item.span,
    }
}

#[test]
fn ui() {
    crate::ui_harness::ui_test(NON_LEAF_MODULE_ITEMS);
}
