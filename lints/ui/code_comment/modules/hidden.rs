//! A module an inner `cfg` hides from `trellis_lints`, read all the same.
#![cfg(not(dylint_lib = "trellis_lints"))]

// Flagged: a line comment in a hidden module.
pub fn hidden() {}
