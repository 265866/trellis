// compile-flags: --test
//! `expect`s of `file_named_after_item` and `non_leaf_module_items` on items with modules beneath
//! them, which would cover those modules' reports too. A test build, so `#[cfg(test)]` modules are
//! part of it.

// Flagged: a module whose function body holds a module with a child module.
#[cfg_attr(
    dylint_lib = "trellis_lints",
    expect(non_leaf_module_items, reason = "the runner ties the startup steps together")
)]
#[path = "modules/runner.rs"]
mod runner;

// Flagged: a module whose `#[cfg(test)]` child declares a module of its own.
#[cfg_attr(
    dylint_lib = "trellis_lints",
    expect(file_named_after_item, reason = "the module gathers widget helpers, not only Widget")
)]
#[path = "modules/extra.rs"]
mod extra;

// Flagged: an inline module whose `#[cfg(test)]` child holds a module with a child module.
#[cfg_attr(
    dylint_lib = "trellis_lints",
    expect(
        non_leaf_module_items,
        reason = "the sealed trait keeps other crates from implementing the public traits"
    )
)]
mod sealed {
    pub trait Sealed {}

    #[cfg(test)]
    mod tests {
        mod helpers {
            mod deeper {}

            fn assemble() {}

            #[test]
            fn assembles() {
                assemble();
            }
        }
    }
}

pub struct Meter(u32);

// Flagged: an impl block whose method body holds a module with a child module.
#[cfg_attr(
    dylint_lib = "trellis_lints",
    expect(non_leaf_module_items, reason = "the reading joins the child modules' units")
)]
impl Meter {
    fn read(&self) -> u32 {
        mod inner {
            pub(super) mod deeper {}

            pub(super) const fn two() -> u32 {
                2
            }
        }
        self.0 + inner::two()
    }
}

// Flagged: a module whose function body declares a module from its own file.
#[cfg_attr(
    dylint_lib = "trellis_lints",
    expect(file_named_after_item, reason = "the module gathers gear totals, not only Gear")
)]
#[path = "modules/totals.rs"]
mod totals;

// Flagged: a function, a method and a `let` statement whose bodies hold a module with a child
// module.
#[cfg_attr(
    dylint_lib = "trellis_lints",
    expect(non_leaf_module_items, reason = "registers each child module's routes in one place")
)]
pub fn routes() -> u32 {
    mod table {
        pub(super) mod deeper {}

        pub(super) const fn count() -> u32 {
            1
        }
    }
    table::count()
}

impl Meter {
    #[cfg_attr(
        dylint_lib = "trellis_lints",
        expect(non_leaf_module_items, reason = "the reset joins the child modules' units")
    )]
    fn reset(&mut self) {
        mod zero {
            pub(super) mod deeper {}

            pub(super) const fn value() -> u32 {
                0
            }
        }
        self.0 = zero::value();
    }
}

pub fn total() -> u32 {
    #[cfg_attr(
        dylint_lib = "trellis_lints",
        expect(non_leaf_module_items, reason = "the sum joins the child modules' counts")
    )]
    let sum = {
        mod parts {
            pub(super) mod deeper {}

            pub(super) const fn three() -> u32 {
                3
            }
        }
        parts::three()
    };
    sum
}

// Not flagged: a module whose only child module is a `#[cfg(test)]` one that holds no module.
#[cfg_attr(
    dylint_lib = "trellis_lints",
    expect(file_named_after_item, reason = "the module gathers toolbox helpers, not only Toolbox")
)]
#[path = "modules/kit.rs"]
mod kit;

#[cfg_attr(
    dylint_lib = "trellis_lints",
    expect(
        non_leaf_module_items,
        reason = "the sealed trait keeps other crates from implementing the public traits"
    )
)]
mod seal {
    pub trait Sealed {}

    #[cfg(test)]
    mod tests {
        #[test]
        fn seals() {}
    }
}

fn main() {}
