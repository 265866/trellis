fn parse(text: &str) -> Result<u32, std::num::ParseIntError> {
    text.parse()
}

struct Shelves(Vec<u32>);

impl Shelves {
    fn map(&self, f: impl Fn(u32) -> u32) -> Vec<u32> {
        self.0.iter().copied().map(f).collect()
    }
}

fn main() {
    let texts = vec!["1", "x", "3"];
    let values = vec![1_u32, 2, 3];

    // Flagged: the guide's example, a `filter_map` and `filter` chain in the header.
    for value in texts
        .iter()
        .filter_map(|text| parse(text).ok())
        .filter(|value| *value > 1)
    {
        let _ = value;
    }

    // Flagged: one closure-taking adapter is still a combinator chain.
    for doubled in values.iter().map(|value| value * 2) {
        let _ = doubled;
    }

    // Flagged: a function path takes the closure's place.
    for value in texts.iter().map(|text| parse(text)).filter_map(Result::ok) {
        let _ = value;
    }

    // Not flagged: `iter`, `enumerate`, `rev`, `zip`, `skip` and `copied` take no closure.
    for (index, value) in values.iter().enumerate().rev() {
        let _ = (index, value);
    }
    for (value, text) in values.iter().copied().zip(texts.iter()).skip(1) {
        let _ = (value, text);
    }

    // Not flagged: the chain is assigned to a named variable first.
    let urgent = texts
        .iter()
        .filter_map(|text| parse(text).ok())
        .filter(|value| *value > 1);
    for value in urgent {
        let _ = value;
    }

    // Not flagged: a non-`Iterator` method named `map` that takes a closure.
    for value in Shelves(values.clone()).map(|value| value * 2) {
        let _ = value;
    }

    // Flagged: five closure-taking combinators in one chain.
    let total: u32 = values
        .iter()
        .map(|value| value + 1)
        .filter(|value| value % 2 == 0)
        .map(|value| value * 3)
        .inspect(|value| assert!(*value > 0))
        .fold(0, |sum, value| sum + value);

    // Flagged once: a long chain in a `for` header gets the header warning only.
    for value in values
        .iter()
        .map(|value| value + 1)
        .filter(|value| value % 2 == 0)
        .map(|value| value * 3)
        .inspect(|value| assert!(*value > 0))
        .skip_while(|value| *value < 5)
    {
        let _ = value;
    }

    // Not flagged: four closure-taking combinators.
    let count = values
        .iter()
        .map(|value| value + 1)
        .filter(|value| value % 2 == 0)
        .map(|value| value * 3)
        .take_while(|value| *value < 100)
        .count();

    // Not flagged: a short chain whose combinator carries the logic, the guide's preferred case.
    let exceeded = values.iter().any(|&value| value > 2);
}

// Not reported: flagged, but an item-level `expect` in the documented form takes it.
#[cfg_attr(
    dylint_lib = "trellis_lints",
    expect(
        iterator_chain_in_for,
        reason = "the fixture checks an item-level expect"
    )
)]
fn excused(values: &[u32]) {
    for doubled in values.iter().map(|value| value * 2) {
        let _ = doubled;
    }
}

// Flagged: the same code with a raw identifier, whose text `is_from_proc_macro` misreads; only
// the code a derive writes is skipped for that.
fn raw_chain(values: &[u8]) {
    for value in r#values.iter().filter(|value| **value > 0) {
        let _ = value;
    }
}
