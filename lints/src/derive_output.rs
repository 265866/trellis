//! Telling the code a derive writes from its caller's tokens apart from the code around it, for
//! the few lints that would otherwise misreport what a derive writes at the caller's spans.
//!
//! A derive builds its impls from the type's tokens and keeps their spans, so a path it writes
//! can point at the caller's source while its text isn't there: `#[derive(thiserror::Error)]`
//! writes `::core::convert::From` at a `#[from]`. clippy's `is_from_proc_macro` notices the text
//! doesn't match, but it also misreads hand-written code whose text differs from its name, such
//! as a raw identifier. So a lint skips a node only if both hold: a derive wrote an item around
//! it, and its text doesn't match. Hand-written code, whatever its text, is never inside a
//! derive's output. A path a derive parses from a string the caller wrote, as serde does for
//! `#[serde(default = "std::string::String::new")]`, has the string's span: its text doesn't
//! match either, but the caller wrote the path there, so it isn't skipped. The string's value
//! counts, not its text, read as tokens, so no escape, comment, raw identifier or generic argument
//! hides the path in it.

use clippy_utils::is_from_proc_macro;
use rustc_hir::{HirId, Path};
use rustc_lint::{LateContext, LintContext};
use rustc_span::hygiene::MacroKind;
use rustc_span::{ExpnId, ExpnKind, Span, Symbol, kw};

use crate::string_literal;

/// The names of the segments of `path`, as written.
pub(crate) fn path_names(path: &Path<'_>) -> Vec<Symbol> {
    path.segments
        .iter()
        .map(|segment| segment.ident.name)
        .collect()
}

/// Whether the source at `span` holds the path whose segments are `names`, ignoring whitespace and
/// in the composed form the compiler interns names in, or, if it is a string literal, whether its
/// value may spell the path, as `string_literal::Literal::may_spell` says. A lint skips a node in a
/// derive's output only if `is_from_proc_macro` holds for it and this doesn't.
pub(crate) fn holds_path(cx: &LateContext<'_>, span: Span, names: &[Symbol]) -> bool {
    if span.from_expansion() {
        return false;
    }
    let Ok(text) = cx.sess().source_map().span_to_snippet(span) else {
        return false;
    };
    let names: Vec<&str> = names
        .iter()
        .filter(|&&name| name != kw::PathRoot && name != kw::DollarCrate)
        .map(Symbol::as_str)
        .collect();
    if let Some(literal) = string_literal::literal(&text) {
        return literal.may_spell(&names);
    }
    let text: String = string_literal::normalized(&text)
        .split_whitespace()
        .collect();
    text.contains(&names.join("::"))
}

/// The runs of `path`'s segments that no macro wrote, each as a path of its own spanning just
/// those segments.
///
/// A lint reads these rather than `path.span`, which can come from a macro's expansion while
/// segments in it come from the source. serde parses `#[serde(with = "a::b")]` into a path at the
/// string's span and appends `deserialize` at the derive's, which gives `a::b::deserialize`, and a
/// type in it such as the `a::B` of `a::B::deserialize`, the derive's span.
pub(crate) fn source_runs<'hir>(path: &Path<'hir>) -> impl Iterator<Item = Path<'hir>> {
    let res = path.res;
    path.segments
        .split(|segment| segment.ident.span.from_expansion())
        .filter_map(move |run| {
            let (first, last) = (run.first()?, run.last()?);
            Some(Path {
                span: first.ident.span.to(last.ident.span),
                res,
                segments: run,
            })
        })
}

/// Whether a derive wrote `path`, a run of the source segments of a path at `hir_id`, as code of
/// its own at the caller's spans rather than a path the caller wrote.
pub(crate) fn derive_wrote(cx: &LateContext<'_>, hir_id: HirId, path: &Path<'_>) -> bool {
    in_derive_output(cx, hir_id)
        && is_from_proc_macro(cx, path)
        && !holds_path(cx, path.span, &path_names(path))
}

/// Whether the caller wrote the name `name` at `span` in a node at `hir_id`, which a derive may
/// have built if `node_from_proc_macro` holds: `span` is outside any macro's expansion, and the
/// source there holds the name if a derive wrote the node.
pub(crate) fn caller_wrote_name(
    cx: &LateContext<'_>,
    hir_id: HirId,
    span: Span,
    name: Symbol,
    node_from_proc_macro: impl FnOnce() -> bool,
) -> bool {
    !span.from_expansion()
        && (!in_derive_output(cx, hir_id)
            || !node_from_proc_macro()
            || holds_path(cx, span, &[name]))
}

/// Whether an item around `hir_id`, or the item it is, was written by a derive.
pub(crate) fn in_derive_output(cx: &LateContext<'_>, hir_id: HirId) -> bool {
    std::iter::once(hir_id.owner)
        .chain(cx.tcx.hir_parent_owner_iter(hir_id).map(|(owner, _)| owner))
        .any(|owner| written_by_derive(cx.tcx.expn_that_defined(owner.to_def_id())))
}

/// Whether `expansion`, or one it is inside, is a derive's.
fn written_by_derive(mut expansion: ExpnId) -> bool {
    while expansion != ExpnId::root() {
        let data = expansion.expn_data();
        if matches!(data.kind, ExpnKind::Macro(MacroKind::Derive, _)) {
            return true;
        }
        expansion = data.parent;
    }
    false
}
