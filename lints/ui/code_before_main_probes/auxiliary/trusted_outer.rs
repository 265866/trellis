// edition:2021
//! A trusted dependency whose macro writes a module under `allow(warnings)` and calls an untrusted
//! constructor macro inside it.

pub extern crate probes;

/// Writes a module under `allow(warnings)` that holds the registry's constructor.
#[macro_export]
macro_rules! outer {
    () => {
        #[allow(warnings)]
        mod outer_mod {
            $crate::probes::submit_plain!();
        }
    };
}
