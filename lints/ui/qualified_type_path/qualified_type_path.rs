// edition:2024

use std::fmt::Write;
use std::{collections, fmt, io};

use parts::*;

mod machine {
    pub struct Config;
    pub struct Gauge;
    pub struct Lever;
}

mod settings {
    pub struct Config;
}

mod parts {
    pub struct Gauge;
}

mod custom {
    pub struct Option;
}

mod shelves {
    pub struct Shelf;

    pub enum Symbol {
        Cherry,
    }
}

struct Order;

// Not flagged: a type of the same name is defined here.
struct Lever;

// Flagged: a trait and a type named through `fmt`; `fmt::Result` is on the ambiguous list.
impl fmt::Display for Order {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Ok(())
    }
}

// Flagged: a type in a signature, and again as the receiver of an associated function.
fn table() -> collections::HashMap<u8, u8> {
    collections::HashMap::new()
}

// Flagged: a type of this crate named through its module.
fn first_shelf() -> shelves::Shelf {
    shelves::Shelf
}

// Not flagged: `io::Error` is on the default ambiguous list.
fn fail() -> io::Error {
    io::Error::other("fail")
}

// Not flagged: two types named `Config` are both qualified here.
fn configs(_: machine::Config, _: settings::Config) {}

// Not flagged: `Lever` is also defined in this module.
fn lever(_: machine::Lever) {}

// Not flagged: `Write` is imported from `fmt`, so `io::Write` disambiguates.
fn writer(_: &mut dyn io::Write) {}

// Not flagged: the glob import brings in another `Gauge`.
fn gauge(_: machine::Gauge) {}

// Not flagged: importing it would shadow the prelude's `Option`.
fn option(_: custom::Option) {}

// Not flagged: enum variants, `Self` and associated types are reached through a type.
fn symbol<T: Iterator>(iter: T) -> Option<T::Item> {
    let _ = shelves::Symbol::Cherry;
    let _: Option<<T as Iterator>::Item> = None;
    None
}

macro_rules! qualified_shelf {
    () => {
        shelves::Shelf
    };
}

// Not flagged: macro output.
fn generated() {
    let _: qualified_shelf!() = shelves::Shelf;
}

// Flagged: a prelude type named through its module only needs its bare name.
fn owned() -> std::string::String {
    std::string::String::new()
}

// Not flagged: an `expect` on the function covers its signature and body.
#[cfg_attr(
    dylint_lib = "trellis_lints",
    expect(qualified_type_path, reason = "the fixture checks an item-level expect")
)]
fn excused() -> shelves::Shelf {
    shelves::Shelf
}

fn statement() {
    // Not flagged: an `expect` on a statement covers it.
    #[cfg_attr(
        dylint_lib = "trellis_lints",
        expect(qualified_type_path, reason = "the fixture checks a statement-level expect")
    )]
    let _ = collections::HashSet::<u8>::new();
}

mod inner {
    // Flagged: a child module is checked on its own, through `super` too.
    fn shelf() -> super::shelves::Shelf {
        super::shelves::Shelf
    }
}

fn main() {}
