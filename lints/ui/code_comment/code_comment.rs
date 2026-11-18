//! Flagged and allowed comments for `code_comment`.
#![doc = include_str!("modules/README.md")]

#[path = "modules/helper.rs"]
mod helper;

#[path = "modules/hidden.rs"]
mod hidden;

#[cfg(any())]
#[path = "modules/elsewhere.rs"]
mod elsewhere;

const SNIPPET: &str = include_str!("modules/snippet.rs");

/// Not flagged: doc comments document the item.
fn order() -> u64 {
    // Flagged: a line comment.
    let price = 7;
    /* Flagged: a block comment. */
    price
}

fn first(values: &[u64]) -> u64 {
    // SAFETY: not flagged, because clippy requires `SAFETY:` comments on unsafe blocks, and
    // this second line belongs to the same comment.
    unsafe { *values.as_ptr() }
}

fn second(values: &[u64]) -> u64 {
    // SAFETY: not flagged: the `unsafe` is on the line after the comment.
    let value = unsafe { *values.as_ptr().add(1) };
    value
}

fn third(values: &[u64]) -> u64 {
    unsafe { *values.as_ptr().add(2) } // SAFETY: not flagged: the `unsafe` is on its line.
}

/// Marks types that are all zeros when zeroed.
unsafe trait Zeroable {}

// SAFETY: not flagged: attributes and doc comments between it and the `unsafe` are skipped.
#[allow(dead_code)]
/// `u64` is all zeros when zeroed.
unsafe impl Zeroable for u64 {}

fn smuggled() -> u64 {
    let total = 1 + 1; // SAFETY: flagged: no unsafe code on this line or after it.
    // SAFETY: flagged on its own, since a comment at the end of a line takes in no lines below
    // it. The next line of code is safe.
    let doubled = total * 2;
    doubled
}

fn split() -> u64 {
    // SAFETY: flagged: the blank line below ends the comment, and no unsafe code follows it.

    // Flagged: a separate comment after the blank line.
    // SAFETY: not flagged: a contiguous run that ends right above the `unsafe`.
    unsafe { *[4_u64].as_ptr() }
}

fn main() {
    let text = "// not flagged: inside a string literal";
    let _ = (
        order(),
        first(&[1, 2, 3]),
        second(&[1, 2, 3]),
        third(&[1, 2, 3]),
        smuggled(),
        split(),
        helper::helper(),
        text,
        SNIPPET,
    );
}
