// edition:2024

use std::error::Error;
use std::fmt;

// Flagged: a struct error without `#[must_use]`.
#[derive(Debug)]
struct AmountTooLarge;

impl fmt::Display for AmountTooLarge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("amount too large")
    }
}

impl Error for AmountTooLarge {}

// Flagged: an enum error without `#[must_use]`.
#[derive(Debug)]
enum OrderError {
    Offline,
}

impl fmt::Display for OrderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("offline")
    }
}

impl Error for OrderError {}

// Flagged: a generic error whose impl has bounds.
#[derive(Debug)]
struct Wrapped<E>(E);

impl<E: fmt::Debug> fmt::Display for Wrapped<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

impl<E: fmt::Debug> Error for Wrapped<E> {}

// Not flagged: it's already `#[must_use]`.
#[derive(Debug)]
#[must_use]
struct Timeout;

impl fmt::Display for Timeout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("timed out")
    }
}

impl Error for Timeout {}

// Not flagged: `#[must_use]` with a reason counts too.
#[derive(Debug)]
#[must_use = "a lost connection needs handling"]
struct Disconnected;

impl fmt::Display for Disconnected {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("disconnected")
    }
}

impl Error for Disconnected {}

// Not flagged: it implements `Display` but not `Error`.
struct Price;

impl fmt::Display for Price {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("price")
    }
}

macro_rules! error_type {
    ($name:ident) => {
        #[derive(Debug)]
        struct $name;

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(stringify!($name))
            }
        }

        impl Error for $name {}
    };
}

// Flagged: a macro writes the `Error` impl, but the type is written here.
#[derive(Debug)]
struct Rejected;

impl fmt::Display for Rejected {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("rejected")
    }
}

macro_rules! impl_error {
    ($name:ident) => {
        impl Error for $name {}
    };
}

impl_error!(Rejected);

// Not flagged: a macro defines the type itself.
error_type!(Generated);

fn main() {}

// Not reported: flagged, but an item-level `expect` in the documented form takes it.
#[cfg_attr(
    dylint_lib = "trellis_lints",
    expect(
        error_type_must_use,
        reason = "the fixture checks an item-level expect"
    )
)]
#[derive(Debug)]
struct Excused;

impl fmt::Display for Excused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("excused")
    }
}

impl Error for Excused {}
