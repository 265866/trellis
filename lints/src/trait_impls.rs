//! Questions about trait impls that several lints ask.

use clippy_utils::sym;
use rustc_hir::{ImplItem, ImplItemImplKind};
use rustc_middle::ty::{Ty, TyCtxt};

/// Whether `item` is in a trait impl, which takes its names and signatures from the trait, so
/// lints about those leave it alone.
pub(crate) fn is_in_trait_impl(item: &ImplItem<'_>) -> bool {
    matches!(item.impl_kind, ImplItemImplKind::Trait { .. })
}

/// Whether some impl names `ty` as an implementor of `core::error::Error`. It looks for an impl
/// rather than proving `ty: Error`, so a generic error whose impl needs bounds on its parameters
/// still counts. The impl of an error type defined in this crate is in this crate too, as the
/// orphan rule requires.
pub(crate) fn implements_error<'tcx>(tcx: TyCtxt<'tcx>, ty: Ty<'tcx>) -> bool {
    tcx.get_diagnostic_item(sym::Error)
        .is_some_and(|error| tcx.non_blanket_impls_for_ty(error, ty).next().is_some())
}
