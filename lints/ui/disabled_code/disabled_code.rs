// Flagged: an always-false `cfg_attr` predicate on the crate.
#![cfg_attr(any(), no_std)]
// Not flagged: the one `cfg_attr` on `debug_assertions` allowed, which only sets how Windows
// starts the binary.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// Flagged: an ignored test, with or without a reason, and through `cfg_attr`.
#[test]
#[ignore]
fn stops_at_target() {}

#[test]
#[ignore = "flaky on CI"]
fn stops_at_the_stock_limit() {}

#[test]
#[cfg_attr(windows, ignore)]
fn reconnects() {}

// Flagged: predicates that are false in every build.
#[cfg(any())]
fn old_parser() {}

#[cfg(false)]
fn older_parser() {}

#[cfg(not(all()))]
fn oldest_parser() {}

#[cfg(all(unix, any()))]
mod unix_only {}

#[cfg_attr(not(any(true, unix)), derive(Debug))]
struct Order;

// Flagged: `all` that contradicts itself.
#[cfg(all(unix, windows))]
fn both_families() {}

#[cfg(all(target_os = "linux", target_os = "macos"))]
fn two_systems() {}

#[cfg(all(test, not(test)))]
fn tested_and_not() {}

// Flagged: code that only one lint run sees, or neither.
#[cfg(not(clippy))]
fn hidden_from_clippy() {}

#[cfg(dylint_lib = "trellis_lints")]
fn hidden_from_builds() {}

#[cfg_attr(not(dylint_lib = "trellis_lints"), path = "other.rs")]
mod configured {}

#[cfg(doc)]
fn only_in_docs() {}

#[cfg(not(miri))]
fn hidden_from_miri() {}

#[cfg_attr(rustfmt, rustfmt::skip)]
fn skipped_by_rustfmt() {}

#[cfg(rust_analyzer)]
fn only_in_the_editor() {}

// Flagged: code that differs between the lint runs and release builds.
#[cfg(not(debug_assertions))]
fn release_only() {}

#[cfg(debug_assertions)]
fn debug_only() {}

#[cfg(feature = "serde")]
fn with_feature() {}

#[cfg(not(feature = "serde"))]
fn without_feature() {}

#[cfg(panic = "abort")]
fn when_aborting() {}

#[cfg_attr(feature = "serde", derive(Clone))]
struct Receipt;

#[cfg_attr(not(debug_assertions), derive(Debug))]
struct Amount;

// Flagged: code that only targets CI doesn't build compile, or that some CI targets leave out.
#[cfg(all(windows, target_arch = "aarch64"))]
fn windows_on_arm() {}

#[cfg(target_os = "freebsd")]
fn freebsd() {}

#[cfg(any(target_os = "linux", target_family = "wasm"))]
fn linux_or_wasm() {}

#[cfg(not(target_pointer_width = "64"))]
fn narrow() {}

#[cfg_attr(target_env = "musl", derive(Debug))]
struct Static;

#[cfg(target_has_atomic = "64")]
fn atomic() {}

#[cfg(target_feature = "avx2")]
fn vectorized() {}

#[cfg(target_endian = "big")]
fn big_endian() {}

#[cfg(target_vendor = "apple")]
fn apple() {}

#[cfg(target_abi = "eabihf")]
fn hard_float() {}

// Flagged: code only doctests compile.
#[cfg(doctest)]
fn only_in_doctests() {}

// Not flagged: the targets CI builds.
#[cfg(windows)]
fn windows_only() {}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_desktop() {}

#[cfg(target_family = "unix")]
fn unix_family() {}

#[cfg(any(target_os = "windows", target_family = "windows"))]
fn windows_family() {}

// Flagged: code that changes under tests, outside test code.
#[cfg(test)]
fn test_helper() {}

#[cfg(not(test))]
mod product_only {}

#[cfg(any(test, unix))]
mod tests_or_unix {}

#[cfg_attr(test, derive(PartialEq))]
struct Wallet;

impl Wallet {
    #[cfg(test)]
    fn balance_for_tests(&self) -> u32 {
        0
    }
}

// Flagged: the exception to `dead_code` nested with a lint run's predicate, either way round,
// since the outer `cfg_attr` holds a `cfg_attr` rather than only lint levels.
#[cfg_attr(
    not(test),
    cfg_attr(
        dylint_lib = "trellis_lints",
        expect(dead_code, reason = "the device manager in the next change calls it")
    )
)]
fn nested_exception() {}

#[cfg_attr(
    dylint_lib = "trellis_lints",
    cfg_attr(
        not(test),
        expect(dead_code, reason = "the device manager in the next change calls it")
    )
)]
fn nested_the_other_way() {}

// Not flagged: test modules, and code inside them or a test function under `cfg(test)`.
#[cfg(test)]
mod unit_tests {
    #[cfg(test)]
    fn helper() {}

    #[cfg_attr(test, derive(Debug))]
    struct Fixture;

    mod nested {
        #[cfg(all(test, unix))]
        fn unix_helper() {}
    }

    // Flagged: a predicate that is false in tests hides code in a test module too.
    #[cfg(not(test))]
    #[test]
    fn hidden_test() {}
}

#[cfg(all(test, windows))]
#[path = "modules/test_helpers.rs"]
mod test_helpers;

// Checked inside: a test module of tests with a test framework's attributes.
#[cfg(test)]
#[path = "modules/framework_tests.rs"]
mod framework_tests;

#[test]
fn checks_an_order() {
    #[cfg(test)]
    let spun = true;
    let _ = spun;
}

// Flagged inside: a module file that isn't a test module.
#[path = "modules/product.rs"]
mod product;

