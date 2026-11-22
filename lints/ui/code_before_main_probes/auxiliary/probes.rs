// edition:2021
//! A dependency whose macros write a constructor under a lint level that could turn
//! `code_before_main` off, or hide the caller's code from every lint.

/// Writes its constructor under `allow(warnings)`, a group that holds `code_before_main`.
#[macro_export]
macro_rules! submit_warnings {
    () => {
        #[allow(warnings)]
        const _: () = {
            extern "C" fn constructor() {}
            #[used]
            #[link_section = ".init_array"]
            static CONSTRUCTOR: extern "C" fn() = constructor;
        };
    };
}

/// Writes its constructor under an `allow` that names `code_before_main`.
#[macro_export]
macro_rules! submit_named {
    () => {
        #[allow(unknown_lints, code_before_main)]
        const _: () = {
            extern "C" fn constructor() {}
            #[used]
            #[link_section = ".init_array"]
            static CONSTRUCTOR: extern "C" fn() = constructor;
        };
    };
}

/// Writes the caller's items into a module of its own, beside its constructor, under lint groups.
#[macro_export]
macro_rules! wrap_user {
    ($($user:item)*) => {
        #[allow(clippy::all, warnings)]
        pub mod wrapped {
            $($user)*

            extern "C" fn constructor() {}
            #[used]
            #[link_section = ".init_array"]
            static CONSTRUCTOR: extern "C" fn() = constructor;
        }
    };
}
