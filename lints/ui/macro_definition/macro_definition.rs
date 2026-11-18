// edition:2024
// compile-flags: --crate-type proc-macro
// A proc-macro crate, so the fixture can define procedural macros next to `macro_rules!` ones.
// `ui_listed_crate` runs this file again with `macro_crates` listing it, where nothing is flagged.
#![feature(decl_macro)]

extern crate proc_macro;

use proc_macro::TokenStream;

// Flagged: a `macro_rules!` definition.
macro_rules! double {
    ($value:expr) => {
        $value * 2
    };
}

// Flagged: an unstable `macro` definition.
macro triple($value:expr) {
    $value * 3
}

// Flagged: a function-like procedural macro.
#[proc_macro]
pub fn shout(input: TokenStream) -> TokenStream {
    input
}

// Flagged: a derive macro.
#[proc_macro_derive(Describe)]
pub fn describe(input: TokenStream) -> TokenStream {
    let _ = input;
    TokenStream::new()
}

// Flagged: an attribute macro.
#[proc_macro_attribute]
pub fn traced(attribute: TokenStream, item: TokenStream) -> TokenStream {
    let _ = attribute;
    item
}

// Flagged: a macro whatever its reason; `suppression_policy` rejects an `expect` of this lint.
macro_rules! sizes {
    ($($kind:ty),*) => {
        [$(size_of::<$kind>()),*]
    };
}

// Not flagged: an ordinary function that takes a token stream.
fn passthrough(input: TokenStream) -> TokenStream {
    input
}

// Flagged: this definition; the `quadruple` it defines is not, since it comes from an expansion.
macro_rules! define_quadruple {
    () => {
        macro_rules! quadruple {
            ($value: expr) => {
                $value * 4
            };
        }
    };
}

// Not flagged: invoking a macro.
define_quadruple!();

fn main() {
    // Flagged: a `macro_rules!` inside a function is still a macro.
    macro_rules! halve {
        ($value:expr) => {
            $value / 2
        };
    }

    // Not flagged: using a macro.
    let _ = (
        double!(1),
        triple!(1),
        quadruple!(1),
        halve!(4),
        sizes![u8, u16],
        vec![1],
    );
}
