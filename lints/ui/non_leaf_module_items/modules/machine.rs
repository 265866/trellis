#[path = "machine_parts.rs"]
mod parts;

pub use parts::Lever;

// Flagged: an enum in a non-leaf file module.
pub enum State {
    Idle,
}

// Flagged: `main` is only allowed in the crate root.
fn main() {}

// Not flagged here: an `expect` on the item covers it, in a file module, and
// `suppression_policy` allows it there.
#[cfg_attr(
    dylint_lib = "trellis_lints",
    expect(non_leaf_module_items, reason = "the fixture checks an item-level expect")
)]
fn wire() {}

// Flagged: `lever`, but not the struct in its body, which isn't the module's item.
fn lever() -> Lever {
    struct Local;
    let _ = Local;
    Lever
}
