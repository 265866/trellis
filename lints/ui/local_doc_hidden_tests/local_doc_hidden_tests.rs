// Not flagged in a test build: the hidden items `#[test]` and derives generate.

/// The shelves.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Shelves;

#[test]
fn orders() {
    assert_eq!(Shelves, Shelves::default());
}
