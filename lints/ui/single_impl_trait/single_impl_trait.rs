use std::fmt::Display;

struct Order {
    price: u64,
}

struct Shelf;

// Flagged: one impl, for a local type, and no bound names it.
trait Scoring {
    fn score(&self) -> u64;
}

impl Scoring for Order {
    fn score(&self) -> u64 {
        self.price * 2
    }
}

// Not flagged: two impls.
trait Named {
    fn name(&self) -> &'static str;
}

impl Named for Order {
    fn name(&self) -> &'static str {
        "order"
    }
}

impl Named for Shelf {
    fn name(&self) -> &'static str {
        "shelf"
    }
}

// Not flagged: a generic bound uses it.
trait Weighted {
    fn weight(&self) -> u32;
}

impl Weighted for Order {
    fn weight(&self) -> u32 {
        1
    }
}

fn total<T: Weighted>(items: &[T]) -> u32 {
    items.iter().map(Weighted::weight).sum()
}

// Not flagged: a `where` clause uses it.
trait Ranked {
    fn rank(&self) -> u8;
}

impl Ranked for Order {
    fn rank(&self) -> u8 {
        1
    }
}

fn best<T>(item: &T) -> u8
where
    T: Ranked,
{
    item.rank()
}

// Not flagged: `dyn` uses it.
trait Draw {
    fn draw(&self);
}

impl Draw for Shelf {
    fn draw(&self) {}
}

fn draw_all(items: &[Box<dyn Draw>]) {
    for item in items {
        item.draw();
    }
}

// Not flagged: `impl Trait` uses it.
trait Animate {
    fn frame(&self) -> u32;
}

impl Animate for Shelf {
    fn frame(&self) -> u32 {
        0
    }
}

fn loader() -> impl Animate {
    Shelf
}

// Not flagged: another trait names it as a supertrait.
trait Base {
    fn id(&self) -> u32;
}

impl Base for Shelf {
    fn id(&self) -> u32 {
        7
    }
}

trait Labeled: Base + Display {}

// Not flagged: its one impl is for a type from another crate, so it extends that type.
trait Doubled {
    fn doubled(&self) -> Self;
}

impl Doubled for u64 {
    fn doubled(&self) -> Self {
        self * 2
    }
}

// Not flagged: a blanket impl covers every type.
trait Describe {
    fn describe(&self) -> String;
}

impl<T: Display> Describe for T {
    fn describe(&self) -> String {
        self.to_string()
    }
}

// Not flagged: other crates can see it, so they may implement it or bound on it.
pub trait Exported {
    fn exported(&self);
}

impl Exported for Shelf {
    fn exported(&self) {}
}

// Not flagged: no impl at all.
trait Unused {}

// Not reported: flagged, but an `#[expect]` on the trait itself takes it.
#[expect(single_impl_trait)]
trait Expected {
    fn expected(&self);
}

impl Expected for Shelf {
    fn expected(&self) {}
}

fn main() {
    let order = Order { price: 3 };
    let _ = (
        order.score(),
        order.name(),
        Shelf.name(),
        total(&[Order { price: 1 }]),
        best(&order),
    );
    draw_all(&[Box::new(Shelf)]);
    let _ = (loader().frame(), Shelf.id(), 2_u64.doubled(), 1.describe());
    Shelf.exported();
}

// Not reported: flagged, but an item-level `expect` in the documented form takes it.
#[cfg_attr(
    dylint_lib = "trellis_lints",
    expect(single_impl_trait, reason = "the fixture checks an item-level expect")
)]
trait Excused {
    fn excused(&self);
}

impl Excused for Shelf {
    fn excused(&self) {}
}
