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

/// Writes its constructor under the one `allow` that `inventory::submit!` writes.
#[macro_export]
macro_rules! submit_plain {
    () => {
        #[allow(non_upper_case_globals)]
        const _: () = {
            extern "C" fn constructor() {}
            #[used]
            #[link_section = ".init_array"]
            static CONSTRUCTOR: extern "C" fn() = constructor;
        };
    };
}

/// Writes the caller's items into a module of its own, beside its constructor, under one lint.
#[macro_export]
macro_rules! wrap_quiet {
    ($($user:item)*) => {
        #[allow(dead_code)]
        pub mod quiet_wrapped {
            $($user)*

            extern "C" fn constructor() {}
            #[used]
            #[link_section = ".init_array"]
            static CONSTRUCTOR: extern "C" fn() = constructor;
        }
    };
}

/// Writes its constructor under an `allow` that names `code_before_main` with a tool's prefix.
#[macro_export]
macro_rules! submit_tool {
    () => {
        #[allow(unknown_lints, clippy::code_before_main)]
        const _: () = {
            extern "C" fn constructor() {}
            #[used]
            #[link_section = ".init_array"]
            static CONSTRUCTOR: extern "C" fn() = constructor;
        };
    };
}

/// Writes its constructor under an `allow` of one lint, with a reason.
#[macro_export]
macro_rules! submit_reason {
    () => {
        #[allow(non_upper_case_globals, reason = "the registry names its statics in lowercase")]
        const _: () = {
            extern "C" fn constructor() {}
            #[used]
            #[link_section = ".init_array"]
            static CONSTRUCTOR: extern "C" fn() = constructor;
        };
    };
}

/// Writes the caller's items beside a static whose first `link_section` is an ordinary section
/// and whose second is a constructor's, which rustc drops.
#[macro_export]
macro_rules! wrap_duplicate_section {
    ($($user:item)*) => {
        #[allow(dead_code)]
        pub mod duplicate_wrapped {
            $($user)*

            extern "C" fn constructor() {}
            #[used]
            #[link_section = ".text.ordinary"]
            #[link_section = ".init_array"]
            static CONSTRUCTOR: extern "C" fn() = constructor;
        }
    };
}
