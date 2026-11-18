// module_level_trait_import.rs includes this file in a library build, and
// module_level_trait_import_test_build.rs in a test build, so each result here holds in both.

mod globbed_by_tests {
    // Flagged: a test module's glob import doesn't keep it.
    use std::fmt::Write as _;

    fn render() -> String {
        let mut out = String::new();
        let _ = write!(out, "order");
        out
    }

    #[cfg(test)]
    mod tests {
        use super::*;
    }
}

mod imported_by_tests {
    // Flagged: a test module's import through this module doesn't keep it.
    use std::io::Seek;

    fn rewind(file: &mut std::fs::File) {
        let _ = file.rewind();
    }

    #[cfg(test)]
    mod tests {
        use super::Seek;
    }
}

mod used_in_tests {
    // Flagged: uses in test-only code don't count.
    use std::str::FromStr;

    fn parse(text: &str) -> Option<u8> {
        u8::from_str(text).ok()
    }

    #[cfg(test)]
    fn fixture() -> Option<u8> {
        u8::from_str("1").ok()
    }

    #[test]
    fn parses() {
        let _ = u8::from_str("1");
    }

    struct Shelf;

    impl Shelf {
        #[cfg(test)]
        fn stop() -> Option<u8> {
            u8::from_str("2").ok()
        }
    }
}

mod test_import {
    // Not flagged: a test-only import isn't checked.
    #[cfg(test)]
    use std::ops::Add;

    #[cfg(test)]
    fn sum(value: u8) -> u8 {
        Add::add(value, 1)
    }
}

mod excused {
    // Not flagged: an `expect` on the `use` covers it.
    #[cfg_attr(
        dylint_lib = "trellis_lints",
        expect(module_level_trait_import, reason = "the fixture checks an item-level expect")
    )]
    use std::fmt::Write as _;

    // Not flagged: an `expect` on a `use` list covers each import in it.
    #[cfg_attr(
        dylint_lib = "trellis_lints",
        expect(module_level_trait_import, reason = "the fixture checks an item-level expect")
    )]
    use std::io::{BufRead, Read};

    fn render() -> String {
        let mut out = String::new();
        let _ = write!(out, "order");
        out
    }

    fn read(mut input: &[u8]) -> usize {
        let mut text = String::new();
        let _ = input.read_to_string(&mut text);
        input.lines().count()
    }
}
