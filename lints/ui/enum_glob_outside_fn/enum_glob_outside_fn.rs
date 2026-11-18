// edition:2024

pub enum Fruit {
    Cherry,
    Plum,
}

mod shelves {
    pub enum Shelf {
        Left,
        Right,
    }
}

// Flagged: an enum glob at module scope.
use Fruit::*;

mod nested {
    // Flagged: an enum glob inside a list import.
    use crate::shelves::Shelf::{self, *};

    // Flagged: a public enum glob is still a glob outside a function.
    pub use std::cmp::Ordering::*;
}

// Not flagged: a module glob is a different rule.
use shelves::*;

fn price(fruit: Fruit) -> u64 {
    // Not flagged: an enum glob inside a function that deals with the enum.
    use Fruit::*;

    match fruit {
        Cherry => 2,
        Plum => 50,
    }
}

impl Fruit {
    fn is_plum(&self) -> bool {
        // Not flagged: an enum glob inside a method of the enum.
        use Fruit::*;

        matches!(self, Plum)
    }
}

// Not flagged: an enum glob inside a constant's body.
const FIRST: Fruit = {
    use Fruit::*;
    Cherry
};

macro_rules! import_variants {
    () => {
        use crate::shelves::Shelf::*;
    };
}

mod generated {
    // Not flagged: the glob comes from a macro expansion.
    import_variants!();
}

fn main() {}

// Not reported: flagged, but an item-level `expect` in the documented form takes it.
#[cfg_attr(
    dylint_lib = "trellis_lints",
    expect(
        enum_glob_outside_fn,
        reason = "the fixture checks an item-level expect"
    )
)]
use shelves::Shelf::*;
