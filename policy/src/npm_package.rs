//! An npm package installed under `node_modules`, with its license and the people its
//! `package.json` credits.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::policy_error::PolicyError;

/// Text that, in a lowercased file name, marks a license or notice file, such as `LICENSE`,
/// `MIT-LICENSE`, `ThirdPartyNoticeText.txt` or `CopyrightNotice.txt`.
const LICENSE_FILE_MARKERS: [&str; 4] = ["licen", "notice", "copying", "copyright"];

/// Endings that, in a lowercased file name, mark code or data rather than a license file, such as
/// `license.js` or `licenses.json`. `.d.ts` ends in `.ts`.
const CODE_FILE_ENDINGS: [&str; 10] = [
    ".json", ".js", ".mjs", ".cjs", ".jsx", ".ts", ".mts", ".cts", ".tsx", ".map",
];

/// An npm package installed under `node_modules`: what its `package.json` says, and the text of
/// its license files.
///
/// The fields are in the order the derived [`Ord`] compares them, so copies of a package that
/// differ only in their directory sort together.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct NpmPackage {
    /// The package's name, with its scope, such as `@scope/name`, as its `package.json` states it.
    name: String,
    /// The package's version.
    version: String,
    /// The SPDX license expressions the package may be used under, any one of them: one for a
    /// `license`, one per entry of a `licenses` array, and none when its `package.json` states
    /// none.
    licenses: Vec<String>,
    /// The package's author, if its `package.json` names one.
    author: Option<String>,
    /// The package's contributors, as its `package.json` lists them.
    contributors: Vec<String>,
    /// The package's maintainers, as its `package.json` lists them.
    maintainers: Vec<String>,
    /// The text of each license file directly in the package's directory, sorted by file name.
    license_texts: Vec<String>,
    /// The directory the package is installed in.
    directory: PathBuf,
}

/// What a directory an installer places packages in, such as `node_modules/<name>`, holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum NpmPackageDirectory {
    /// Nothing at all, so no package.
    Empty,
    /// Something that isn't a package the check can name, with what is wrong, worded to follow
    /// the directory in a sentence: files but no `package.json`, a `package.json` that isn't a
    /// regular file, or one without a non-empty string `name` and `version`.
    Unidentified(String),
    /// An installed package.
    Package(NpmPackage),
}

