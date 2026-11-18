//! Flagged: a module that isn't a test module, so its code may not be under `cfg(test)`.

#[cfg(test)]
pub fn helper() {}
