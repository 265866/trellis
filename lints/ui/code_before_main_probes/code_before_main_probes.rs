// edition:2024
// aux-build:probes.rs
// aux-build:trusted_outer.rs
//! Constructors under a dependency's lint levels. One that could turn `code_before_main` off, or
//! sits where something else turned it off, is flagged by `suppression_policy`; any other is left
//! to `code_before_main`. Either way something reports the macro call.

extern crate probes;
extern crate trusted_outer;

// Flagged: `allow(warnings)` holds `code_before_main`, which reports nothing under it.
probes::submit_warnings!();

// Flagged twice: an `allow`, and one of `code_before_main`, which has no exceptions.
probes::submit_named!();

// Flagged: the groups hide the code written here and the static passed in from every lint.
probes::wrap_user! {
    pub fn user_code() -> i32 {
        1 + 1
    }

    extern "C" fn user_init() {}

    #[used]
    #[unsafe(link_section = ".init_array")]
    static USER: extern "C" fn() = user_init;
}

// Flagged by `suppression_policy`: the trusted macro's `allow(warnings)` around the call turns
// `code_before_main` off at the static, so the registry's own `allow` is checked as its macro's.
trusted_outer::outer!();

// Flagged by `code_before_main` alone, unless `constructor_macros` lists the macro: then
// `suppression_policy` flags the code written here under its `allow(dead_code)`, which
// `trusted_macro_lints` doesn't list.
probes::wrap_quiet! {
    pub fn quiet_code() -> i32 {
        2 + 2
    }
}

// Flagged by both: `suppression_policy` checks a level naming `code_before_main` behind a tool's
// prefix as its macro's, and the tool's lint of that name isn't `code_before_main`.
probes::submit_tool!();

// Flagged by `code_before_main` alone: a reason names no lint.
probes::submit_reason!();

mod quiet {
    #![allow(code_before_main)]

    // Flagged by `suppression_policy`, as is the inner `allow` above: `code_before_main` is off
    // here, so the registry's own `allow` is checked as its macro's.
    probes::submit_plain!();
}

fn main() {}
