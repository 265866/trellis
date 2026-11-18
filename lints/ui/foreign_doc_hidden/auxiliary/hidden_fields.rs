//! A crate with a public field hidden from its docs, which the crate's own macros or test
//! helpers need public.

/// A context with one documented field and one hidden one.
pub struct Context {
    /// The documented field.
    pub visible: u32,
    /// The hidden field.
    #[doc(hidden)]
    pub hidden: u32,
}

/// A tuple struct whose field is hidden.
pub struct Wrapper(#[doc(hidden)] pub u32);

/// An enum whose variant has a hidden field.
pub enum Event {
    /// An order with a hidden field.
    Order {
        /// The documented field.
        shelf: u32,
        /// The hidden field.
        #[doc(hidden)]
        seed: u32,
    },
}

/// A function hidden from the docs.
#[doc(hidden)]
pub fn helper() {}

/// A module hidden from the docs, whose `deserialize` a derive may call for
/// `#[serde(with = "...")]`.
#[doc(hidden)]
pub mod internals {
    /// Reads a value.
    pub fn deserialize() {}
}

/// A type hidden from the docs, whose `deserialize` a derive may call for
/// `#[serde(with = "...")]`.
#[doc(hidden)]
pub struct Internal;

impl Internal {
    /// Reads a value.
    pub fn deserialize() {}
}

/// A struct with a hidden field, which a derive may mirror for `#[serde(remote = "...")]`.
pub struct Remote {
    /// The documented field.
    pub shown: u32,
    /// The hidden field.
    #[doc(hidden)]
    pub concealed: u32,
    /// Another hidden field, which only the derive names.
    #[doc(hidden)]
    pub own: u32,
}
