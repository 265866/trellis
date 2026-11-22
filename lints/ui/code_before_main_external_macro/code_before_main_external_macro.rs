// edition:2024
// aux-build:registry.rs
// aux-build:actions_derive.rs
// aux-build:gui.rs
//! Constructors that a dependency's macros write, as GPUI's do.

extern crate gui;

// Flagged at the call: `actions!` declares a struct with a derive, whose impl's function holds a
// `submit!` that writes the static inside a `const _`.
gui::actions!(Open);

// Flagged at the derive: the same derive, written directly.
#[derive(gui::Action)]
pub struct Close;

fn main() {}
