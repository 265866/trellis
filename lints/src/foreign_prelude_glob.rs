use clippy_utils::diagnostics::span_lint_and_help;
use rustc_hir::Item;
use rustc_lint::{LateContext, LateLintPass};
use rustc_lint_defs::{declare_lint, declare_lint_pass};
use rustc_span::sym;

use crate::imports::{Import, imports};

declare_lint! {
    /// Checks for glob imports from another crate through a module named `prelude`, such as
    /// `use rayon::prelude::*` or `use std::io::prelude::*`.
    ///
    /// The Google Rust Style Guide, "Don't use glob imports", leaves third-party preludes to the
    /// reviewer; trellis doesn't allow them. Clippy's `wildcard_imports` exempts any path through
    /// a `prelude`, and a prelude glob brings every trait it holds into the whole module, which
    /// also hides the module-level trait imports that `module_level_trait_import` checks. The
    /// crate's own preludes are `prelude_module`'s concern.
    ///
    /// Code a macro writes isn't checked. Code a proc macro copies from its caller's tokens, such
    /// as the body of a function under an attribute macro, is: its author wrote it there.
    ///
    /// ```rust,ignore
    /// use rayon::prelude::*;
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
    /// ```
    pub FOREIGN_PRELUDE_GLOB,
    Warn,
    "a glob import of another crate's prelude"
}

declare_lint_pass!(ForeignPreludeGlob => [FOREIGN_PRELUDE_GLOB]);

impl<'tcx> LateLintPass<'tcx> for ForeignPreludeGlob {
    fn check_item(&mut self, cx: &LateContext<'tcx>, item: &'tcx Item<'tcx>) {
        if item.span.from_expansion() {
            return;
        }
        for import in imports(cx.tcx, item) {
            if import.name.is_none() {
                check(cx, &import);
            }
        }
    }
}

/// Reports the glob import `import` if it imports a foreign prelude: a module of another crate
/// whose path has a `prelude` segment, as the paths `wildcard_imports` exempts have, or a module
/// named `prelude` re-exported under another name.
fn check(cx: &LateContext<'_>, import: &Import<'_>) {
    let Some(module) = import
        .res
        .type_ns
        .and_then(|res| res.opt_def_id())
        .filter(|module| !module.is_local())
    else {
        return;
    };
    let is_prelude = import
        .segments
        .iter()
        .any(|segment| segment.ident.name == sym::prelude)
        || cx.tcx.opt_item_name(module) == Some(sym::prelude);
    if !is_prelude {
        return;
    }
    let module = import
        .segments
        .iter()
        .map(|segment| segment.ident.as_str())
        .collect::<Vec<_>>()
        .join("::");
    span_lint_and_help(
        cx,
        FOREIGN_PRELUDE_GLOB,
        import.path_span,
        format!("glob import of `{module}`"),
        None,
        "import the items you use by name, as the Google Rust Style Guide's \"Don't use glob \
         imports\" asks",
    );
}

#[test]
fn ui() {
    crate::ui_harness::ui_test(FOREIGN_PRELUDE_GLOB);
}
