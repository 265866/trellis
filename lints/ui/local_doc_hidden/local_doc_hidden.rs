// Flagged: an item of the crate hidden from the docs and `missing_docs`.
#[doc(hidden)]
pub fn price_table() -> &'static [u32] {
    &[1, 2, 5]
}

// Flagged: `hidden` beside other `doc` settings, and on a field.
#[doc(hidden, alias = "Amount")]
pub struct Cart {
    #[doc(hidden)]
    pub quantity: u32,
}

pub mod internals {
    // Flagged: an inner `doc(hidden)` hides the whole module.
    #![doc(hidden)]

    pub fn helper() {}
}

// Flagged: a `doc(hidden)` that a macro the crate defines writes.
macro_rules! hidden_constant {
    ($name:ident) => {
        #[doc(hidden)]
        pub const $name: u32 = 1;
    };
}

hidden_constant!(HIDDEN);

// Not flagged: an item a macro needs public, with the documented exception.
#[cfg_attr(
    dylint_lib = "trellis_lints",
    expect(local_doc_hidden, reason = "`order!` expands to a call to it")
)]
#[doc(hidden)]
pub fn order_unchecked() {}

// Not flagged: other `doc` attributes, and the hidden items `#[test]` and derives generate.
#[doc = "The shelves."]
#[doc(alias = "Racks")]
#[derive(Debug, Clone, PartialEq)]
pub struct Shelves;

#[test]
fn orders() {}

fn main() {
    let _ = (price_table(), Cart { quantity: 1 }, HIDDEN, Shelves);
    internals::helper();
    order_unchecked();
}
