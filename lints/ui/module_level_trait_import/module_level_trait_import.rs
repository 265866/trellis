// edition:2024

mod render {
    // Flagged: `write!` needs `Write` in one function only.
    use std::fmt::Write as _;

    fn render(orders: &[u64]) -> String {
        let mut out = String::new();
        for order in orders {
            let _ = write!(out, "{order}");
        }
        out
    }

    fn total(orders: &[u64]) -> u64 {
        orders.iter().sum()
    }
}

mod parse {
    // Flagged: a path through the type, `u8::from_str`, in one function.
    use std::str::FromStr;

    // Flagged: a path through the trait inside a closure, in one function.
    use std::ops::Add;

    fn parse(text: &str) -> Option<u8> {
        u8::from_str(text).ok()
    }

    fn sum(values: &[u8]) -> u8 {
        values
            .iter()
            .copied()
            .fold(0, |total, value| Add::add(total, value))
    }
}

mod spread {
    // Not flagged: two functions call `Read` methods.
    use std::io::Read;

    fn first(reader: &mut impl std::io::BufRead) -> String {
        let mut text = String::new();
        let _ = reader.read_to_string(&mut text);
        text
    }

    fn second(bytes: &[u8]) -> usize {
        let mut out = Vec::new();
        let _ = { bytes }.read_to_end(&mut out);
        out.len()
    }
}

mod signatures {
    use std::any::Any;
    use std::fmt::Display;
    use std::hash::{Hash, Hasher};

    // Not flagged: `Display` is a bound in a signature.
    fn show<T: Display>(value: T) -> String {
        value.to_string()
    }

    // Not flagged: `Any` names a field's type.
    struct Holder(Box<dyn Any>);

    fn type_of(holder: &Holder) -> std::any::TypeId {
        holder.0.type_id()
    }

    struct Order(u64);

    // Not flagged: `Hash` is in an `impl` header.
    impl Hash for Order {
        fn hash<H: Hasher>(&self, state: &mut H) {
            self.0.hash(state);
        }
    }
}

mod reexported {
    // Not flagged: a re-export isn't a local import.
    pub use std::fmt::Write;

    fn render() -> String {
        let mut out = String::new();
        let _ = write!(out, "order");
        out
    }
}

mod shared {
    // Not flagged: a child module imports it through this module.
    use std::io::BufRead;

    fn lines(input: &[u8]) -> usize {
        input.lines().count()
    }

    mod child {
        use super::BufRead;

        fn first(input: impl BufRead) -> Option<String> {
            input.lines().next()?.ok()
        }
    }
}

mod shared_by_path {
    // Not flagged: a child module imports it by a path through this module.
    use std::io::Seek;

    fn rewind(file: &mut std::fs::File) {
        let _ = file.rewind();
    }

    mod child {
        use crate::shared_by_path::Seek;

        fn position(file: impl Seek) {}
    }
}

mod globbed {
    // Not flagged: the child module's glob import could rely on it.
    use std::fmt::Write as _;

    fn render() -> String {
        let mut out = String::new();
        let _ = write!(out, "order");
        out
    }

    mod tests {
        use super::*;
    }
}

mod derived {
    // Not flagged: the derive names `Debug` by its absolute path, and `fmt` uses it outside
    // a function, in an `impl` header.
    use std::fmt::Debug;

    #[derive(Debug)]
    struct Order(u64);

    struct Shelf;

    impl Debug for Shelf {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("Shelf")
        }
    }
}

mod linked {
    // Not flagged: an intra-doc link resolves through the import.
    use std::str::FromStr;

    /// Parses a quantity with [`FromStr::from_str`].
    fn parse(text: &str) -> Option<u8> {
        u8::from_str(text).ok()
    }
}

mod mentioned {
    // Flagged: the docs name the trait, but not in a link.
    use std::str::FromStr;

    /// Parses a quantity with `FromStr`.
    fn parse(text: &str) -> Option<u8> {
        u8::from_str(text).ok()
    }
}

#[path = "modules/test_only.rs"]
mod test_only;

fn main() {}
