// Turns off every warning in the run of `trellis_lints`, `suppression_policy`'s reports included,
// so they become errors.
#![cfg_attr(dylint_lib = "trellis_lints", allow(warnings))]

mod legacy {
    // An inner attribute on a module doesn't reach the crate root, where the lint reports.
    #![expect(suppression_policy, reason = "the legacy module predates the policy")]
}

fn main() {}
