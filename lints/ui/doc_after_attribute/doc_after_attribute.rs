//! Doc comments before and after attributes.

// Flagged: the doc comment comes after the attribute.
#[derive(Debug)]
/// An order of the shelves.
struct Order;

// Flagged once for a doc comment of several lines, after an attribute that a doc line precedes
// too.
/// An amount.
#[derive(Debug, Clone, Copy)]
/// It settles in cents.
/// Its quantity never changes.
struct Amount {
    // Flagged: a field's doc comment after the field's attribute.
    #[allow(dead_code)]
    /// The quantity, in cents.
    quantity: u64,
}

// Not flagged: doc comments come first, and inner docs may follow an inner attribute.
/// A shelf.
#[derive(Debug)]
struct Shelf;

mod shelves {
    #![allow(dead_code)]
    //! The shelves.

    /// A symbol.
    #[derive(Debug)]

    struct Symbol;
}

/** Flagged: a block doc comment after an attribute counts too. */
#[derive(Debug)]
/** A price. */
struct Price;

fn main() {
    let _ = (Order, Amount { quantity: 1 }, Shelf, Price);
}
