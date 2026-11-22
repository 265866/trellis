//! A package that `restricted_crates` lets only some members reach.

use cargo_metadata::Package;

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

    /// Whether the entry lets the member `name` reach the package.
    pub(crate) fn allows(&self, name: &str) -> bool {
        self.allowed.iter().any(|allowed| allowed == name)
    }

    /// Whether `package` is one the entry names.
    pub(crate) fn names(&self, package: &Package) -> bool {
        self.pattern.matches(package)
    }
}

/// A package `restricted_crates` names, by one of the three forms its keys take.
#[derive(Debug, PartialEq, Eq)]
enum Pattern {
    /// A package whose name is this, such as `gpui`, as `package_name` writes it.
    Name(String),
    /// A package whose name starts with this, from a key that ends in `*`, such as `gpui-*`, as
    /// `package_name` writes it.
    Prefix(String),
    /// A package from this repository, from a key that starts with `git+`, `registry+` or
    /// `sparse+`, such as `git+https://github.com/zed-industries/zed`.
    Source(Repository),
}

impl Pattern {
    /// The pattern a `restricted_crates` key writes, or `None` if the key is none of the forms.
    fn parse(key: &str) -> Option<Self> {
        if let Some(repository) = Repository::parse(key) {
            return Some(Self::Source(repository));
        }
        let is_name = |name: &str| {
            !name.is_empty()
                && name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        };
        match key.strip_suffix('*') {
            Some(prefix) if is_name(prefix) => Some(Self::Prefix(package_name(prefix))),
            None if is_name(key) => Some(Self::Name(package_name(key))),
            _ => None,
        }
    }

    /// Whether `package` matches the pattern.
    fn matches(&self, package: &Package) -> bool {
        match self {
            Self::Name(name) => package_name(&package.name) == *name,
            Self::Prefix(prefix) => package_name(&package.name).starts_with(prefix.as_str()),
            Self::Source(repository) => package
                .source
                .as_ref()
                .and_then(|source| Repository::parse(&source.repr))
                .is_some_and(|source| source == *repository),
        }
    }
}

/// `name` in lowercase with each `-` read as `_`, so two spellings of one package name compare
/// equal, as crates.io, which ignores case, and Cargo, which reads `-` and `_` as one, compare
/// them.
fn package_name(name: &str) -> String {
    name.to_ascii_lowercase().replace('-', "_")
}

/// The repository a package source names, with what can differ between two spellings of one
/// repository taken out.
#[derive(Debug, PartialEq, Eq)]
struct Repository {
    /// The kind of source: `git`, `registry` or `sparse`.
    kind: String,
    /// The host and path in lowercase, without the scheme, a `user@`, a port, a query, a
    /// fragment, a trailing `/` or a trailing `.git`, and with each percent-escape of an
    /// unreserved character decoded, such as `github.com/zed-industries/zed`.
    location: String,
}

impl Repository {
    /// The repository of `source`, a source as Cargo writes one, such as
    /// `git+https://github.com/zed-industries/zed?branch=main#0123abc`, or `None` if it isn't a
    /// git, registry or sparse source or names no location.
    fn parse(source: &str) -> Option<Self> {
        let (kind, url) = source.split_once('+')?;
        if !matches!(kind, "git" | "registry" | "sparse") {
            return None;
        }
        let url = url.to_lowercase();
        let url = url.split_once("://").map_or(url.as_str(), |(_, rest)| rest);
        let url = url.split(['?', '#']).next().unwrap_or_default();
        let authority = url.split('/').next().unwrap_or_default();
        let url = authority.rfind('@').map_or(url, |at| &url[at + 1..]);
        let (host, path) = url.split_once('/').unwrap_or((url, ""));
        let host = host
            .rsplit_once(':')
            .filter(|(_, port)| !port.is_empty() && port.bytes().all(|byte| byte.is_ascii_digit()))
            .map_or(host, |(host, _)| host);
        let url = unreserved_decoded(&format!("{host}/{path}"));
        let url = url.trim_end_matches('/');
        let location = url
            .strip_suffix(".git")
            .unwrap_or(url)
            .trim_end_matches('/');
        if location.is_empty() {
            return None;
        }
        Some(Self {
            kind: kind.to_owned(),
            location: location.to_owned(),
        })
    }
}

