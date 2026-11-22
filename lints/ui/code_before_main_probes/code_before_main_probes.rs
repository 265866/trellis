// edition:2024
// aux-build:probes.rs
//! Constructors under a dependency's lint levels that could turn `code_before_main` off. Each
//! level is flagged by `suppression_policy`, so something always reports the macro call.

extern crate probes;

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

fn main() {}
