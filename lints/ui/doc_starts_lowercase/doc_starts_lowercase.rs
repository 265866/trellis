//! the crate docs. Flagged: the crate's inner docs start lowercase.

/// returns one. Flagged: an outer doc comment starts lowercase.
fn one() -> u32 {
    1
}

#[doc = "returns two. Flagged: a `doc` attribute starts lowercase."]
fn two() -> u32 {
    2
}

/** returns three. Flagged: a block doc comment starts lowercase. */
fn three() -> u32 {
    3
}

///
/// returns four. Flagged: blank doc lines before the first sentence don't count.
fn four() -> u32 {
    4
}

/// Not flagged: later lines may start lowercase,
/// as this one does.
fn five() -> u32 {
    5
}

/// `six` is not flagged: the doc starts with code.
fn six() -> u32 {
    6
}

/// [`seven`] is not flagged: the doc starts with a link.
fn seven() -> u32 {
    7
}

/// 8 is not flagged: the doc starts with a digit.
fn eight() -> u32 {
    8
}

/// iOS is not flagged: a first word with a capital inside is a name.
fn nine() -> u32 {
    9
}

/// trellis's shelf count. Not flagged: `trellis` is in the workspace's `lowercase_doc_words`.
fn twelve() -> u32 {
    12
}

/// e.g. a value. Not flagged: so are `e.g.` and `i.e.`.
fn thirteen() -> u32 {
    13
}

/// i.e., a value. Not flagged: punctuation may follow a listed word.
fn fourteen() -> u32 {
    14
}

/// trellises is flagged: a listed word followed by more letters is another word.
fn fifteen() -> u32 {
    15
}

/// returns sixteen. Not flagged: an exception on the item, with a reason.
#[cfg_attr(
    dylint_lib = "trellis_lints",
    expect(doc_starts_lowercase, reason = "the text mirrors a lowercase upstream doc")
)]
fn sixteen() -> u32 {
    16
}

mod shelves {
    //! the shelves. Flagged: a module's inner docs start lowercase.

    /// Not flagged: a capitalized field doc.
    pub struct Shelf {
        /// the stop. Flagged: a field doc starts lowercase.
        pub stop: u32,
    }
}

fn nested() -> u32 {
    /// returns ten. Flagged: an item inside a function body.
    fn ten() -> u32 {
        10
    }

    ten()
}

macro_rules! documented {
    ($name:ident) => {
        /// returns eleven, from a macro.
        fn $name() -> u32 {
            11
        }
    };
}

// Not flagged: the doc comment comes from a macro expansion.
documented!(eleven);

fn main() {
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
        nested(),
    );
    let _ = (eleven(), shelves::Shelf { stop: 0 });
    let _ = (twelve(), thirteen(), fourteen(), fifteen(), sixteen());
}
