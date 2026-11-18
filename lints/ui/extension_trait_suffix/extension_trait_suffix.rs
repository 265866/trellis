// edition:2024

use std::fmt::Display;

struct Order {
    price: u64,
}

// Flagged: its only impl adds methods to a slice of a primitive.
trait Prices {
    fn total_price(&self) -> u64;
}

impl Prices for [u64] {
    fn total_price(&self) -> u64 {
        self.iter().sum()
    }
}

// Flagged: every impl is for a standard library type.
trait Shout {
    fn shout(&self) -> String;
}

impl Shout for String {
    fn shout(&self) -> String {
        self.to_uppercase()
    }
}

impl Shout for &str {
    fn shout(&self) -> String {
        self.to_uppercase()
    }
}

// Flagged: a blanket impl over a foreign trait extends every type that has it.
trait Describe {
    fn describe(&self) -> String;
}

impl<T: Display + ?Sized> Describe for T {
    fn describe(&self) -> String {
        format!("<{self}>")
    }
}

// Not flagged: the name already ends in `Ext`.
trait PricesExt {
    fn largest_price(&self) -> u64;
}

impl PricesExt for Vec<u64> {
    fn largest_price(&self) -> u64 {
        self.iter().copied().max().unwrap_or(0)
    }
}

// Not flagged: a local type implements it too.
trait Total {
    fn total(&self) -> u64;
}

impl Total for u64 {
    fn total(&self) -> u64 {
        *self
    }
}

impl Total for Order {
    fn total(&self) -> u64 {
        self.price
    }
}

// Not flagged: an impl for a standard library container of a local type extends the local type.
trait Totals {
    fn totals(&self) -> u64;
}

impl Totals for Vec<Order> {
    fn totals(&self) -> u64 {
        self.iter().map(|order| order.price).sum()
    }
}

// Not flagged: a blanket impl over a local trait extends local types.
trait Doubled {
    fn doubled(&self) -> u64;
}

impl<T: Total> Doubled for T {
    fn doubled(&self) -> u64 {
        self.total() * 2
    }
}

// Not flagged: an input trait used as a bound, like the guide's `DateLike`.
trait Amount {
    fn cents(&self) -> u64;
}

impl Amount for u32 {
    fn cents(&self) -> u64 {
        u64::from(*self)
    }
}

fn sum_cents(amounts: &[impl Amount]) -> u64 {
    amounts.iter().map(Amount::cents).sum()
}

// Not flagged: a trait used as a trait object.
trait Label {
    fn label(&self) -> String;
}

impl Label for bool {
    fn label(&self) -> String {
        self.to_string()
    }
}

fn labels() -> Vec<Box<dyn Label>> {
    vec![Box::new(true)]
}

// Not flagged: a marker trait has no methods to add.
trait Sealed {}

impl Sealed for u8 {}

// Not flagged: nothing implements it.
trait Unused {
    fn unused(&self);
}

fn main() {
    let _ = sum_cents(&[1_u32]);
    let _ = labels();
}

// Not reported: flagged, but an item-level `expect` in the documented form takes it.
#[cfg_attr(
    dylint_lib = "trellis_lints",
    expect(
        extension_trait_suffix,
        reason = "the fixture checks an item-level expect"
    )
)]
trait Halved {
    fn halved(&self) -> u64;
}

impl Halved for u64 {
    fn halved(&self) -> u64 {
        self / 2
    }
}
