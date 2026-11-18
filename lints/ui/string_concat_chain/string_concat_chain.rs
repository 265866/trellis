fn main() {
    let name = String::from("shelf");
    let score = 7;

    // Flagged: three strings joined with two `+`.
    let line = name.clone() + ": " + &score.to_string();

    // Flagged: one warning for the whole four-string chain.
    let longer = String::new() + "a" + "b" + "c";

    // Not flagged: two strings is a single append.
    let pair = name.clone() + "!";

    // Not flagged: the chain adds numbers, not strings.
    let sum = 1 + 2 + 3;

    // Not flagged: two `+=` in a row append in place.
    let mut built = String::new();
    built += "a";
    built += "b";

    // Flagged: three `+=` in a row onto one string.
    let mut report = String::new();
    report += "orders: ";
    report += &score.to_string();
    report += "!";

    // Not flagged: other statements break up the appends, and appends to different strings.
    let mut log = String::new();
    log += "a";
    let marker = 1;
    log += "b";
    built += "c";
    log += "d";

    // Not flagged: the extra `+` is inside the right operand, so each `+` joins two strings.
    let nested = name.clone() + &(String::from("x") + "y");

    // Not flagged: `concat!` joins literals at compile time.
    let literal = String::from(concat!("a", "b", "c"));
}

// Not reported: flagged, but an item-level `expect` in the documented form takes it.
#[cfg_attr(
    dylint_lib = "trellis_lints",
    expect(
        string_concat_chain,
        reason = "the fixture checks an item-level expect"
    )
)]
fn excused(name: String, score: u32) -> String {
    name + ": " + &score.to_string()
}

// Flagged: the same code with a raw identifier, whose text `is_from_proc_macro` misreads; only
// the code a derive writes is skipped for that.
fn raw_concat(text: String) -> String {
    r#text + "a" + "b" + "c"
}
