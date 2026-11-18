// aux-build:hush.rs
// aux-build:swap.rs
// The test's `dylint.toml` sets `trusted_macro_crates = ["alloc", "core", "std", "hush"]`.

extern crate hush;
extern crate swap;

// Not flagged: an `allow` that an attribute macro from a crate `trusted_macro_crates` lists
// writes from the item's tokens.
#[hush::hush]
fn first() {}

// Not flagged: an `allow` that a derive from that crate writes from the type's name.
#[derive(hush::Hush)]
struct Second;

// Still flagged: an `allow` that a function-like macro from that crate writes from its argument,
// which no item a macro wrote is around, so nothing says which macro wrote it.
fn third() {
    hush::hushed_block!({
        let unused = 1;
    });
}

// Still flagged: an `allow` written in the source.
#[allow(dead_code)]
fn fourth() {}

macro_rules! with_meta {
    ($meta:meta; $body:block) => {
        #[$meta]
        $body
    };
}

// Still flagged: inside an item a trusted macro wrote, an `allow` whose `#[` and `]` a macro this
// crate defines writes around the caller's tokens.
#[hush::hush]
fn fifth() {
    with_meta!(allow(unused_variables); {
        let unused = 1;
    });
}

// Still flagged: inside an item a trusted macro wrote, an `allow` that a function-like macro from
// a crate `trusted_macro_crates` doesn't list built from the tokens of its call, which keep their
// spans in the source, so nothing in them says a macro wrote them.
#[hush::hush]
fn sixth() {
    swap::swap! { quiet allow
        #[quiet(unused_variables)]
        let unused = 1;
    }
}

fn main() {
    let _ = (first(), Second, third(), fourth(), fifth(), sixth());
}
