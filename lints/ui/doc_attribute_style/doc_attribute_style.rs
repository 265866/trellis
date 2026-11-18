#![doc = include_str!("readme.md")]
// Flagged above: crate docs included from a file, README or not.

/** Returns one. Flagged: an outer block doc comment. */
fn one() -> u32 {
    1
}

mod shelves {
    /*! The shelves. Flagged: an inner block doc comment. */

    //! Not flagged: inner line docs.
}

#[doc = "Returns two. Flagged: a `doc` attribute holding a literal."]
fn two() -> u32 {
    2
}

#[doc = concat!("Returns ", "three. Flagged: a macro generates the text.")]
fn three() -> u32 {
    3
}

const FOUR: &str = "Returns four.";

#[doc = include_str!("readme.md")]
fn four() -> &'static str {
    FOUR
}

/// Not flagged: `///` lines,
/// however many.
fn five() -> u32 {
    5
}

/// Not flagged: `doc(hidden)` and `doc(alias)` aren't documentation text.
#[doc(hidden)]
#[doc(alias = "half_dozen")]
fn six() -> u32 {
    6
}

/* Not flagged: an ordinary block comment, which `code_comment` owns. */
fn seven() -> u32 {
    7
}

macro_rules! documented {
    ($name:ident, $text:expr) => {
        #[doc = $text]
        fn $name() -> u32 {
            8
        }
    };
}

// Not flagged: a macro expansion writes the `doc` attribute.
documented!(eight, "Returns eight.");

/// Returns nine. Not flagged: the `expect` on the item covers its `doc` attribute, and is
/// fulfilled.
#[cfg_attr(
    dylint_lib = "trellis_lints",
    expect(
        doc_attribute_style,
        reason = "the price table doc is shared with the vendor's spec"
    )
)]
#[doc = concat!("Pays ", "nine.")]
fn nine() -> u32 {
    9
}

fn main() {
    let _ = more();
    let _ = (
        one(),
        two(),
        three(),
        four(),
        five(),
        six(),
        seven(),
        eight(),
        nine(),
    );
}

macro_rules! passed {
    ($item:item) => {
        $item
    };
}

passed! {
    /// Returns ten. Not flagged: `///` lines in a macro's input, which reach the compiler as
    /// `doc` attributes.
    fn ten() -> u32 {
        10
    }
}

passed! {
    /** Returns eleven. Flagged: a block doc comment in a macro's input. */
    fn eleven() -> u32 {
        11
    }
}

fn more() -> u32 {
    ten() + eleven()
}
