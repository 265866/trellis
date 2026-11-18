//! A counter and its view, written in the style these lints enforce, so that every lint checks
//! idiomatic code at once. The rules come from the
//! [Google Rust Style Guide](https://google.github.io/styleguide/rustguide.html).

use std::error;
use std::fmt::{self, Display, Formatter};
use std::rc::Rc;

/// The stylesheet the view draws with, kept in a file of its own.
pub const STYLESHEET: &str = include_str!("theme.css");

/// How many times the user has pressed the button, and the most it may reach.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counter {
    /// The presses so far.
    count: u32,
    /// The count the counter stops at.
    limit: u32,
}

impl Counter {
    /// Builds a counter at zero that stops at `limit`.
    ///
    /// # Errors
    ///
    /// Returns [`LimitError`] when `limit` is zero, since such a counter could never count.
    pub fn try_new(limit: u32) -> Result<Self, LimitError> {
        if limit == 0 {
            return Err(LimitError);
        }
        Ok(Self { count: 0, limit })
    }

    /// The presses so far.
    #[must_use]
    pub fn count(&self) -> u32 {
        self.count
    }

    /// Counts one press, staying at the limit once it reaches it.
    pub fn press(&mut self) {
        self.count = (self.count + 1).min(self.limit);
    }
}

impl Display for Counter {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} of {}", self.count, self.limit)
    }
}

/// The error [`Counter::try_new`] returns for a limit of zero.
#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LimitError;

impl Display for LimitError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("a counter needs a limit above zero")
    }
}

impl error::Error for LimitError {}

/// The colors the window and its views draw with.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Theme {
    /// The accent color, as `0xRRGGBB`, the hex notation that
    /// <https://www.w3.org/TR/css-color-4/#hex-notation> describes.
    pub accent: u32,
}

impl Theme {
    /// The red channel of the accent color.
    #[must_use]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "shifting a 24-bit color right by 16 bits leaves 8 bits"
    )]
    pub fn red(&self) -> u8 {
        (self.accent >> 16) as u8
    }
}

/// A counter as the window shows it, drawn with the window's theme.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CounterView {
    /// The counter the view shows.
    pub counter: Counter,
    /// The theme, which the window and each of its views share.
    #[cfg_attr(
        dylint_lib = "trellis_lints",
        expect(
            shared_ownership,
            reason = "the window and every view draw with one theme, and none of them outlives \
                      the others"
        )
    )]
    pub theme: Rc<Theme>,
}

/// One line per counter, numbered from one, such as `1: 3 of 10`.
#[must_use]
pub fn summary(counters: &[Counter]) -> String {
    let mut lines: Vec<String> = Vec::new();
    for (index, counter) in counters.iter().enumerate() {
        lines.push(format!("{}: {counter}", index + 1));
    }
    lines.join("\n")
}

/// Tests of the counter, which only test builds compile.
#[cfg(test)]
mod tests {
    use super::Counter;

    /// Pressing past the limit leaves the count at the limit.
    #[test]
    fn press_stops_at_the_limit() {
        let mut counter = Counter::try_new(2).expect("a limit of two is valid");
        for _ in 0..3 {
            counter.press();
        }
        assert_eq!(counter.count(), 2, "the count stays at the limit");
    }
}
