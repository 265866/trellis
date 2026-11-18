// edition:2024

use std::error::Error;
use std::fmt;

macro_rules! display {
    ($name:ty) => {
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(stringify!($name))
            }
        }
    };
}

// Flagged: an enum error whose fields are all `Copy`.
#[derive(Debug)]
enum OrderError {
    Offline,
    AmountTooLarge { limit: u64 },
}

display!(OrderError);
impl Error for OrderError {}

// Flagged: every field is `Copy`, and `Clone` alone isn't enough.
#[derive(Clone, Debug)]
struct OutOfRange {
    value: i64,
    reason: &'static str,
}

display!(OutOfRange);
impl Error for OutOfRange {}

// Flagged: a lifetime parameter doesn't stop it from being `Copy`.
#[derive(Debug)]
struct Unexpected<'a> {
    input: &'a str,
}

display!(Unexpected<'_>);
impl Error for Unexpected<'_> {}

// Not flagged: it's already `Copy`.
#[derive(Clone, Copy, Debug)]
struct Timeout;

display!(Timeout);
impl Error for Timeout {}

// Not flagged: a `String` field isn't `Copy`.
#[derive(Debug)]
struct ParseError {
    message: String,
}

display!(ParseError);
impl Error for ParseError {}

// Not flagged: a type parameter makes `Copy` depend on the caller.
#[derive(Debug)]
struct Wrapped<E>(E);

impl<E: fmt::Debug> fmt::Display for Wrapped<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

impl<E: fmt::Debug> Error for Wrapped<E> {}

// Not flagged: a type with `Drop` can't be `Copy`.
#[derive(Debug)]
struct Logged;

impl Drop for Logged {
    fn drop(&mut self) {}
}

display!(Logged);
impl Error for Logged {}

// Not flagged: it isn't an error type.
#[derive(Debug)]
struct Price {
    cents: u64,
}

fn main() {}

// Not reported: flagged, but an item-level `expect` in the documented form takes it.
#[cfg_attr(
    dylint_lib = "trellis_lints",
    expect(error_type_copy, reason = "the fixture checks an item-level expect")
)]
#[derive(Debug)]
struct Excused;

display!(Excused);
impl Error for Excused {}
