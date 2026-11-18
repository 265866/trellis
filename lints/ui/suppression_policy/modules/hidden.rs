//! A module an inner `cfg` hides from `trellis_lints`, read all the same.
#![cfg(not(dylint_lib = "trellis_lints"))]

// Flagged: an `allow` in the hidden module.
#[allow(dead_code)]
fn hidden() {}
