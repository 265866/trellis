// Flagged: an inner `allow`, hidden from these lints' run.
#![cfg_attr(
    not(dylint_lib = "trellis_lints"),
    allow(clippy::pedantic, clippy::unwrap_used, reason = "x")
)]
// Flagged: a crate-wide `expect`, even in the allowed `cfg_attr`.
#![cfg_attr(
    dylint_lib = "trellis_lints",
    expect(code_comment, reason = "the generated tables carry comments")
)]
// Flagged: a crate-wide `expect` of a lint group.
#![expect(clippy::pedantic, reason = "the prototype predates the style rules")]
// Flagged: an inner `warn` resets a level the workspace set.
#![warn(unused_qualifications)]
// Not flagged: inner `deny` and `forbid` only tighten levels.
#![deny(unsafe_code)]
#![forbid(unstable_features)]

// Flagged: `allow`, even in the allowed `cfg_attr`.
#[cfg_attr(dylint_lib = "trellis_lints", allow(shared_ownership))]
macro_rules! double {
    ($value:expr) => {
        $value * 2
    };
}

// Flagged: `expect` without a reason.
#[cfg_attr(dylint_lib = "trellis_lints", expect(shared_ownership))]
macro_rules! triple {
    ($value:expr) => {
        $value * 3
    };
}

// Flagged: an empty reason.
#[expect(non_snake_case, reason = " ")]
fn Order() {}

// Flagged: `allow`, and a `cfg_attr` predicate other than `dylint_lib = "trellis_lints"`.
#[cfg_attr(test, allow(dead_code))]
fn unused() {}

// Flagged: an `expect` of `warnings` and of a rustc group, behind `cfg_attr(any(..))`.
#[cfg_attr(
    any(dylint_lib = "trellis_lints", clippy),
    expect(warnings, unused, reason = "the function is a placeholder for now")
)]
fn quiet() {}

// Flagged: nested `cfg_attr`, one predicate of which isn't allowed.
#[cfg_attr(
    dylint_lib = "trellis_lints",
    cfg_attr(test, expect(non_snake_case, reason = "mirrors the test data names"))
)]
fn nested() {}

// Flagged: an exception to a lint that has none, even in the documented form.
#[cfg_attr(
    dylint_lib = "trellis_lints",
    expect(code_comment, missing_workspace_lints, reason = "the parser is generated")
)]
fn commented() {}

// Flagged: an exception to `macro_definition`, even in the documented form with a reason; only
// `macro_crates` in `dylint.toml` allows a macro.
#[cfg_attr(
    dylint_lib = "trellis_lints",
    expect(macro_definition, reason = "a function can't take a variable number of arguments")
)]
macro_rules! sum {
    ($($value:expr),*) => {
        0 $(+ $value)*
    };
}

// Not flagged: the documented form of an exception to a custom lint.
#[cfg_attr(
    dylint_lib = "trellis_lints",
    expect(get_prefixed_getter, reason = "mirrors the field name of the vendor's API")
)]
fn get_balance() {}

// Not flagged: an `expect` of one lint, with a reason, and a `deny`.
#[expect(non_snake_case, reason = "mirrors the vendor's field name")]
#[deny(clippy::unwrap_used)]
fn Price() {}

// Flagged: a reason too short to judge, one that is only the lint's name, one under 20
// characters, and one with three words besides the lint's name.
#[expect(non_snake_case, reason = "x")]
fn Short() {}

#[expect(clippy::needless_pass_by_value, reason = "needless pass by value")]
fn takes_ownership(_value: String) {}

#[expect(clippy::needless_pass_by_value, reason = "Clippy: Needless_Pass_By_Value!")]
fn takes_ownership_again(_value: String) {}

#[expect(non_snake_case, reason = "it is ok here")]
fn Brief() {}

#[expect(clippy::needless_pass_by_value, reason = "the value is needless here")]
fn takes_ownership_once_more(_value: String) {}

// Not flagged: four words besides the lint's name, in exactly 20 characters.
#[expect(clippy::needless_pass_by_value, reason = "the callback owns it")]
fn hands_over(_value: String) {}

// Flagged: clippy's own attributes, outer and in `cfg_attr`.
#[clippy::msrv = "1.50"]
fn old_rust() {}

