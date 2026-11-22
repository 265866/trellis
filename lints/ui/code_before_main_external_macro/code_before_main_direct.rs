// edition:2021
// aux-build:registry.rs
//! A constructor that the registry's own `submit!` writes, called directly, under the `allow` it
//! writes around it.

extern crate registry;

fn start() {}

// Flagged by `code_before_main` alone, which judges the `allow` around the static too. Once
// `constructor_macros` lists `registry::submit`, `suppression_policy` flags the call instead: a
// macro call written in the source under a lint level counts as code written there, which only
// `trusted_macro_lints` lets a macro's lint level cover, as for any trusted crate's macro.
// `deny.toml` bans the crates that place constructors, so only a framework's macros call them.
registry::submit!(start);

fn main() {}
