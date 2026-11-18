use std::cell::Cell;
use std::collections::HashSet;

struct Tally {
    errors: u32,
}

fn parse(text: &str) -> Result<u32, std::num::ParseIntError> {
    text.parse()
}

fn main() {
    let texts = ["1", "x", "3"];

    // Flagged: the guide's example, a counter bumped inside `map`.
    let mut error_count = 0;
    let parsed: Vec<u32> = texts
        .iter()
        .map(|text| parse(text))
        .map(|result| {
            if result.is_err() {
                error_count += 1;
            }
            result
        })
        .filter_map(Result::ok)
        .collect();

    // Flagged: `for_each` pushing onto a captured vector.
    let mut doubled = Vec::new();
    parsed.iter().for_each(|value| doubled.push(value * 2));

    // Flagged: `filter` mutating a captured set, through a `&mut self` method.
    let mut seen = HashSet::new();
    let unique: Vec<&u32> = parsed.iter().filter(|value| seen.insert(**value)).collect();

    // Flagged: a field of a captured struct, in `any`.
    let mut tally = Tally { errors: 0 };
    let _ = texts.iter().any(|text| {
        tally.errors += 1;
        text.is_empty()
    });

    // Flagged: writes through a captured `&mut` reference, a unique borrow, in `inspect`.
    let mut total = 0;
    let total_ref = &mut total;
    let summed: u32 = parsed.iter().inspect(|value| *total_ref += **value).sum();

    // Flagged: a `DoubleEndedIterator` method.
    let mut checked = 0;
    let _ = parsed.iter().rfind(|value| {
        checked += 1;
        **value > 1
    });

    // Flagged: `fold` mutating captured state besides its accumulator.
    let mut folds = 0;
    let _ = parsed.iter().fold(0, |sum, value| {
        folds += 1;
        sum + value
    });

    // Not flagged: `fold` and `scan` mutate state the iterator hands them, not a capture.
    let _ = parsed.iter().fold(0, |mut sum, value| {
        sum += value;
        sum
    });
    let _: Vec<u32> = parsed
        .iter()
        .scan(0, |running, value| {
            *running += value;
            Some(*running)
        })
        .collect();

    // Flagged: lending a captured vector out `&mut`.
    let mut lent = Vec::new();
    let _: Vec<u32> = parsed
        .iter()
        .map(|value| {
            push_into(&mut lent, *value);
            *value
        })
        .collect();

    // Not flagged: closures that only read captured state.
    let limit = 2;
    let _ = parsed.iter().filter(|value| **value > limit).count();

    // Flagged: a `move` closure bumping its own copy of a counter, as in the guide's example.
    let mut index = 0;
    let _: Vec<(u32, u32)> = parsed
        .iter()
        .map(move |value| {
            index += 1;
            (index, *value)
        })
        .collect();

    // Not flagged: interior mutability through a shared borrow.
    let calls = Cell::new(0);
    let _ = parsed.iter().map(|value| {
        calls.set(calls.get() + 1);
        value
    });

    // Not flagged: a `for` loop, and closures passed to methods that aren't on `Iterator`.
    let mut loop_count = 0;
    for _ in &parsed {
        loop_count += 1;
    }
    let mut option_count = 0;
    let _ = Some(1).map(|value| {
        option_count += 1;
        value
    });
    let mut retained = 0;
    let mut kept = parsed.clone();
    kept.retain(|_| {
        retained += 1;
        true
    });

    // Not flagged: a macro writes the closure.
    let mut from_macro = 0;
    macro_rules! count_all {
        ($values:expr) => {
            $values.iter().for_each(|_| from_macro += 1)
        };
    }
    count_all!(parsed);

    let _ = (
        error_count,
        doubled,
        unique,
        tally.errors,
        summed,
        checked,
        folds,
        index,
    );
    let _ = (calls, loop_count, option_count, retained, from_macro);
    let _ = counted_in_place(&parsed);
    let mut table = Table { rows: 0 };
    let mut log = Log { entries: 0 };
    let _ = Report.write_all(&["a", "b"], &mut table, &mut log);
    let _ = lent;
}

fn push_into(values: &mut Vec<u32>, value: u32) {
    values.push(value);
}

struct Table {
    rows: u32,
}

struct Log {
    entries: u32,
}

impl Log {
    fn record(&mut self) {
        self.entries += 1;
    }
}

struct Report;

impl Report {
    fn write_row(&self, name: &str, table: &mut Table, log: &mut Log) -> usize {
        table.rows += 1;
        log.record();
        name.len()
    }

    // Not flagged: passing captured `&mut` references on, and calling a method through one.
    fn write_all(&self, names: &[&str], table: &mut Table, log: &mut Log) -> Vec<usize> {
        names
            .iter()
            .map(|name| {
                log.record();
                self.write_row(name, table, log)
            })
            .collect()
    }
}

// Not flagged: the `expect` on the function covers the closure in its body, and is fulfilled.
#[cfg_attr(
    dylint_lib = "trellis_lints",
    expect(
        mutating_iterator_closure,
        reason = "the fixture's sanctioned exception"
    )
)]
fn counted_in_place(values: &[u32]) -> u32 {
    let mut count = 0;
    values.iter().for_each(|_| count += 1);
    count
}

// Flagged: the same code with a raw identifier, whose text `is_from_proc_macro` misreads; only
// the code a derive writes is skipped for that.
fn raw_for_each(values: &[u32]) -> Vec<u32> {
    let mut doubled = Vec::new();
    r#values.iter().for_each(|value| doubled.push(value * 2));
    doubled
}
