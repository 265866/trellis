//! A package that `restricted_crates` lets only some members reach.

use cargo_metadata::{Metadata, PackageId};

/// A package that `restricted_crates` names, and the members that may reach it.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct RestrictedCrate {
    /// Which packages the entry's key names.
    pattern: Pattern,
    /// The members that may reach them.
    allowed: Vec<String>,
}

impl RestrictedCrate {
    /// The entry with the key `key` and the members `allowed`, or `None` if the key names no
    /// package.
    pub(crate) fn new(key: &str, allowed: Vec<String>) -> Option<Self> {
        Pattern::parse(key).map(|pattern| Self { pattern, allowed })
    }

    /// The members that may reach the package.
    pub(crate) fn allowed(&self) -> &[String] {
        &self.allowed
    }

    /// Whether the package `id` of `metadata` is one the entry names.
    pub(crate) fn names(&self, metadata: &Metadata, id: &PackageId) -> bool {
        self.pattern.matches(metadata, id)
    }
}

/// A package `restricted_crates` names, by one of the three forms its keys take.
#[derive(Debug, PartialEq, Eq)]
enum Pattern {
    /// A package whose name is this, such as `gpui`.
    Name(String),
    /// A package whose name starts with this, from a key that ends in `*`, such as `gpui*`.
    Prefix(String),
    /// A package whose source starts with this, from a key that starts with `git+` or
    /// `registry+`, such as `git+https://github.com/zed-industries/zed`.
    Source(String),
}

impl Pattern {
    /// The pattern a `restricted_crates` key writes, or `None` if the key is none of the forms.
    fn parse(key: &str) -> Option<Self> {
        if key.starts_with("git+") || key.starts_with("registry+") {
            return Some(Self::Source(key.to_owned()));
        }
        let is_name = |name: &str| {
            !name.is_empty()
                && name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        };
        match key.strip_suffix('*') {
            Some(prefix) if is_name(prefix) => Some(Self::Prefix(prefix.to_owned())),
            None if is_name(key) => Some(Self::Name(key.to_owned())),
            _ => None,
        }
    }

    /// Whether the package `id` of `metadata` matches the pattern. Names match with `-` and `_`
    /// read as one, as Cargo reads them.
    fn matches(&self, metadata: &Metadata, id: &PackageId) -> bool {
        let package = &metadata[id];
        let name = package.name.replace('-', "_");
        match self {
            Self::Name(pattern) => name == pattern.replace('-', "_"),
            Self::Prefix(prefix) => name.starts_with(&prefix.replace('-', "_")),
            Self::Source(source) => package
                .source
                .as_ref()
                .is_some_and(|actual| actual.repr.starts_with(source.as_str())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_parse_as_names_prefixes_and_sources() {
        assert_eq!(
            Pattern::parse("gpui"),
            Some(Pattern::Name("gpui".to_owned()))
        );
        assert_eq!(
            Pattern::parse("zed-*"),
            Some(Pattern::Prefix("zed-".to_owned()))
        );
        assert_eq!(
            Pattern::parse("git+https://github.com/zed-industries/zed"),
            Some(Pattern::Source(
                "git+https://github.com/zed-industries/zed".to_owned()
            ))
        );
        for key in ["", "*", "gp*ui", "gpui**", "a b", "https://github.com/x"] {
            assert_eq!(Pattern::parse(key), None, "{key:?}");
        }
    }
}
