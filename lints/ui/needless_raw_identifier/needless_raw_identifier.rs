// edition:2021

use std::sync::Arc;

struct Holder {
    // Flagged: `Arc` isn't a keyword.
    shared: r#Arc<u8>,
    // Not flagged: `type` is a keyword in every edition.
    r#type: u8,
    // Not flagged: `gen` is a keyword from edition 2024, so a 2021 crate may rename it ahead.
    r#gen: u8,
    // Flagged: `union` is only a contextual keyword, usable as a name in every edition.
    r#union: u8,
}

// Flagged: a raw lifetime whose name isn't a keyword.
fn first<'r#input>(text: &'r#input str) -> &'r#input str {
    text
}

macro_rules! double {
    // Flagged: in a macro's body too.
    ($value:expr) => {
        r#u32::from($value) * 2
    };
}

fn main() {
    // Flagged: a raw local name, and a raw call of a function.
    let r#holder = Holder {
        shared: Arc::new(1),
        r#type: 2,
        r#gen: 3,
        union: 4,
    };
    let _ = (r#first("text"), double!(holder.r#type), r#holder.r#gen);
}
