mod shelf {
    pub struct Shelf;
}

mod inline {
    // Flagged: an inline module named `prelude`.
    pub mod prelude {
        pub use crate::shelf::Shelf;
    }
}

// Flagged: a module named `prelude` loaded from its own file.
#[path = "modules/prelude.rs"]
mod prelude;

// Not flagged: the name only contains `prelude`.
mod preludes {}

// Not flagged: importing another crate's prelude is the reviewer's call, not a definition.
use std::io::prelude::*;

macro_rules! define_prelude {
    () => {
        mod prelude {}
    };
}

mod generated {
    // Not flagged: the module comes from a macro expansion.
    define_prelude!();
}

fn main() {}

// Not reported: flagged, but an item-level `expect` in the documented form takes it.
mod excused {
    #[cfg_attr(
        dylint_lib = "trellis_lints",
        expect(prelude_module, reason = "the fixture checks an item-level expect")
    )]
    pub mod prelude {}
}
