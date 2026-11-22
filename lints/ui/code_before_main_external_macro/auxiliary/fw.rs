// edition:2024
//! A framework whose `app!` takes the app's items as free tokens and writes them inside items of
//! its own.

#[macro_export]
macro_rules! app {
    ($($items:tt)*) => {
        pub mod app {
            $($items)*
        }
    };
}
