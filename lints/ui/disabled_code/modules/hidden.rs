//! Flagged: an inner `cfg` that hides the whole module from `trellis_lints`.
#![cfg(not(dylint_lib = "trellis_lints"))]

#[test]
#[ignore]
fn flagged_although_the_module_is_hidden() {}
