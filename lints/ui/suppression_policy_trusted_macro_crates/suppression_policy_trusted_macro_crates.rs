// aux-build:quiet.rs
// The test's `dylint.toml` sets `trusted_macro_crates = ["alloc", "core", "std", "quiet"]` and
// lets `quiet` set the level of `clippy::unwrap_used` over code written in the source.

extern crate quiet;

// Flagged: an `allow` of a lint group that a trusted macro writes over code written in the source.
quiet::silence! {
    fn first(values: &[u32]) -> u32 {
        *values.first().unwrap()
    }
}

// Not flagged: an `expect` with a reason that a macro from another crate writes, and the
// standard library's macros.
quiet::documented_exception! {
    fn second(values: &[u32]) -> u32 {
        values[1]
    }
}

// Not flagged: an `expect` whose reason is too short that a macro from a trusted crate writes, of a
// lint the configuration lets it set.
quiet::terse_exception! {
    fn third(values: &[u32]) -> u32 {
        *values.first().unwrap()
    }
}

// Flagged: a clippy attribute that a trusted macro writes over code written in the source.
quiet::old_rust! {
    fn fourth() {}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
struct Derived(u32);

thread_local! {
    static COUPONS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

fn main() {
    let _ = (first(&[1]), second(&[1, 2]), Derived(1), COUPONS.with(std::cell::Cell::get));
    let _ = (third(&[1]), fourth());
    let _ = (vec![1, 2], format!("{}", 1));
}
