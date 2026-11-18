// edition:2024

use std::fmt::Write as _;
use std::sync::Mutex;

#[derive(Debug)]
struct Error;

fn save() -> Result<(), Error> {
    Err(Error)
}

fn load() -> Result<u64, Error> {
    Ok(1)
}

#[must_use]
struct Receipt;

fn receipt() -> Receipt {
    Receipt
}

#[must_use]
fn total() -> u64 {
    1
}

async fn fetch() -> u64 {
    1
}

macro_rules! discard {
    ($value:expr) => {
        _ = $value
    };
}

fn flagged(mut out: String) {
    // Flagged: a `Result`, from a call, an `io::Result`, a macro and a variable.
    _ = save();
    _ = std::fs::remove_file("amount.log");
    _ = write!(out, "{}", 1);
    let loaded = load();
    _ = loaded;

    // Flagged: what `.is_ok()` leaves of a `Result`.
    _ = save().is_ok();

    // Flagged: a `#[must_use]` type, a `#[must_use]` function's value and a future.
    _ = receipt();
    _ = total();
    _ = fetch();

    // Flagged: a `Result` bound to a name starting with `_`, which is never read.
    let _saved = save();
    let _loaded: Result<u64, Error> = load();

    // Flagged: what `.is_ok()`, `.is_err()`, `.ok()` and `.err()` leave of a `Result`.
    let _ok = save().is_ok();
    let _failed = save().is_err();
    let _value = load().ok();
    let _error = load().err();

    // Flagged: an `if` with an empty body and no `else` on a `Result`, or on what `.is_ok()` or
    // `.ok()` leaves of one.
    if let Ok(()) = save() {}
    if let Err(_) = save() {}
    if save().is_ok() {}
    if let Some(_) = load().ok() {}
}

fn not_flagged_if(mut values: Vec<u64>) -> u64 {
    // Not flagged: a body that uses the value, and an `else`.
    if let Ok(value) = load() {
        return value;
    }
    if let Ok(()) = save() {
    } else {
        return 0;
    }

    // Not flagged: an `Option`, and a `bool` that isn't from a `Result`.
    if let Some(_) = values.pop() {}
    if values.is_empty() {}

    // Not flagged: a body that is empty only once `cfg` strips its code.
    if let Ok(value) = load() {
        #[cfg(any())]
        return value;
    }

    // Not flagged: a `while let` loop draining until an error.
    while let Ok(_) = load() {}
    1
}

fn not_flagged(mut values: Vec<u64>, lock: &Mutex<u64>) -> Result<(), Error> {
    // Not flagged: an `Option` and a plain value aren't `#[must_use]`.
    _ = values.pop();
    let count = values.len();
    _ = count;

    // Not flagged: a borrow of a `Result` discards nothing.
    let loaded = load();
    _ = &loaded;
    let value = loaded?;

    // Not flagged: a guard bound to keep it alive is no `Result`, nor is a plain value.
    let _guard = lock.lock().expect("the lock should not be poisoned");
    let _total = value;

    // Not flagged: `let _ = ..` is `clippy::let_underscore_must_use`'s to check.
    let _ = save();

    // Not flagged: a named binding that is read, and a handled error.
    let saved = save();
    saved?;
    save()?;

    // Not flagged: the code a macro writes.
    discard!(save());

    // Not flagged: an `expect` on the `let` covers it.
    #[cfg_attr(
        dylint_lib = "trellis_lints",
        expect(discarded_result, reason = "the fixture checks a statement-level expect")
    )]
    let _saved = save();
    Ok(())
}

// Not flagged: an `expect` on the function covers an assignment, which takes no attributes.
#[cfg_attr(
    dylint_lib = "trellis_lints",
    expect(discarded_result, reason = "the fixture checks a function-level expect")
)]
fn excused() {
    _ = save();
}

// Not flagged: an `if` statement takes its `expect` on the statement itself.
fn excused_if() {
    #[cfg_attr(
        dylint_lib = "trellis_lints",
        expect(discarded_result, reason = "the fixture checks an expect on an if statement")
    )]
    if save().is_ok() {}
}

fn main() {
    flagged(String::new());
    let _ = not_flagged_if(Vec::new());
    let _ = not_flagged(Vec::new(), &Mutex::new(0));
    excused();
    excused_if();
}

// Flagged: the same code with a raw identifier, whose text `is_from_proc_macro` misreads; only
// the code a derive writes is skipped for that.
fn raw_discard() {
    let r#_parsed = "1".parse::<u8>();
}
