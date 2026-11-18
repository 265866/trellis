//! Whether code is compiled only for tests, which several lints leave out.

use rustc_ast::{AttrKind, GeneratedAttr, Item, ItemKind};
use rustc_hir::HirId;
use rustc_middle::ty::TyCtxt;
use rustc_span::Span;

/// Whether the AST item `item`, one of the items `siblings`, is test-only: a `#[cfg(test)]` item,
/// or a `#[test]` function.
pub(crate) fn is_test_item(item: &Item, siblings: &[Box<Item>]) -> bool {
    clippy_utils::ast_utils::is_cfg_test(item) || test_functions(siblings).contains(&item.span)
}

/// The spans of the `#[test]` functions among `items`. The test harness writes a static beside
/// each, at the function's span, which it marks for the test runner.
pub(crate) fn test_functions(items: &[Box<Item>]) -> Vec<Span> {
    let markers: Vec<Span> = items
        .iter()
        .filter(|item| {
            item.attrs.iter().any(|attribute| {
                matches!(
                    attribute.kind,
                    AttrKind::Generated(GeneratedAttr::RustcTestMarker(_))
                )
            })
        })
        .map(|item| item.span)
        .collect();
    items
        .iter()
        .filter(|item| {
            matches!(item.kind, ItemKind::Fn(_))
                && !item.span.from_expansion()
                && markers
                    .iter()
                    .any(|marker| marker.lo_hi() == item.span.lo_hi())
        })
        .map(|item| item.span)
        .collect()
}

/// Whether the HIR node `id` is test-only: a `#[cfg(test)]` item or anything in one, or a
/// `#[test]` function or anything in one.
pub(crate) fn is_test_code(tcx: TyCtxt<'_>, id: HirId) -> bool {
    clippy_utils::is_cfg_test(tcx, id) || clippy_utils::is_in_test(tcx, id)
}
