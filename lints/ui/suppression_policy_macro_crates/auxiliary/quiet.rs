//! A dependency whose macros write lint levels into the code that calls them.

#[macro_export]
macro_rules! silence {
    ($item:item) => {
        #[allow(warnings)]
        $item
    };
}

#[macro_export]
macro_rules! documented_exception {
    ($item:item) => {
        #[expect(clippy::unwrap_used, reason = "the dependency's own exception")]
        $item
    };
}

#[macro_export]
macro_rules! terse_exception {
    ($item:item) => {
        #[expect(clippy::unwrap_used, reason = "x")]
        $item
    };
}

#[macro_export]
macro_rules! old_rust {
    ($item:item) => {
        #[clippy::msrv = "1.50"]
        $item
    };
}