impl NpmPackage {
    /// Reads the package in `directory`, a directory an installer places a package in, with the
    /// license files whose real path is in `root`, the canonical path of the `node_modules` that
    /// holds it.
    ///
    /// A `package.json` that is a symbolic link isn't read, so the package can't come from
    /// outside `root`. The package's name is the one its `package.json` states, which differs from
    /// the directory's for an alias such as `"leftpad": "npm:left-pad@1.3.0"`. Its licenses are
    /// `license` when it is a string, `license.type` when it is an object, or else the `type` of
    /// each entry of a `licenses` array.
    ///
    /// # Errors
    ///
    /// Fails if the directory or a file in it can't be read, or if `package.json` isn't JSON.
    pub(crate) fn read(directory: &Path, root: &Path) -> Result<NpmPackageDirectory, PolicyError> {
        let manifest_path = directory.join("package.json");
        let read_error = |path: &Path| {
            let path = path.to_owned();
            move |source| PolicyError::Read { path, source }
        };
        let metadata = match fs::symlink_metadata(&manifest_path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                let mut entries = fs::read_dir(directory).map_err(read_error(directory))?;
                return Ok(if entries.next().is_none() {
                    NpmPackageDirectory::Empty
                } else {
                    NpmPackageDirectory::Unidentified("holds files but no package.json".to_owned())
                });
            }
            Err(source) => return Err(read_error(&manifest_path)(source)),
        };
        if metadata.is_symlink() {
            return Ok(NpmPackageDirectory::Unidentified(
                "has a package.json that is a symbolic link, which the check doesn't follow"
                    .to_owned(),
            ));
        }
        if !metadata.is_file() {
            return Ok(NpmPackageDirectory::Unidentified(
                "has a package.json that isn't a file".to_owned(),
            ));
        }
        let text = fs::read_to_string(&manifest_path).map_err(read_error(&manifest_path))?;
        let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
        let manifest: Value = serde_json::from_str(text).map_err(|source| PolicyError::Json {
            path: manifest_path,
            source,
        })?;
        let (Some(name), Some(version)) = (
            non_empty_string(&manifest, "name"),
            non_empty_string(&manifest, "version"),
        ) else {
            return Ok(NpmPackageDirectory::Unidentified(
                "holds a package.json without a string `name` and `version`".to_owned(),
            ));
        };
        Ok(NpmPackageDirectory::Package(Self {
            name: name.to_owned(),
            version: version.to_owned(),
            licenses: licenses(&manifest),
            author: manifest.get("author").and_then(person),
            contributors: people(&manifest, "contributors"),
            maintainers: people(&manifest, "maintainers"),
            license_texts: license_texts(directory, root)?,
            directory: directory.to_owned(),
        }))
    }

    /// The package's name, with its scope, such as `@scope/name`.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The package's version.
    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }

    /// The package's license expression, if its `package.json` states one: its one license, or
    /// the alternatives of a `licenses` array each in parentheses, joined with ` OR `.
    #[must_use]
    pub fn license(&self) -> Option<String> {
        match self.licenses.as_slice() {
            [] => None,
            [license] => Some(license.clone()),
            alternatives => {
                let mut parenthesized = Vec::new();
                for alternative in alternatives {
                    parenthesized.push(format!("({alternative})"));
                }
                Some(parenthesized.join(" OR "))
            }
        }
    }

    /// The SPDX license expressions the package may be used under, any one of them, each to be
    /// judged on its own. Empty when its `package.json` states no license.
    #[must_use]
    pub fn licenses(&self) -> &[String] {
        &self.licenses
    }

    /// The package's author, as `Name <email>`, if its `package.json` names one.
    #[must_use]
    pub fn author(&self) -> Option<&str> {
        self.author.as_deref()
    }

    /// The package's contributors, each as `Name <email>`.
    #[must_use]
    pub fn contributors(&self) -> &[String] {
        &self.contributors
    }

    /// The package's maintainers, each as `Name <email>`.
    #[must_use]
    pub fn maintainers(&self) -> &[String] {
        &self.maintainers
    }

    /// The directory the package is installed in.
    #[must_use]
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// The text of each license file directly in the package's directory, sorted by file name.
    #[must_use]
    pub fn license_texts(&self) -> &[String] {
        &self.license_texts
    }

    /// Whether `other` is the same package with the same license, people and license texts,
    /// whatever directory each is installed in.
    pub(crate) fn has_same_contents(&self, other: &Self) -> bool {
        self.name == other.name
            && self.version == other.version
            && self.licenses == other.licenses
            && self.author == other.author
            && self.contributors == other.contributors
            && self.maintainers == other.maintainers
            && self.license_texts == other.license_texts
    }
}

/// The value of `key` in `manifest`, if it is a non-empty string.
fn non_empty_string<'a>(manifest: &'a Value, key: &str) -> Option<&'a str> {
    manifest
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
}

/// The license expressions the `package.json` `manifest` states, any one of which applies. An
/// entry of a `licenses` array without a string `type` is the empty expression, which no
/// accepted license satisfies.
fn licenses(manifest: &Value) -> Vec<String> {
    match manifest.get("license") {
        Some(Value::String(expression)) => return vec![expression.clone()],
        Some(Value::Object(license)) => {
            return license
                .get("type")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .into_iter()
                .collect();
        }
        Some(_) | None => {}
    }
    let mut types = Vec::new();
    for entry in manifest
        .get("licenses")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let license_type = entry
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        types.push(license_type.to_owned());
    }
    types
}

/// The people the array `key` of the `package.json` `manifest` lists.
fn people(manifest: &Value, key: &str) -> Vec<String> {
    let mut people = Vec::new();
    for entry in manifest
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        people.extend(person(entry));
    }
    people
}