#[clippy::cognitive_complexity = "50"]
fn tangled() {}

#[cfg_attr(dylint_lib = "trellis_lints", clippy::has_significant_drop)]
struct Guard;

#[clippy::dump]
fn dumped() {}

// Not flagged: `automatically_derived`, and another tool's attribute.
struct Plain;

#[automatically_derived]
impl Clone for Plain {
    fn clone(&self) -> Self {
        Self
    }
}

#[rustfmt::skip]
fn unformatted() {}

fn main() {
    // Flagged: an inner `allow` on a function.
    #![allow(unused_variables)]

    let unused_value = 1;
    let text = "#[allow(not_flagged_in_a_string)]";
    let _ = (double!(1), triple!(1), sum!(1, 2), text);
    Order();
    unused();
    quiet();
    nested();
    commented();
    Price();
}

// Flagged: an exception to the other two lints that have none, on an item.
#[cfg_attr(
    dylint_lib = "trellis_lints",
    expect(disabled_code, suppression_policy, reason = "the guard is written by a code generator")
)]
fn guarded() {}

// Flagged: an `expect` on a module, inline or out-of-line, directly or through `cfg_attr`.
#[expect(
    clippy::unwrap_used,
    clippy::missing_panics_doc,
    reason = "the legacy module predates these rules"
)]
mod outer {
    pub fn first(values: &[u32]) -> u32 {
        *values.first().unwrap()
    }
}

#[expect(clippy::unwrap_used, reason = "the legacy module predates these rules")]
#[path = "modules/legacy.rs"]
pub(crate) mod legacy;

#[cfg_attr(
    dylint_lib = "trellis_lints",
    expect(non_leaf_module_items, reason = "the legacy module predates these rules")
)]
mod configured {}

// Flagged inside: a module an inner `cfg` hides.
#[path = "modules/hidden.rs"]
mod hidden;

struct Coupons(u32);

// Flagged: an `expect` on an impl block or a trait of a lint reported inside it.
#[expect(clippy::unwrap_used, reason = "every method here unwraps a lookup")]
impl Coupons {
    fn first(values: &[u32]) -> Self {
        Self(*values.first().unwrap())
    }
}

#[expect(clippy::missing_errors_doc, reason = "the errors are obvious from the types")]
trait Wallet {
    fn balance(&self) -> u32;
}

// Not flagged: an `expect` on an impl block or a trait of a lint reported at the block itself.
#[cfg_attr(
    dylint_lib = "trellis_lints",
    expect(operator_overload, reason = "adding coupons adds their counts")
)]
impl std::ops::Add for Coupons {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self(self.0 + other.0)
    }
}

#[expect(clippy::from_over_into, reason = "mirrors the vendor's API")]
impl Into<u32> for Coupons {
    fn into(self) -> u32 {
        self.0
    }
}

#[cfg_attr(
    dylint_lib = "trellis_lints",
    expect(single_impl_trait, reason = "a second store implements it next")
)]
trait Store {
    fn name(&self) -> &str;
}

// Flagged: attributes whose path, or whose lint, is a macro variable, and an `expect` on an item
// the macro's caller passes.
macro_rules! with_level {
    ($level:ident, $item:item) => {
        #[$level(clippy::unwrap_used)]
        $item
    };
}

with_level!(allow, fn unwrapped() {});

macro_rules! with_inner_level {
    ($level:ident) => {
        fn inner_level() {
            #![$level(clippy::unwrap_used)]
        }
    };
}

with_inner_level!(allow);

