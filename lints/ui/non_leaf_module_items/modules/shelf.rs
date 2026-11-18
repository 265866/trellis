// Not flagged: a `#[cfg(test)]` child module leaves this module a leaf.
pub struct Shelf;

#[cfg(test)]
mod tests {}
