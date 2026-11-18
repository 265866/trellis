const TEMPLATE: &str = "Order {n}";

struct Template;

impl Template {
    fn replace(&self, _: &str, _: &str) -> String {
        String::new()
    }
}

fn main() {
    let amount = 250;

    // Flagged: a literal used as a template for `replace`.
    let banner = "You won {amount}!".replace("{amount}", &amount.to_string());

    // Flagged: `replacen` too, and raw string literals.
    let first = r"{x} and {x}".replacen("{x}", "1", 1);

    // Not flagged: the receiver is a runtime string.
    let input = String::from("a-b");
    let dashes = input.replace('-', "_");

    // Flagged: a `const` template, and `String`s made from a literal.
    let order = TEMPLATE.replace("{n}", "3");
    let owned = String::from("Order {n}").replace("{n}", "4");
    let converted = "Order {n}".to_owned().replace("{n}", "5");

    // Not flagged: a `const` document edited in place, whose replaced text is no placeholder.
    let edited = TEMPLATE.replace("Order", "Receipt");

    // Not flagged: a `String` built from a variable.
    let built = String::from(input.as_str()).replace('-', "+");

    // Not flagged: a non-`str` method named `replace`.
    let other = Template.replace("a", "b");

    // Not flagged: the literal comes from a macro.
    let from_macro = concat!("{a}", "!").replace("{a}", "hi");
}

// Not reported: flagged, but an item-level `expect` in the documented form takes it.
#[cfg_attr(
    dylint_lib = "trellis_lints",
    expect(
        str_replace_on_literal,
        reason = "the fixture checks an item-level expect"
    )
)]
fn excused(amount: u32) -> String {
    "You won {amount}!".replace("{amount}", &amount.to_string())
}
