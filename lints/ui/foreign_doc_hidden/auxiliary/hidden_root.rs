//! A crate that hides its own root from its docs unless a feature asks for them, as Microsoft's
//! `windows` crate does so docs.rs doesn't build its whole API. Its items keep their own
//! attributes: the documented ones are its API, and the hidden ones are hidden.
#![cfg_attr(not(feature = "docs"), doc(hidden))]

/// A documented function.
pub fn documented() -> u32 {
    1
}

/// A documented module.
pub mod api {
    /// A documented type.
    pub struct Handle;

    /// A hidden function in a documented module.
    #[doc(hidden)]
    pub fn internal() -> u32 {
        2
    }
}

/// A hidden module.
#[doc(hidden)]
pub mod internals {
    /// A function in a hidden module.
    pub fn helper() -> u32 {
        3
    }
}
