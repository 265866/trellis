use std::convert::TryInto;
use std::num::ParseIntError;

struct Amount(u64);

impl From<u64> for Amount {
    fn from(cents: u64) -> Self {
        Self(cents)
    }
}

fn take_amount(amount: Amount) -> u64 {
    amount.0
}

fn charge(cents: u64) -> u64 {
    cents
}

fn parse_all(input: &str, words: &[&str]) -> Result<u64, ParseIntError> {
    // Flagged: `.parse()` through `?`, with no type on the binding.
    let cents = input.parse()?;
    charge(cents);

    // Flagged: `.into()` with the type only known from a later use.
    let amount = cents.into();
    take_amount(amount);

    // Flagged: `.collect()` into an inferred collection.
    let lengths = words.iter().map(|word| word.len()).collect();
    let _: &Vec<usize> = &lengths;

    // Flagged: the turbofish gives `.collect()` the type the `let` could carry.
    let upper = words
        .iter()
        .map(|word| word.to_uppercase())
        .collect::<Vec<_>>();

    // Flagged: `.unwrap()` passes the turbofish-typed result through.
    let small = input.parse::<u8>().unwrap();

    // Flagged: `.try_into()` and `.expect(..)`.
    let narrow = cents.try_into().expect("cents fit in a u32");
    let _: u32 = narrow;

    // Flagged: `.map_err(..)`, `.unwrap_or(..)`, `.unwrap_or_default()` and `.ok()` pass the
    // parsed value through.
    let mapped = input.parse().map_err(|error: ParseIntError| error)?;
    charge(mapped);
    let fallback = input.parse().unwrap_or(0);
    charge(fallback);
    let defaulted = input.parse().unwrap_or_default();
    charge(defaulted);
    let maybe = input.parse().ok();
    let _: Option<u64> = maybe;

    // Not flagged: the binding has a type.
    let typed: u64 = input.parse()?;

    // Not flagged: `_` binds nothing for a reader to puzzle over.
    let _ = input.parse::<u16>();

    // Not flagged: a `let ... else` pattern can only carry the whole `Result` type.
    let Ok(digit) = input.parse::<u8>() else {
        return Ok(0);
    };

    // Not flagged: a turbofish of only `_` names no type; the conversion feeds a further call.
    let count = words.iter().collect::<Vec<_>>().len();

    // Not flagged: a method named `parse` that isn't `str::parse`.
    let parsed = Parser.parse();

    Ok(cents + typed)
}

#[derive(Default)]
struct Settings {
    orders: u32,
}

fn iterators(input: &[&str], values: &[u64], pairs: &[(u8, u8)]) -> Result<u64, ParseIntError> {
    // Not flagged: only the turbofish can say which `Result` to collect into before the `?`.
    let parsed = input.iter().map(|word| word.parse::<u64>()).collect::<Result<Vec<_>, _>>()?;

    // Not flagged: the same for an `Option` through `.unwrap()` and `.expect(..)`.
    let present = values.iter().map(|value| value.checked_mul(2)).collect::<Option<Vec<_>>>().unwrap();
    let firsts = values.iter().map(|value| value.checked_add(1)).collect::<Option<Vec<_>>>().expect("no overflow");

    // Flagged: the turbofish gives `.sum()` the type the `let` could carry.
    let total = values.iter().sum::<u64>();

    // Flagged: `.product()` into an inferred number.
    let product = values.iter().product();
    let _: u64 = product;

    // Flagged: `.partition(..)` given its collection by turbofish.
    let (small, large) = values.iter().partition::<Vec<&u64>, _>(|value| **value < 3);

    // Flagged: `.unzip()` into inferred collections.
    let (lefts, rights) = pairs.iter().copied().unzip();
    let _: (&Vec<u8>, &Vec<u8>) = (&lefts, &rights);

    // Flagged: `Default::default()` doesn't say what it builds.
    let settings = Default::default();
    let _: &Settings = &settings;

    // Not flagged: the path names the type.
    let zero = u64::default();
    let fresh = Settings::default();

    // Not flagged: an exception on the statement, with a reason.
    #[cfg_attr(
        dylint_lib = "trellis_lints",
        expect(inferred_conversion_binding, reason = "the binding is passed straight to `charge`")
    )]
    let excepted = input[0].parse()?;
    charge(excepted);

    let _ = (parsed, present, firsts, small, large, zero, fresh.orders, settings.orders);
    Ok(total)
}

struct Parser;

impl Parser {
    fn parse(&self) -> u8 {
        0
    }
}

macro_rules! parse_here {
    ($input:expr) => {
        let from_macro = $input.parse::<u32>();
    };
}

fn main() {
    let _ = parse_all("1", &["a"]);
    let _ = iterators(&["1"], &[1, 2], &[(1, 2)]);
    // Not flagged: the `let` comes from a macro.
    parse_here!("2");
}

// Not reported: flagged, but an item-level `expect` in the documented form takes it.
#[cfg_attr(
    dylint_lib = "trellis_lints",
    expect(
        inferred_conversion_binding,
        reason = "the fixture checks an item-level expect"
    )
)]
fn excused(input: &str) -> Result<u64, ParseIntError> {
    let cents = input.parse()?;
    Ok(charge(cents))
}

// Flagged: the same code with a raw identifier, whose text `is_from_proc_macro` misreads; only
// the code a derive writes is skipped for that.
fn raw_convert(cents: u64) {
    let amount = r#cents.into();
    take_amount(amount);
}
