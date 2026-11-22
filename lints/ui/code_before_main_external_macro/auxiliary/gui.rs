// edition:2024
//! A facade like `gpui`: it re-exports the derive and the registry, and defines `actions!` in a
//! module of its own, which declares each action as a struct with the derive.

extern crate actions_derive;

pub use actions_derive::Action;

pub mod private {
    pub extern crate registry;
}

mod action {
    #[macro_export]
    macro_rules! actions {
        ($($name:ident),* $(,)?) => {
            $(
                #[derive(Clone, $crate::Action)]
                pub struct $name;
            )*
        };
    }
}
