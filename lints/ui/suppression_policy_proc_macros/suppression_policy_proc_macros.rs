// aux-build:hush.rs
// The test's `dylint.toml` sets `trusted_macro_crates = ["alloc", "core", "std"]`.

extern crate hush;

// Flagged: an `allow` that an attribute macro from a crate `trusted_macro_crates` doesn't list
// writes from the item's tokens.
#[hush::hush]
fn first() {}

// Flagged: an `allow` that a derive from that crate writes from the type's name.
#[derive(hush::Hush)]
struct Second;

// Flagged: an `allow` that a function-like macro from that crate writes from its argument,
// which no item a macro wrote is around, so nothing says which macro wrote it.
fn third() {
    hush::hushed_block!({
        let unused = 1;
    });
}

// Flagged: an `allow` written in the source.
#[allow(dead_code)]
fn fourth() {}

fn main() {
    let _ = (first(), Second, third(), fourth());
}
