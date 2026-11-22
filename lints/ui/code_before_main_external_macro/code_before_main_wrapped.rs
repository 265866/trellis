// edition:2024
// aux-build:registry.rs
// aux-build:actions_derive.rs
// aux-build:gui.rs
// aux-build:fw.rs
// aux-build:fw_macros.rs
//! Constructors under the workspace's own macros.

extern crate fw;
extern crate fw_macros;
extern crate gui;

macro_rules! own_actions {
    ($($name:ident),*) => {
        gui::actions!($($name),*);
    };
}

// Flagged at the wrapper, with help naming `gui::actions`, the macro to list; passes once
// `constructor_macros` lists it, since no workspace macro is inside it.
own_actions!(Save);

macro_rules! own_constructor {
    () => {
        extern "C" fn start() {}

        #[used]
        #[unsafe(link_section = ".init_array")]
        static START: extern "C" fn() = start;
    };
}

// Flagged at the call inside, the outermost macro call in the source: `fw::app!` passes the
// tokens through with their own spans, so the static isn't expanded from it.
fw::app! {
    own_constructor!();
}

// Flagged, even with `fw_macros::app` listed: this `app!` writes the tokens at its own call site,
// so the static is expanded from it, but from the workspace's own macro inside it too, which may
// write anything.
fw_macros::app! {
    own_constructor!();
}

fn main() {}
