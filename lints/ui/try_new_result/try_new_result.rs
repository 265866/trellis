// edition:2024

use std::io;

#[derive(Debug)]
struct ParseError;

type Result<T> = std::result::Result<T, ParseError>;

struct Config {
    depth: u32,
}

impl Config {
    // Flagged: a constructor that returns a `Result` is `try_new`.
    fn new(depth: u32) -> std::result::Result<Self, ParseError> {
        if depth == 0 {
            Err(ParseError)
        } else {
            Ok(Self { depth })
        }
    }

    // Flagged: `new_foo` that returns a `Result` through a local alias is `try_new_foo`.
    fn new_from_text(text: &str) -> Result<Self> {
        text.parse()
            .map(|depth| Self { depth })
            .map_err(|_| ParseError)
    }

    // Flagged: `io::Result` is a `Result` too.
    fn new_from_reader(_reader: &mut dyn io::Read) -> io::Result<Self> {
        Ok(Self { depth: 1 })
    }

    // Flagged: `try_new` must return a `Result`.
    fn try_new() -> Self {
        Self { depth: 1 }
    }

    // Not flagged: a constructor that can fail without a reason may return an `Option`.
    fn try_new_checked(depth: u32) -> Option<Self> {
        (depth > 0).then_some(Self { depth })
    }

    // Not flagged: `try_new` returns a `Result`.
    fn try_new_shallow() -> Result<Self> {
        Ok(Self { depth: 1 })
    }

    // Not flagged: `new` doesn't return a `Result`.
    fn new_deep() -> Self {
        Self { depth: 9 }
    }

    // Not flagged: `newest` only shares a prefix with `new`.
    fn newest() -> Result<Self> {
        Ok(Self { depth: 0 })
    }

    // Not flagged: a method that takes `self` isn't a constructor.
    fn new_child(&self) -> Result<Self> {
        Ok(Self {
            depth: self.depth + 1,
        })
    }

    // Not flagged: an `async fn` returns a future, which the lint doesn't look through.
    async fn new_async() -> Result<Self> {
        Ok(Self { depth: 1 })
    }
}

// Flagged: a trait declares the names its impls use.
trait Build: Sized {
    fn new(depth: u32) -> std::result::Result<Self, ParseError>;
}

// Not flagged: a trait impl takes its names from the trait.
impl Build for u32 {
    fn new(depth: u32) -> std::result::Result<Self, ParseError> {
        Ok(depth)
    }
}

fn main() {}

// Not reported: flagged, but an item-level `expect` in the documented form takes it.
impl Config {
    #[cfg_attr(
        dylint_lib = "trellis_lints",
        expect(try_new_result, reason = "the fixture checks an item-level expect")
    )]
    fn new_checked(depth: u32) -> Result<Self> {
        if depth == 0 {
            Err(ParseError)
        } else {
            Ok(Self { depth })
        }
    }
}
