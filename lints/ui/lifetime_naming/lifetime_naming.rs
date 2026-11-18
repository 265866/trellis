// Flagged: a struct's lifetime is a type lifetime.
struct Parser<'a> {
    input: &'a str,
}

// Flagged: each single-letter lifetime on an enum.
enum Token<'a, 'b> {
    Word(&'a str),
    Number(&'b str),
}

// Flagged: an impl declares its own lifetime.
impl<'a> Parser<'a> {
    // Not flagged: two single-letter lifetimes relating a method's inputs to its output.
    fn pick<'x, 'y>(&self, first: &'x str, second: &'y str) -> (&'x str, &'y str) {
        (first, second)
    }
}

// Flagged: a trait's lifetime.
trait Source<'a> {
    fn next(&mut self) -> Option<&'a str>;
}

// Flagged: a type alias's lifetime.
type Words<'a> = Vec<&'a str>;

// Not flagged: descriptive names on a type and its impl.
struct Lexer<'input> {
    input: &'input str,
}

impl<'input> Lexer<'input> {
    fn rest(&self) -> &'input str {
        self.input
    }
}

// Not flagged: the anonymous lifetime.
impl Lexer<'_> {
    fn len(&self) -> usize {
        self.input.len()
    }
}

// Flagged: three single-letter lifetimes on one function.
fn join<'a, 'b, 'c>(first: &'a str, second: &'b str, third: &'c str) -> usize {
    first.len() + second.len() + third.len()
}

// Not flagged: two single-letter lifetimes and a descriptive one.
fn longest<'a, 'b, 'shared>(first: &'a str, second: &'b str, shared: &'shared str) -> &'shared str {
    let _ = (first, second);
    shared
}

// Not flagged: higher-ranked lifetimes are local to the bound.
fn apply<F>(function: F) -> usize
where
    F: for<'a> Fn(&'a str) -> usize,
{
    function("shelf")
}

macro_rules! borrowed {
    ($name:ident) => {
        struct $name<'a> {
            input: &'a str,
        }
    };
}

// Not flagged: the lifetime comes from a macro expansion.
borrowed!(Borrowed);

fn main() {
    let _ = (join("a", "b", "c"), longest("a", "b", "c"), apply(str::len));
}

// Not reported: flagged, but an item-level `expect` in the documented form takes it.
#[cfg_attr(
    dylint_lib = "trellis_lints",
    expect(lifetime_naming, reason = "the fixture checks an item-level expect")
)]
struct Excused<'a> {
    input: &'a str,
}
