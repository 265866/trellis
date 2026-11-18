// edition:2024

// Flagged: an integer.
type Cents = u64;

// Flagged: a float, through a path.
type Factor = core::primitive::f64;

// Flagged: `bool`.
type Enabled = bool;

// Flagged: `String`.
type StoreName = String;

// Flagged: `&'static str`.
type Label = &'static str;

// Not flagged: an alias with generic parameters names a family of types.
type Name<'a> = &'a str;

// Not flagged: a compound type isn't a primitive.
type Prices = Vec<u64>;

// Not flagged: an alias of an alias may mirror a platform typedef.
type Handle = Cents;

// Not flagged: a `snake_case` name follows the C convention for FFI typedefs.
#[allow(non_camel_case_types)]
type c_flags = u32;

// Not flagged: an associated type in a trait impl is the trait's choice.
struct Order;

impl Iterator for Order {
    type Item = u64;

    fn next(&mut self) -> Option<u64> {
        None
    }
}

fn main() {}

// Not reported: flagged, but an item-level `expect` in the documented form takes it.
#[cfg_attr(
    dylint_lib = "trellis_lints",
    expect(
        primitive_type_alias,
        reason = "the fixture checks an item-level expect"
    )
)]
type Excused = u32;
