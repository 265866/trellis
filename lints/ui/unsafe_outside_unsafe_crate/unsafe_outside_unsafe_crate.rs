// edition:2024
// The crate is named after this file; `ui_listed_crate` reruns it with that name listed.

// Flagged: an `unsafe extern` block.
unsafe extern "C" {
    safe fn abs(value: i32) -> i32;
}

// Flagged: an `unsafe fn`.
unsafe fn first(values: &[u32]) -> u32 {
    // Flagged: an `unsafe` block, even inside an `unsafe fn`.
    unsafe { *values.get_unchecked(0) }
}

// Flagged: an `unsafe trait`.
unsafe trait Zeroable {}

// Flagged: an `unsafe impl`.
unsafe impl Zeroable for u32 {}

struct Raw;

impl Raw {
    // Flagged: an `unsafe` method.
    unsafe fn read(&self) -> u32 {
        0
    }
}

// Flagged: an unsafe attribute.
#[unsafe(no_mangle)]
extern "C" fn exported() {}

// Not flagged: a safe function that calls only safe code.
fn sum(values: &[u32]) -> u32 {
    values.iter().sum()
}

// Not flagged: an exception on the item, with a reason, as the style guide writes it.
#[cfg_attr(
    dylint_lib = "trellis_lints",
    expect(
        unsafe_outside_unsafe_crate,
        reason = "the bounds check is in the caller"
    )
)]
fn second(values: &[u32]) -> u32 {
    unsafe { *values.get_unchecked(1) }
}

// Not flagged: `derive(Clone, Copy)` writes an `unsafe impl` of its own.
#[derive(Clone, Copy)]
struct Coupon(u32);

macro_rules! read_raw {
    ($raw:expr) => {
        unsafe { $raw.read() }
    };
}

fn main() {
    let values = [1, 2];
    // Flagged: calling an `unsafe fn` needs an `unsafe` block.
    let total = unsafe { first(&values) } + second(&values) + sum(&values);
    // Not flagged: the `unsafe` block comes from a macro expansion.
    let _ = (total, abs(-1), read_raw!(Raw), Coupon(1));
}
