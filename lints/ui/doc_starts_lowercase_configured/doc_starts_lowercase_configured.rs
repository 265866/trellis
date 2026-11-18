// The test's `dylint.toml` sets `lowercase_doc_words = ["npm"]`.

/// npm's registry. Not flagged: the configured word.
fn registry() -> u32 {
    1
}

/// trellis's shelf count. Flagged: the configured list replaces the workspace's words.
fn shelves() -> u32 {
    2
}

/// npmjs is flagged: a listed word followed by more letters is another word.
fn other() -> u32 {
    3
}

fn main() {
    let _ = (registry(), shelves(), other());
}