/// A person as a `package.json` names one: a string such as `Name <email> (url)`, written as it
/// is, or an object with a `name` and an `email`, written as `Name <email>`.
fn person(value: &Value) -> Option<String> {
    if let Some(person) = value.as_str() {
        return (!person.is_empty()).then(|| person.to_owned());
    }
    let name = non_empty_string(value, "name");
    let email = non_empty_string(value, "email");
    match (name, email) {
        (Some(name), Some(email)) => Some(format!("{name} <{email}>")),
        (Some(name), None) => Some(name.to_owned()),
        (None, Some(email)) => Some(format!("<{email}>")),
        (None, None) => None,
    }
}

/// Whether a file named `name` is a license or notice file rather than code.
fn is_license_file_name(name: &str) -> bool {
    let name = name.to_lowercase();
    LICENSE_FILE_MARKERS
        .iter()
        .any(|marker| name.contains(marker))
        && !CODE_FILE_ENDINGS
            .iter()
            .any(|ending| name.ends_with(ending))
}

/// The text of each license file directly in `directory`, sorted by name. A symbolic link is read
/// only when it leads to a file inside `root`, and a file that isn't UTF-8 text without NUL
/// characters, such as an image, is skipped.
fn license_texts(directory: &Path, root: &Path) -> Result<Vec<String>, PolicyError> {
    let read_error = |path: &Path| {
        let path = path.to_owned();
        move |source| PolicyError::Read { path, source }
    };
    let mut files = Vec::new();
    for entry in fs::read_dir(directory).map_err(read_error(directory))? {
        let entry = entry.map_err(read_error(directory))?;
        let path = entry.path();
        let name = entry.file_name();
        if !is_license_file_name(&name.to_string_lossy()) {
            continue;
        }
        let file_type = entry.file_type().map_err(read_error(&path))?;
        if file_type.is_file() {
            files.push((name, path));
        } else if file_type.is_symlink()
            && let Ok(target) = fs::canonicalize(&path)
            && target.starts_with(root)
            && target.is_file()
        {
            files.push((name, target));
        }
    }
    files.sort();
    let mut texts = Vec::new();
    for (_, path) in files {
        let bytes = fs::read(&path).map_err(read_error(&path))?;
        if let Ok(text) = String::from_utf8(bytes)
            && !text.contains('\0')
        {
            texts.push(text);
        }
    }
    Ok(texts)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn license_and_notice_files_count_and_code_doesnt() {
        for name in [
            "LICENSE",
            "license.md",
            "LICENCE-MIT",
            "MIT-LICENSE",
            "COPYING",
            "NOTICE",
            "ThirdPartyNoticeText.txt",
            "CopyrightNotice.txt",
            "THIRD-PARTY-LICENSE",
            "THIRD-PARTY-NOTICES.md",
        ] {
            assert!(is_license_file_name(name), "{name} is a license file");
        }
        for name in [
            "README.md",
            "license.js",
            "licenses.json",
            "notice.d.ts",
            "license.mjs",
            "license.cjs",
            "licence.js.map",
            "copyright.ts",
        ] {
            assert!(!is_license_file_name(name), "{name} isn't a license file");
        }
    }

    #[test]
    fn people_are_written_as_name_and_email() {
        for (value, expected) in [
            (
                json!("Ada <ada@example.com> (https://ada.example)"),
                Some("Ada <ada@example.com> (https://ada.example)"),
            ),
            (
                json!({"name": "Ada", "email": "ada@example.com", "url": "https://ada.example"}),
                Some("Ada <ada@example.com>"),
            ),
            (json!({"name": "Ada"}), Some("Ada")),
            (
                json!({"email": "ada@example.com"}),
                Some("<ada@example.com>"),
            ),
            (json!({"url": "https://ada.example"}), None),
            (json!(""), None),
            (json!(1), None),
        ] {
            assert_eq!(
                person(&value).as_deref(),
                expected,
                "{value} names that person"
            );
        }
    }
}
