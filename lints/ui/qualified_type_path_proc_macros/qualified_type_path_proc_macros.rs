// edition:2024
// aux-build:convert.rs
// aux-build:defaulted.rs

extern crate convert;
extern crate defaulted;

// Not flagged: `From` through its module, which a derive writes at the span of the type's name,
// where the author can't change it.
#[derive(convert::Convert)]
struct Meters(u32);

// Flagged: the same path written in the source.
impl ::core::convert::From<Meters> for u32 {
    fn from(meters: Meters) -> Self {
        meters.0
    }
}

// Flagged: a path written by hand whose text doesn't end the way its HIR does, since `Fn(u8) -> u8`
// holds its arguments in parentheses. Only the code a derive writes is skipped for that.
fn apply(function: &dyn ::core::ops::Fn(u8) -> u8) -> u8 {
    function(1)
}

// Flagged: a type written with its module in a string a derive parses as a path, as serde does
// for `#[serde(default = "...")]`. The caller wrote the path there.
#[derive(defaulted::Defaulted)]
#[defaulted(path = "std::string::String::new")]
struct Named;

// Flagged: the same through text that differs from the path: a comment, a raw identifier and an
// empty turbofish.
#[derive(defaulted::Defaulted)]
#[defaulted(path = "std::string::/**/String::new")]
struct Commented;

#[derive(defaulted::Defaulted)]
#[defaulted(path = "std::string::r#String::new")]
struct Raw;

#[derive(defaulted::Defaulted)]
#[defaulted(path = "std::string::String::<>::new")]
struct Turbofish;

// Flagged: a type written with its module in a string a derive parses as a path and adds a
// segment of its own to at its own span, as serde adds `deserialize` for
// `#[serde(with = "...")]`, which gives the path the derive's span.
#[derive(defaulted::Defaulted)]
#[defaulted(with = "units::Length")]
struct With;

mod units {
    pub struct Length;

    impl Length {
        pub fn deserialize() {}
    }
}

// Flagged: the same with the type's name written decomposed, as `e` and a combining accent, which
// the compiler reads as the composed name it interns.
#[derive(defaulted::Defaulted)]
#[defaulted(with = "accents::Café")]
struct Decomposed;

mod accents {
    pub struct Café;

    impl Café {
        pub fn deserialize() {}
    }
}

// Not flagged: a path the derive writes at a string's span, which the string doesn't hold.
#[derive(defaulted::Defaulted)]
#[defaulted(own = "a message")]
struct Messaged;

fn main() {
    let _ = (u32::from(Meters::from(1)), apply(&|number| number));
}