/// `text` with each percent-escape of an unreserved character, a letter, a digit, `-`, `.`, `_`
/// or `~`, decoded and in lowercase, since a URL means the same with or without it.
fn unreserved_decoded(text: &str) -> String {
    let mut decoded = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('%') {
        decoded.push_str(&rest[..at]);
        let unreserved = rest
            .get(at + 1..at + 3)
            .and_then(|hex| u8::from_str_radix(hex, 16).ok())
            .map(char::from)
            .filter(|character| character.is_ascii_alphanumeric() || "-._~".contains(*character));
        if let Some(character) = unreserved {
            decoded.push(character.to_ascii_lowercase());
            rest = &rest[at + 3..];
        } else {
            decoded.push('%');
            rest = &rest[at + 1..];
        }
    }
    decoded.push_str(rest);
    decoded
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
            Pattern::parse("Zed-*"),
            Some(Pattern::Prefix("zed_".to_owned()))
        );
        for (key, kind) in [
            ("git+https://github.com/zed-industries/zed", "git"),
            ("registry+https://github.com/zed-industries/zed", "registry"),
            ("sparse+https://github.com/zed-industries/zed", "sparse"),
        ] {
            assert_eq!(
                Pattern::parse(key),
                Some(Pattern::Source(Repository {
                    kind: kind.to_owned(),
                    location: "github.com/zed-industries/zed".to_owned(),
                })),
                "{key:?}"
            );
        }
        for key in [
            "",
            "*",
            "gp*ui",
            "gpui**",
            "a b",
            "https://github.com/x",
            "git+",
            "git+https://",
            "sparse+/",
            "gitx+https://github.com/x",
        ] {
            assert_eq!(Pattern::parse(key), None, "{key:?}");
        }
    }

    #[test]
    fn sources_name_the_same_repository_however_they_are_written() {
        let key = Repository::parse("git+https://github.com/zed-industries/zed");
        assert!(key.is_some(), "the key should parse");
        for source in [
            "git+https://github.com/zed-industries/zed",
            "git+https://github.com/zed-industries/zed#0123abc",
            "git+https://github.com/Zed-Industries/zed?branch=x",
            "git+https://github.com/zed-industries/zed?branch=x#0123abc",
            "git+ssh://git@github.com/zed-industries/zed",
            "git+https://github.com/zed-industries/zed.git",
            "git+https://github.com/zed-industries/ZED.GIT",
            "git+https://github.com/zed-industries/zed/",
            "git+https://github.com/zed-industries/zed.git/",
            "git+https://user@GitHub.com/zed-industries/zed",
            "git+ssh://git@github.com:22/zed-industries/zed",
            "git+https://github.com:443/zed-industries/zed",
            "git+https://github.com/zed%2Dindustries/%7a%65d",
        ] {
            assert_eq!(Repository::parse(source), key, "{source:?}");
        }
        for source in [
            "git+https://github.com/zed-industries/zed-fork#abc",
            "git+https://github.com/zed-industries/zed2",
            "git+https://github.com/zed-industries",
            "git+https://gitlab.com/zed-industries/zed",
            "registry+https://github.com/zed-industries/zed",
            "sparse+https://github.com/zed-industries/zed",
            "git+https://github.com/zed-industries/zed%2Ffork",
        ] {
            assert_ne!(Repository::parse(source), key, "{source:?}");
        }
        assert_eq!(
            Repository::parse("sparse+https://index.crates.io/"),
            Repository::parse("sparse+https://INDEX.crates.io"),
            "a sparse source should compare without its trailing `/` or case"
        );
    }

    #[test]
    fn names_compare_in_any_case_with_dash_and_underscore_as_one() {
        assert_eq!(package_name("Gpui-Macros"), "gpui_macros");
        assert_eq!(package_name("gpui_macros"), package_name("GPUI-MACROS"));
    }
}
