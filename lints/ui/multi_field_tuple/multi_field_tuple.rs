// edition:2024

use std::io;
use std::marker::PhantomData;

// Flagged: a tuple struct with two fields.
struct Range(u64, u64);

// Flagged: `PhantomData` doesn't count, but two data fields remain.
struct Tagged<T>(u64, u64, PhantomData<T>);

// Not flagged: a newtype has one field.
struct Cents(u64);

// Not flagged: a newtype with a `PhantomData` marker still has one data field.
struct Id<T>(u64, PhantomData<T>);

// Flagged: a newtype's field holds a tuple.
struct Pair((u64, u64));

// Not flagged: named fields.
struct Span {
    start: u64,
    end: u64,
}

struct Machine {
    // Flagged: a field holds a tuple.
    bounds: (u64, u64),
    // Flagged: a field holds a tuple inside an `Option`.
    last: Option<(u64, bool)>,
    // Flagged: tuples in a collection and in a map's values.
    history: Vec<(u64, u64)>,
    totals: std::collections::HashMap<u64, (usize, usize)>,
    // Not flagged: the unit type, and a one-element tuple.
    marker: (),
    single: (u64,),
}

// Flagged: a tuple variant with two fields, and a named field holding a tuple.
enum Event {
    Sale(u64, u64),
    Refund { range: (u64, u64) },
    // Not flagged: a variant with one field, and one with named fields.
    Hold(u64),
    Discount { orders: u64, factor: u64 },
}

// Flagged: a function returning a tuple of three.
fn order() -> (u64, u64, bool) {
    (1, 2, false)
}

// Flagged: a pair.
fn bounds() -> (u64, u64) {
    (0, 1)
}

// Flagged: a tuple in an `Option`.
fn parse() -> Option<(u64, u64, u64)> {
    None
}

// Flagged: a tuple in a `Result`, and in an alias of `Result`.
fn read() -> Result<(u64, u64), String> {
    Ok((0, 1))
}

fn read_io() -> io::Result<Option<(u64, u64)>> {
    Ok(None)
}

// Flagged: an `async fn` declares its return type too.
async fn fetch() -> (u64, u64) {
    (0, 1)
}

// Flagged: a tuple in a collection, an iterator's items, and a parameter.
fn pairs() -> Vec<(u64, u64)> {
    Vec::new()
}

fn prices() -> impl Iterator<Item = (u64, bool)> {
    std::iter::empty()
}

fn pay(order: (u64, bool)) -> u64 {
    order.0
}

// Flagged: a type alias names a tuple.
type Bounds = (u64, u64);

// Not flagged: unit, a one-element tuple, and no return type.
fn nothing() -> Result<(), String> {
    Ok(())
}

fn single() -> (u64,) {
    (0,)
}

async fn done() {}

struct Shelf;

impl Shelf {
    // Flagged: an inherent method.
    fn stops(&self) -> (u8, u8, u8) {
        (0, 1, 2)
    }

    // Flagged: an inherent `async` method.
    async fn fetch(&self) -> Option<(u8, u8)> {
        None
    }
}

trait Stops {
    // Flagged: a trait declares its own signature.
    fn stops(&self) -> (u8, u8);
}

// Not flagged: a trait impl takes its signature from the trait.
impl Stops for u8 {
    fn stops(&self) -> (u8, u8) {
        (*self, *self)
    }
}

// Not flagged: an `expect` on the item covers it.
#[cfg_attr(
    dylint_lib = "trellis_lints",
    expect(multi_field_tuple, reason = "the fixture checks an item-level expect")
)]
fn excused() -> (u8, u8) {
    (0, 1)
}

struct Request;
struct Session;
struct Store;

// Not flagged: a closure trait's parameter list isn't a tuple, in an alias, a field, a `Box` or
// an `Rc`, a returned `impl Fn` or a `where` clause.
type RequestHandler = Box<dyn Fn(&Request, &mut Session, &mut Store)>;

struct Endpoint {
    on_get: RequestHandler,
    on_head: Box<dyn Fn(&Request, &mut Store)>,
    on_delete: std::rc::Rc<dyn Fn(&Request, &mut Session, &mut Store)>,
    on_close: Box<dyn FnOnce(u64, u64) -> u64>,
}

fn handler() -> impl Fn(&Request, &mut Store) {
    |_, _| {}
}

fn listen<F>(callback: F)
where
    F: Fn(u64, u64, &mut Store),
{
    let _ = callback;
}

fn listen_impl(callback: impl Fn(&Request, &mut Session, &mut Store)) {
    let _ = callback;
}

// Flagged: a closure returning a tuple, and a closure taking one.
struct Picker {
    pick: Box<dyn Fn() -> (u64, u64)>,
    place: Box<dyn Fn((u64, u64), &mut Store)>,
}

fn picker() -> impl FnMut(u64) -> Option<(u64, bool)> {
    |_| None
}

fn main() {
    // Not flagged: closures and bindings are local.
    let triple = || (1, 2, 3);
    let (a, b, c) = triple();
    let _ = (a, b, c);
}

// Flagged: the same code with a raw identifier, whose text `is_from_proc_macro` misreads; only
// the code a derive writes is skipped for that.
fn raw_pair() -> (r#u32, u32) {
    (1, 2)
}