macro_rules! with_attributes {
    ($(#[$attribute:meta])* $name:ident) => {
        $(#[$attribute])*
        fn $name() {}
    };
}

with_attributes!(#[allow(dead_code)] attributed);

macro_rules! expecting {
    ($lint:path, $item:item) => {
        #[expect($lint, reason = "the caller names a lint it expects")]
        fn expecting() {}

        #[expect(clippy::unwrap_used, reason = "the caller passes a lookup that never misses")]
        $item

        #[cfg_attr(dylint_lib = "trellis_lints", $lint)]
        fn configured_by_caller() {}
    };
}

expecting!(clippy::pedantic, fn passed() {});

// Flagged: a `#` passed to a macro, brackets after the `:tt` variable that takes it, and the
// `allow` the macro builds from them, after expansion.
macro_rules! accessor {
    ($pound:tt $($item:tt)*) => {
        $pound [allow(warnings, clippy::all)]
        $($item)*
    };
}

accessor! { # pub fn first(values: &[u32]) -> u32 { *values.first().unwrap() } }

// Flagged: brackets after a repetition of `:tt` variables, and an inner `allow` the macro
// writes.
macro_rules! inner_accessor {
    ($($pound:tt)?) => {
        fn inner_accessor() {
            $($pound)? ! [allow(unused_variables)]
            let unused = 1;
        }
    };
}

inner_accessor! { # }

// Flagged: an `allow` a macro puts together from a `#` and brackets its caller passes apart,
// after expansion.
macro_rules! joined {
    ($pound:tt, $brackets:tt, $item:item) => {
        $pound $brackets
        $item
    };
}

joined!(#, [allow(dead_code)], fn joined_item() {});

// Flagged: an attribute that doesn't start with a path, such as `paste` joins into `allow`.
macro_rules! discard {
    ($($tokens:tt)*) => {};
}

discard! { #![[<al low>](clippy::unwrap_used, warnings)] }

// Flagged: an inner clippy attribute, which needs the unstable `custom_inner_attributes` but is
// read where it is written all the same.
discard! { #![clippy::msrv = "1.40"] }

// Flagged: a lint level whose lint isn't written as a path.
discard! { #[expect([<clippy::un wrap_used>], reason = "the caller checks it")] fn pasted() {} }

// Flagged after expansion: lint levels a macro writes without a reason or of a group.
macro_rules! careless {
    ($pound:tt) => {
        $pound [expect(warnings)]
        fn careless() {}
    };
}

careless!(#);

// Not flagged after expansion: a lint level written in a macro's body, which is checked where it
// is written, and the standard library's own macros.
macro_rules! checked {
    ($name:ident) => {
        #[expect(clippy::unwrap_used, reason = "a lookup that can't miss")]
        fn $name(values: &[u32]) -> u32 {
            *values.first().unwrap()
        }
    };
}

checked!(checked_first);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
struct Derived(u32);

#[::std::prelude::v1::derive(Debug)]
struct GloballyDerived;

macro_rules! crate_attribute {
    () => {
        #[$crate::inline_attribute]
        fn never_expanded() {}
    };
}

thread_local! {
    static COUPONS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

// Not flagged: a macro whose attributes are written out, and an attribute other than a lint
// level on the caller's item.
macro_rules! documented {
    ($text:expr, $item:item) => {
        #[doc = $text]
        #[derive(Debug)]
        $item
    };
}

documented!("A documented struct.", struct Documented;);

// Not flagged: an `expect` of `dead_code` alone outside tests, for private code that only its
// tests use until its first caller lands.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the device manager in the next change calls it")
)]
fn connect() {}

// Flagged: `allow` behind `not(test)`.
#[cfg_attr(not(test), allow(dead_code))]
fn disconnect() {}

// Flagged: an `expect` of a lint other than `dead_code` behind `not(test)`, alone or beside it.
#[cfg_attr(
    not(test),
    expect(clippy::unwrap_used, reason = "the device manager in the next change calls it")
)]
fn pair() {}

#[cfg_attr(
    not(test),
    expect(dead_code, non_snake_case, reason = "the device manager in the next change calls it")
)]
fn Unpair() {}

// Flagged: an `expect` of `dead_code` behind any predicate but exactly `not(test)`.
#[cfg_attr(
    test,
    expect(dead_code, reason = "the device manager in the next change calls it")
)]
fn reboot() {}

#[cfg_attr(
    not(any(test, unix)),
    expect(dead_code, reason = "the device manager in the next change calls it")
)]
fn shell() {}

// Flagged: an `expect` behind `not(test)` without a reason, inner, or on a module.
#[cfg_attr(not(test), expect(dead_code))]
fn push() {}

fn pull() {
    #![cfg_attr(
        not(test),
        expect(dead_code, reason = "the device manager in the next change calls it")
    )]
}

#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the device manager in the next change calls it")
)]
mod forward {}