// Flagged: an `#[ignore]` that a macro builds from a `#` its caller passes, after expansion.
macro_rules! ignored {
    ($pound:tt $($item:tt)*) => {
        $pound [ignore = "flaky on CI"]
        $($item)*
    };
}

ignored! { # fn flaky() {} }

// Not flagged after expansion: an `#[ignore]` written in a macro's body, and one its caller
// passes, which are flagged where they are written.
macro_rules! written {
    ($(#[$attribute:meta])* $name:ident) => {
        #[ignore]
        $(#[$attribute])*
        fn $name() {}
    };
}

written!(#[ignore] forwarded);

// Flagged: a predicate the macro's caller writes.
macro_rules! when {
    ($predicate:meta, $item:item) => {
        #[cfg($predicate)]
        $item
    };
}

when!(unix, fn unix_helper() {});

macro_rules! check {
    ($predicate:meta) => {
        cfg!($predicate)
    };
}

// Flagged: `include!`, however it is named, and the code it brings in.
include!("modules/included.rs");

use std::include as bring;
bring!("modules/renamed.rs");

// Not flagged: text that `include_str!` reads, even from a `.rs` file.
const DATA: &str = include_str!("modules/data.rs");

// Flagged inside: a module that an inner `cfg` hides, and one whose declaration a `cfg` leaves
// out.
#[path = "modules/hidden.rs"]
mod hidden;

#[cfg(any())]
#[path = "modules/never.rs"]
mod never;

// Not flagged: ordinary predicates, and the documented exceptions to a custom lint and to
// `dead_code`, which `suppression_policy` checks.
#[cfg(any(unix, windows))]
fn desktop() {}

#[cfg(all(unix, target_os = "linux"))]
fn linux() {}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn unix_desktop() {}

#[cfg(test)]
mod tests {}

#[cfg_attr(
    dylint_lib = "trellis_lints",
    expect(non_snake_case, reason = "mirrors the vendor's own field name")
)]
fn Exception() {}

#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the device manager in the next change calls it")
)]
fn connect() {}

/// Flagged: doctests marked `ignore`, however written.
///
/// ```ignore
/// order();
/// ```
///
/// ```rust,ignore
/// order();
/// ```
///
/// ~~~ ignore
/// order();
/// ~~~
///
/// ````edition2024,ignore
/// order();
/// ````
///
/// Not flagged: blocks that run, compile, or aren't Rust.
///
/// ```
/// order();
/// ```
///
/// ```no_run
/// order();
/// ```
///
/// ```text
/// ```ignore
/// ```
///
/// ```text,ignore
/// not Rust
/// ```
///
/// ```json
/// {}
/// ```
fn documented() {}

mod shelves {
    //! Flagged: an ignored doctest in inner docs.
    //!
    //! ```ignore
    //! order();
    //! ```
}

// Flagged: code only a proc-macro crate builds.
#[cfg(proc_macro)]
fn expanding() {}

// Flagged: a `cfg_select!` arm that tells the lint runs apart; the `_` arm after it is the
// code the other run sees.
cfg_select! {
    any(clippy, dylint_lib = "trellis_lints") => {
        fn linted() {}
    }
    _ => {
        fn linted() {}
    }
}

// Not flagged: an arm per system CI builds, and a `_` arm for the rest.
cfg_select! {
    windows => {
        fn separator() -> char { '\\' }
    }
    _ => {
        fn separator() -> char { '/' }
    }
}

// Flagged: the `_` arm, which no build selects, since `unix` and `windows` cover every built
// system.
cfg_select! {
    unix => {
        fn family() {}
    }
    windows => {
        fn family() {}
    }
    _ => {
        fn family() {}
    }
}

// Flagged: an arm on `debug_assertions`.
cfg_select! {
    debug_assertions => {
        fn checked() {}
    }
    _ => {
        fn checked() {}
    }
}

// Flagged: `cfg_select!` and `cfg!` imported, renamed or not, so a call the lint can't find by
// name could hide an arm only one lint run selects.
use std::cfg_select as pick;
use std::{cfg as flag, cfg_select};

pick! {
    any(clippy, dylint_lib = "trellis_lints") => {
        fn picked() -> u8 {
            42
        }
    }
    _ => {
        fn picked() -> u8 {
            "42".parse().unwrap()
        }
    }
}

// Not flagged: a module named `cfg`, imported by its name, which isn't the macro.
mod cfg {
    pub(crate) fn enabled() -> bool {
        true
    }
}

mod settings {
    use crate::cfg;

    pub(crate) fn enabled() -> bool {
        cfg::enabled()
    }
}

fn main() {
    let _ = (picked(), flag!(clippy), settings::enabled());
    // Flagged: an expression arm on a target CI doesn't build every way.
    let width = cfg_select! {
        target_pointer_width = "64" => 8,
        _ => 4,
    };
    // Not flagged: expression arms on built systems.
    let shell = cfg_select! {
        windows => "cmd",
        unix => "sh",
    };
    let _ = (width, shell, separator(), linted(), checked(), family());
    // Flagged: `cfg!` that is false in every build, that tells the lint runs apart, that differs
    // in release builds, or that the macro's caller writes.
    if cfg!(any()) {
        desktop();
    }
    let in_clippy = cfg!(clippy);
    let checked = cfg!(debug_assertions);
    let on_unix = check!(unix);
    // Not flagged: an ordinary `cfg!`.
    let on_windows = cfg!(windows);
    // Flagged: `cfg!` on `test`, and on a target CI doesn't build every way.
    let testing = cfg!(test);
    let on_arm = cfg!(target_arch = "aarch64");
    let _ = (in_clippy, checked, on_unix, on_windows, testing, on_arm);
    let _ = (Order, Receipt, Amount, DATA);
    desktop();
    Exception();
    documented();
    let _ = (included(), renamed());
}
