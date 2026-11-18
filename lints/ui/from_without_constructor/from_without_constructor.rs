use std::fmt;

struct Funds {
    cents: u64,
}

// Flagged: no inherent constructor takes a `u64`.
impl From<u64> for Funds {
    fn from(cents: u64) -> Self {
        Self { cents }
    }
}

struct Name(String);

impl Name {
    fn new(name: &str) -> Self {
        Self(name.to_owned())
    }

    // A constructor named for the conversion, but from a different type.
    fn from_chars(chars: Vec<char>) -> Self {
        Self(chars.into_iter().collect())
    }
}

// Not flagged: `Name::new` takes the same `&str`, whatever its lifetime.
impl From<&str> for Name {
    fn from(name: &str) -> Self {
        Self::new(name)
    }
}

// Flagged: `Name::from_chars` takes a `Vec<char>`, not a `String`.
impl From<String> for Name {
    fn from(name: String) -> Self {
        Self(name)
    }
}

struct Seconds(u64);

impl Seconds {
    // A method with `self` and a `u64` isn't a constructor.
    fn new_plus(self, more: u64) -> Self {
        Self(self.0 + more)
    }

    fn from_minutes(minutes: u32) -> Self {
        Self(u64::from(minutes) * 60)
    }
}

// Not flagged: `Seconds::from_minutes` takes the `u32`.
impl From<u32> for Seconds {
    fn from(minutes: u32) -> Self {
        Self::from_minutes(minutes)
    }
}

// Flagged: `new_plus` takes `self` as well as the `u64`.
impl From<u64> for Seconds {
    fn from(seconds: u64) -> Self {
        Self(seconds)
    }
}

struct Stack<T>(Vec<T>);

impl<T> Stack<T> {
    fn new(items: Vec<T>) -> Self {
        Self(items)
    }
}

// Not flagged: `Stack::new` takes the `Vec<T>` under its own generic parameter.
impl<U> From<Vec<U>> for Stack<U> {
    fn from(items: Vec<U>) -> Self {
        Self::new(items)
    }
}

#[derive(Debug)]
struct LoadError(std::io::Error);

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "load failed: {}", self.0)
    }
}

impl std::error::Error for LoadError {}

// Not flagged: an error type's `From` impl exists for `?`.
impl From<std::io::Error> for LoadError {
    fn from(error: std::io::Error) -> Self {
        Self(error)
    }
}

struct Wrapper;

// Not flagged: `Option<u8>` belongs to another crate.
impl From<Wrapper> for Option<u8> {
    fn from(_: Wrapper) -> Self {
        None
    }
}

macro_rules! from_unit {
    ($ty:ident) => {
        struct $ty;

        impl From<()> for $ty {
            fn from((): ()) -> Self {
                Self
            }
        }
    };
}

// Not flagged: the impl comes from a macro.
from_unit!(Unit);

fn main() {
    let _ = (
        Funds::from(1).cents,
        Name::from("a").0,
        Name::from(String::new()).0,
    );
    let _ = (
        Seconds::from(1_u32).new_plus(1).0,
        Seconds::from(1_u64).0,
        Name::from_chars(vec![]).0,
    );
    let _ = (
        Stack::from(vec![1]).0,
        LoadError::from(std::io::Error::other("x")).0,
        Option::<u8>::from(Wrapper),
        Unit::from(()),
    );
}

// Not reported: flagged, but an item-level `expect` in the documented form takes it.
struct Excused {
    cents: u32,
}

#[cfg_attr(
    dylint_lib = "trellis_lints",
    expect(
        from_without_constructor,
        reason = "the fixture checks an item-level expect"
    )
)]
impl From<u32> for Excused {
    fn from(cents: u32) -> Self {
        Self { cents }
    }
}
