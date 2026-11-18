// edition:2024

use std::mem;
use std::mem::{replace as put, swap};
use std::{cmp::{self, max}, iter};

use math::double;
use shelves::Shelf;

mod math {
    pub fn double(x: u64) -> u64 {
        x * 2
    }
}

mod shelves {
    pub struct Shelf(pub u8);
}

mod ffi {
    unsafe extern "C" {
        pub fn abs(x: i32) -> i32;
    }
}

use ffi::abs;

fn triple(x: u64) -> u64 {
    x * 3
}

macro_rules! swap_twice {
    ($a:expr, $b:expr) => {
        swap($a, $b)
    };
}

fn order() {
    let (mut left, mut right) = (1, 2);

    // Flagged: `swap` was imported by name from `std::mem`.
    swap(&mut left, &mut right);

    // Flagged: a renamed import is still unqualified.
    let _ = put(&mut left, 3);

    // Flagged: a function of this crate from another module.
    let _ = double(4);

    // Flagged: a function imported inside nested braces.
    let _ = max(left, right);

    // Not flagged: the call names its module.
    let _ = mem::take(&mut left);
    let _ = cmp::min(left, right);
    let _ = iter::empty::<u8>();

    // Not flagged: a function of the same module.
    let _ = triple(5);

    // Not flagged: `drop` comes from the prelude.
    drop(String::new());

    // Not flagged: a tuple struct constructor isn't a function.
    let _ = Shelf(1);

    // Not flagged: foreign functions are called directly.
    let _ = unsafe { abs(-1) };

    // Not flagged: an associated function is reached through its type.
    let _ = Vec::<u8>::new();

    // Not flagged: the call comes from a macro expansion.
    swap_twice!(&mut left, &mut right);
}

fn largest() -> u64 {
    // Flagged: an import inside the function body is still an import.
    use std::cmp::max;

    max(1, 2)
}

mod tests {
    use super::*;

    fn doubles() {
        // Not flagged: a glob import, such as a test module's `use super::*`.
        let _ = double(2);
    }
}

fn main() {}

// Not reported: flagged, but an item-level `expect` in the documented form takes it.
#[cfg_attr(
    dylint_lib = "trellis_lints",
    expect(
        qualified_function_calls,
        reason = "the fixture checks an item-level expect"
    )
)]
fn excused() {
    let (mut left, mut right) = (1, 2);
    swap(&mut left, &mut right);
}

// Flagged: the same code with a raw identifier, whose text `is_from_proc_macro` misreads; only
// the code a derive writes is skipped for that.
fn raw_swap(first: &mut u8, second: &mut u8) {
    r#swap(first, second);
}
