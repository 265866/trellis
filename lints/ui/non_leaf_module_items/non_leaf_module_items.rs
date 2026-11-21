// compile-flags: --test
// edition:2024

// Not flagged: `extern crate`, imports and re-exports belong in a non-leaf module.
extern crate core;
pub use shelf::Shelf;
use std::fmt;

// Not flagged: child module declarations.
#[path = "modules/machine.rs"]
mod machine;
#[path = "modules/shelf.rs"]
mod shelf;

// Flagged: a struct in a crate root that has child modules.
pub struct Engine;

// Flagged: an impl block.
impl Engine {
    fn start(&self) {}
}

// Flagged: a function other than `main`.
fn helper() {}

// Flagged: a constant.
const LIMIT: u8 = 3;

// Flagged: an inline leaf module with definitions puts code in this file.
mod inline_leaf {
    pub fn price() -> u64 {
        0
    }
}

// Not flagged: an inline module holding only re-exports.
pub mod api {
    pub use crate::shelf::Shelf;
}

// Not flagged here: an inline non-leaf module reports its own items.
mod nested {
    mod child {}

    // Flagged: a function in an inline non-leaf module.
    fn tie() {}

    // Not flagged here: an `expect` on the item covers it, in an inline module, and
    // `suppression_policy` allows it there.
    #[cfg_attr(
        dylint_lib = "trellis_lints",
        expect(non_leaf_module_items, reason = "the fixture checks an item-level expect")
    )]
    fn excused() {}
}

// Not flagged here: an `expect` on the item covers it, in the crate root, and
// `suppression_policy` allows it there.
#[cfg_attr(
    dylint_lib = "trellis_lints",
    expect(non_leaf_module_items, reason = "the fixture checks an item-level expect")
)]
pub fn init() {}

// Not flagged here: an `expect` on an inline leaf module covers it, and
// `suppression_policy` allows it there.
#[cfg_attr(
    dylint_lib = "trellis_lints",
    expect(non_leaf_module_items, reason = "the fixture checks an item-level expect")
)]
mod excused_leaf {
    pub fn price() {}
}

// Not flagged: a parent module must define a macro before the children that use it.
macro_rules! make_helper {
    () => {
        fn generated() {}
    };
}

// Not flagged: macro output.
make_helper!();

// Not flagged: `#[test]` functions only exist in test builds.
#[test]
fn engine_starts() {}

// Not flagged: `#[cfg(test)]` modules.
#[cfg(test)]
mod tests {
    fn fixture() {}
}

// Not flagged: `main` in a binary crate root.
fn main() {}

// Not flagged here: an `expect` on an impl block covers it, and `suppression_policy` allows it
// there, since the lint reports at the block.
#[cfg_attr(
    dylint_lib = "trellis_lints",
    expect(non_leaf_module_items, reason = "the fixture checks a block-level expect")
)]
impl Engine {
    fn stop(&self) {}
}
