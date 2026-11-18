// edition:2024

use std::io;

mod machine {
    pub struct Config;
}

// Not flagged: `Config` is on the configured ambiguous list.
fn config(_: machine::Config) {}

// Flagged: the configured list replaces the default, so `Error` may be imported.
fn fail() -> io::Error {
    io::Error::other("fail")
}

// Not flagged: importing `io::Result` would shadow the prelude's `Result`.
fn read() -> io::Result<()> {
    Ok(())
}

fn main() {}
