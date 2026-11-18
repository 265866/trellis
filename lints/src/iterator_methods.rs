//! Which method calls are calls of `Iterator` methods, which several lints ask.

use rustc_hir::def_id::DefId;
use rustc_lint::LateContext;
use rustc_span::{Symbol, sym};

/// Whether `method` is a method of `Iterator` or `DoubleEndedIterator`.
pub(crate) fn is_iterator_method(cx: &LateContext<'_>, method: DefId) -> bool {
    let tcx = cx.tcx;
    let Some(owner) = tcx.trait_of_assoc(method) else {
        return false;
    };
    tcx.is_diagnostic_item(sym::Iterator, owner)
        || tcx.is_diagnostic_item(Symbol::intern("DoubleEndedIterator"), owner)
}
