// edition:2024

// Flagged: a glob import of another crate's prelude.
use std::io::prelude::*;

// Flagged: a glob import of a module inside another crate's prelude.
use core::prelude::rust_2024::*;

// Flagged: a prelude glob in a `use` list.
use std::io::{self, prelude::*};

mod reexport {
    pub use std::io::prelude as io_prelude;
}

// Flagged: another crate's prelude re-exported under another name.
use reexport::io_prelude::*;

mod prelude {
    pub struct Shelf;
}

// Not flagged: this crate's own prelude, which `prelude_module` reports.
use prelude::*;

// Not flagged: another crate's module that isn't a prelude, which `wildcard_imports` reports.
use std::collections::*;

// Not flagged: items imported by name from a prelude.
use std::io::prelude::BufRead;

// Not flagged: an `expect` on the `use` covers it.
#[cfg_attr(
    dylint_lib = "trellis_lints",
    expect(foreign_prelude_glob, reason = "the fixture checks an item-level expect")
)]
use core::prelude::v1::*;

macro_rules! prelude_glob {
    () => {
        use std::io::prelude::*;
    };
}

fn read() {
    // Flagged: inside a function too.
    use std::io::prelude::*;

    // Not flagged: macro output.
    prelude_glob!();
}

fn main() {}
