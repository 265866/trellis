// aux-build:parsed.rs
// edition:2024

extern crate parsed;

// Flagged: a `cfg` attribute in a string the derive parses as code, which leaves the function out
// of the lint run while the source holds only a string.
#[derive(parsed::Parsed)]
#[parsed(ty = "[(); { #[cfg(not(any(clippy, dylint_lib = \"trellis_lints\")))] fn hidden() {} 0 }]")]
struct Hidden;

// Flagged: the same with `cfg_attr`, with space between the tokens.
#[derive(parsed::Parsed)]
#[parsed(ty = "[(); { # [ cfg_attr(clippy, allow(clippy::all))] fn hidden() {} 0 }]")]
struct Guarded;

// Flagged: a `cfg_select!` in such a string, which leaves the function out of the lint run too.
#[derive(parsed::Parsed)]
#[parsed(ty = "[(); { cfg_select! { clippy => {} _ => { fn hidden() {} } } 0 }]")]
struct Selected;

// Flagged: a `cfg_attr` in a string the derive calls a function from, with a `{..}` that spells
// the function's name in a comment.
#[derive(parsed::Parsed)]
#[parsed(call = "/*{called}*/ #[cfg_attr(clippy, allow(clippy::all))] called")]
struct Called;

fn called(_: ()) {}

// Flagged: a `cfg!` in such a string, which a lint run reads as text.
#[derive(parsed::Parsed)]
#[parsed(ty = "[(); { if cfg!(test) { 1 } else { 1 } }]")]
struct Tested;

// Flagged: a `cfg!` with braces.
#[derive(parsed::Parsed)]
#[parsed(ty = "[(); { if cfg! { test } { 1 } else { 1 } }]")]
struct Braced;

// Not flagged: a type with no attribute in it.
#[derive(parsed::Parsed)]
#[parsed(ty = "[u8; 2]")]
struct Plain;

// Not flagged: a variable named `cfg` compared with `!=`, which is no `cfg!`.
#[derive(parsed::Parsed)]
#[parsed(ty = "[(); { let cfg = 2; (cfg != 1) as usize }]")]
struct Compared;

// Not flagged: a string the derive copies as it is that holds only a `cfg!`.
#[derive(parsed::Parsed)]
#[parsed(text = "cfg!(test) only")]
struct Macro;

// Not flagged: a string the derive copies as it is, as an expression and a pattern, which is only
// text, that mentions `#[cfg(test)]` and `cfg!(test)`.
#[derive(parsed::Parsed)]
#[parsed(text = "#[cfg(test)] and cfg!(test) only")]
struct Text;

/// Not flagged: a message that mentions `#[cfg(test)]` and `cfg!(test)` and reads a variable in a
/// placeholder, which `format!` reads at a span inside the string.
fn message(name: &str, width: usize, precision: usize) -> String {
    format!("{name:>width$} needs #[cfg(test)], not cfg!(test), {name:.precision$?} {:0width$}", 1)
}

/// Not flagged: a string no macro reads, and a doc comment, that mention `#[cfg(test)]`.
const NOTE: &str = "#[cfg(test)]";

fn main() {
    let _ = message("parser", 8, 2);
}
