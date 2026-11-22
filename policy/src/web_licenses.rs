//! The licenses a project accepts for one npm package each, from its `web-licenses.toml`.

use std::borrow::Cow;
use std::collections::BTreeSet;
use std::path::Path;

use spdx::Licensee;
use toml::{Table, Value};

use crate::accepted_licenses::AcceptedLicenses;
use crate::npm_package::NpmPackage;
use crate::policy_error::PolicyError;
use crate::toml_file;
use crate::web_notices::WebNotices;

/// What an entry of `web-licenses.toml` looks like, for error messages.
const ENTRY_SHAPE: &str = "each `[[package]]` entry should have a `name`, the npm package's name; \
                           `licenses`, a non-empty array of SPDX license ids such as \
                           [\"OFL-1.1\"]; and `reason`, why the package may use them";

/// The licenses a project accepts for one npm package each, beyond the `accepted` array of its
/// `about.toml`, which applies to every package. Each `[[package]]` entry of its
/// `web-licenses.toml` names a package by its exact npm name, scope included, the SPDX license
/// ids accepted for every installed version of it, and why.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WebLicenses {
    /// The entries, in the file's order.
    packages: Vec<PackageLicenses>,
}

/// The licenses accepted for one npm package.
#[derive(Clone, Debug, PartialEq, Eq)]
struct PackageLicenses {
    /// The package's npm name, such as `@fontsource/inter`.
    name: String,
    /// The licenses accepted for it, such as `OFL-1.1`.
    licensees: Vec<Licensee>,
}

impl WebLicenses {
    /// Reads the `web-licenses.toml` at `path`.
    ///
    /// # Errors
    ///
    /// Fails if the file can't be read or isn't TOML, if it has a key other than `package`, or if
    /// an entry has a key other than `name`, `licenses` and `reason`, no name, a name another
    /// entry has, no licenses or one that isn't an SPDX license id, or an empty reason.
    pub fn read(path: &Path) -> Result<Self, PolicyError> {
        let table = toml_file::read(path)?;
        Self::from_table(&table).map_err(|message| PolicyError::Malformed {
            path: path.to_owned(),
            message,
        })
    }

    /// The entries in `table`, the parsed file, or what is wrong with it.
    fn from_table(table: &Table) -> Result<Self, String> {
        if let Some(key) = table.keys().find(|key| *key != "package") {
            return Err(format!(
                "unknown key `{}`; the file holds only `[[package]]` entries",
                toml_file::render_key(key)
            ));
        }
        let entries = match table.get("package") {
            None => return Ok(Self::default()),
            Some(Value::Array(entries)) => entries,
            Some(_) => {
                return Err(format!(
                    "`package` should be an array of tables; {ENTRY_SHAPE}"
                ));
            }
        };
        let mut packages = Vec::new();
        let mut names = BTreeSet::new();
        for (index, entry) in entries.iter().enumerate() {
            let number = index + 1;
            let Value::Table(entry) = entry else {
                return Err(format!(
                    "`package` entry {number} isn't a table; {ENTRY_SHAPE}"
                ));
            };
            let package = PackageLicenses::from_table(entry)
                .map_err(|problem| format!("`package` entry {number} {problem}; {ENTRY_SHAPE}"))?;
            if !names.insert(package.name.clone()) {
                return Err(format!(
                    "`package` entry {number} names {:?}, as an earlier entry does; put its \
                     licenses in one entry",
                    package.name
                ));
            }
            packages.push(package);
        }
        Ok(Self { packages })
    }

    /// The licenses accepted for the npm package `name`: `accepted`, plus the licenses its entry
    /// accepts if it has one.
    #[must_use]
    pub fn accepted_for<'accepted>(
        &self,
        name: &str,
        accepted: &'accepted AcceptedLicenses,
    ) -> Cow<'accepted, AcceptedLicenses> {
        match self.packages.iter().find(|package| package.name == name) {
            Some(package) => Cow::Owned(accepted.with(&package.licensees)),
            None => Cow::Borrowed(accepted),
        }
    }

    /// The names of the entries that name no package in `installed`, in the file's order.
    #[must_use]
    pub fn stale(&self, installed: &WebNotices) -> Vec<&str> {
        let names: BTreeSet<&str> = installed.packages().iter().map(NpmPackage::name).collect();
        self.packages
            .iter()
            .map(|package| package.name.as_str())
            .filter(|name| !names.contains(name))
            .collect()
    }

    /// How many entries the file has.
    #[must_use]
    pub fn len(&self) -> usize {
        self.packages.len()
    }

    /// Whether the file has no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.packages.is_empty()
    }
}

impl PackageLicenses {
    /// The entry `entry`, or what is wrong with it, said after "entry N".
    fn from_table(entry: &Table) -> Result<Self, String> {
        if let Some(key) = entry
            .keys()
            .find(|key| !["name", "licenses", "reason"].contains(&key.as_str()))
        {
            return Err(format!(
                "has the unknown key `{}`",
                toml_file::render_key(key)
            ));
        }
        let name = match entry.get("name") {
            Some(Value::String(name)) if !name.trim().is_empty() && name.trim() == name => name,
            Some(value) => return Err(format!("has the name {}", toml_file::render(value))),
            None => return Err("has no `name`".to_owned()),
        };
        let licensees = match entry.get("licenses") {
            Some(Value::Array(values)) if !values.is_empty() => {
                let mut licensees = Vec::new();
                for (index, value) in values.iter().enumerate() {
                    if values[..index].contains(value) {
                        return Err(format!(
                            "has {} twice in `licenses`",
                            toml_file::render(value)
                        ));
                    }
                    licensees.push(licensee(value).ok_or_else(|| {
                        format!(
                            "has {} in `licenses`, which isn't an SPDX license id",
                            toml_file::render(value)
                        )
                    })?);
                }
                licensees
            }
            Some(value) => {
                return Err(format!("has the licenses {}", toml_file::render(value)));
            }
            None => return Err("has no `licenses`".to_owned()),
        };
        match entry.get("reason") {
            Some(Value::String(reason)) if !reason.trim().is_empty() => {}
            Some(value) => return Err(format!("has the reason {}", toml_file::render(value))),
            None => return Err("has no `reason`".to_owned()),
        }
        Ok(Self {
            name: name.clone(),
            licensees,
        })
    }
}

/// The license `value` names, if it is a string that holds one SPDX license id in its exact case,
/// as the `accepted` array of `about.toml` takes it, without a `WITH` exception, which the check
/// never accepts.
fn licensee(value: &Value) -> Option<Licensee> {
    let licensee = Licensee::parse(value.as_str()?).ok()?;
    licensee.as_ref().addition.is_none().then_some(licensee)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;
    use crate::accepted_licenses::LicenseVerdict;

    const HEADER: &str = "# Licenses accepted for one npm package each.\n";

    fn parse(text: &str) -> Result<WebLicenses, String> {
        WebLicenses::from_table(&toml_file::parse(text).unwrap())
    }

    fn accepted() -> AcceptedLicenses {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("about.toml");
        fs::write(&path, "accepted = [\"Apache-2.0\", \"ISC\", \"MIT\"]\n").unwrap();
        AcceptedLicenses::read(&path).unwrap()
    }

    fn judge(licenses: &WebLicenses, name: &str, expression: &str) -> LicenseVerdict {
        licenses.accepted_for(name, &accepted()).judge(expression)
    }

    #[test]
    fn a_file_of_comments_has_no_entries() {
        assert_eq!(
            parse(HEADER),
            Ok(WebLicenses::default()),
            "the generated file accepts nothing"
        );
    }

    #[test]
    fn entries_are_read_in_order() {
        let licenses = parse(
            "[[package]]\nname = \"@fontsource/inter\"\nlicenses = [\"OFL-1.1\"]\n\
             reason = \"A font.\"\n\n[[package]]\nname = \"left-pad\"\n\
             licenses = [\"MPL-2.0\", \"CC0-1.0\"]\nreason = \"Weak copyleft, unmodified.\"\n",
        )
        .unwrap();
        let names: Vec<&str> = licenses
            .packages
            .iter()
            .map(|package| package.name.as_str())
            .collect();
        assert_eq!(
            names,
            ["@fontsource/inter", "left-pad"],
            "both entries, in order"
        );
        assert_eq!(licenses.packages[1].licensees.len(), 2, "both licenses");
    }

    #[test]
    fn an_entry_accepts_its_licenses_for_its_package_alone() {
        let licenses = parse(
            "[[package]]\nname = \"@fontsource/inter\"\nlicenses = [\"OFL-1.1\"]\n\
             reason = \"A font.\"\n",
        )
        .unwrap();
        assert_eq!(
            judge(&licenses, "@fontsource/inter", "OFL-1.1"),
            LicenseVerdict::Accepted,
            "the scoped package named takes its license"
        );
        for other in [
            "inter",
            "@fontsource/roboto",
            "@fontsource/Inter",
            "@fontsource/inter ",
        ] {
            assert_eq!(
                judge(&licenses, other, "OFL-1.1"),
                LicenseVerdict::NotAccepted,
                "{other:?} isn't the package named"
            );
        }
        assert_eq!(
            judge(&licenses, "@fontsource/inter", "GPL-3.0-only"),
            LicenseVerdict::NotAccepted,
            "only the licenses the entry names"
        );
    }

    #[test]
    fn expressions_need_the_entry_and_accepted_together() {
        let licenses = parse(
            "[[package]]\nname = \"mixed\"\nlicenses = [\"OFL-1.1\", \"MPL-2.0\"]\n\
             reason = \"Fonts and code.\"\n",
        )
        .unwrap();
        for (expression, verdict) in [
            ("OFL-1.1 AND MIT", LicenseVerdict::Accepted),
            (
                "(OFL-1.1 OR GPL-3.0-only) AND ISC",
                LicenseVerdict::Accepted,
            ),
            (
                "MPL-2.0 AND (GPL-3.0-only OR Apache-2.0)",
                LicenseVerdict::Accepted,
            ),
            ("OFL-1.1 AND GPL-3.0-only", LicenseVerdict::NotAccepted),
            ("OFL-1.1 WITH LLVM-exception", LicenseVerdict::Exception),
            ("ofl-1.1", LicenseVerdict::Unreadable),
        ] {
            assert_eq!(
                judge(&licenses, "mixed", expression),
                verdict,
                "{expression:?} is {verdict:?}"
            );
            let alone = if verdict == LicenseVerdict::Accepted {
                LicenseVerdict::NotAccepted
            } else {
                verdict
            };
            assert_eq!(
                judge(&licenses, "other", expression),
                alone,
                "{expression:?} without the entry is {alone:?}"
            );
        }
    }

    #[test]
    fn malformed_files_say_what_is_wrong() {
        let entry = |body: &str| format!("[[package]]\n{body}\n");
        let full = "name = \"a\"\nlicenses = [\"OFL-1.1\"]\nreason = \"Why.\"";
        for (text, message) in [
            (
                "accepted = [\"MIT\"]\n".to_owned(),
                "unknown key `accepted`",
            ),
            ("[a]\nlicenses = [\"MIT\"]\n".to_owned(), "unknown key `a`"),
            (
                "package = \"a\"\n".to_owned(),
                "`package` should be an array of tables",
            ),
            (
                "package = [\"a\"]\n".to_owned(),
                "`package` entry 1 isn't a table",
            ),
            (
                entry(&format!("{full}\nversion = \"1\"")),
                "entry 1 has the unknown key `version`",
            ),
            (
                entry("licenses = [\"OFL-1.1\"]\nreason = \"Why.\""),
                "entry 1 has no `name`",
            ),
            (
                entry("name = \"\"\nlicenses = [\"OFL-1.1\"]\nreason = \"Why.\""),
                "has the name \"\"",
            ),
            (
                entry("name = \" a\"\nlicenses = [\"OFL-1.1\"]\nreason = \"Why.\""),
                "has the name \" a\"",
            ),
            (
                entry("name = 1\nlicenses = [\"OFL-1.1\"]\nreason = \"Why.\""),
                "has the name 1",
            ),
            (
                entry("name = \"a\"\nreason = \"Why.\""),
                "has no `licenses`",
            ),
            (
                entry("name = \"a\"\nlicenses = []\nreason = \"Why.\""),
                "has the licenses []",
            ),
            (
                entry("name = \"a\"\nlicenses = \"OFL-1.1\"\nreason = \"Why.\""),
                "has the licenses \"OFL-1.1\"",
            ),
            (
                entry("name = \"a\"\nlicenses = [\"ofl-1.1\"]\nreason = \"Why.\""),
                "has \"ofl-1.1\" in `licenses`, which isn't an SPDX license id",
            ),
            (
                entry("name = \"a\"\nlicenses = [\"MIT OR OFL-1.1\"]\nreason = \"Why.\""),
                "has \"MIT OR OFL-1.1\" in `licenses`",
            ),
            (
                entry(
                    "name = \"a\"\nlicenses = [\"Apache-2.0 WITH LLVM-exception\"]\nreason = \"Why.\"",
                ),
                "in `licenses`, which isn't an SPDX license id",
            ),
            (
                entry("name = \"a\"\nlicenses = [1]\nreason = \"Why.\""),
                "has 1 in `licenses`",
            ),
            (
                entry("name = \"a\"\nlicenses = [\"OFL-1.1\", \"OFL-1.1\"]\nreason = \"Why.\""),
                "entry 1 has \"OFL-1.1\" twice in `licenses`",
            ),
            (
                entry("name = \"a\"\nlicenses = [\"OFL-1.1\"]"),
                "entry 1 has no `reason`",
            ),
            (
                entry("name = \"a\"\nlicenses = [\"OFL-1.1\"]\nreason = \" \""),
                "has the reason \" \"",
            ),
            (
                format!("{}{}", entry(full), entry(full)),
                "entry 2 names \"a\", as an earlier entry does",
            ),
        ] {
            let error = parse(&text).expect_err(&text);
            assert!(
                error.contains(message),
                "{text:?} gives {error:?}, not {message:?}"
            );
        }
    }

    #[test]
    fn stale_entries_name_no_installed_package() {
        let directory = TempDir::new().unwrap();
        let node_modules = directory.path().join("node_modules");
        for (place, name, version) in [
            ("@fontsource/inter", "@fontsource/inter", "5.0.0"),
            (
                "a/node_modules/@fontsource/inter",
                "@fontsource/inter",
                "4.0.0",
            ),
            ("a", "a", "1.0.0"),
        ] {
            let package = node_modules.join(place);
            fs::create_dir_all(&package).unwrap();
            fs::write(
                package.join("package.json"),
                format!(
                    "{{\"name\": \"{name}\", \"version\": \"{version}\", \"license\": \"MIT\"}}"
                ),
            )
            .unwrap();
        }
        let installed = WebNotices::collect(&node_modules).unwrap();
        let licenses = parse(
            "[[package]]\nname = \"gone\"\nlicenses = [\"OFL-1.1\"]\nreason = \"Was a font.\"\n\n\
             [[package]]\nname = \"@fontsource/inter\"\nlicenses = [\"OFL-1.1\"]\n\
             reason = \"A font.\"\n\n[[package]]\nname = \"inter\"\nlicenses = [\"OFL-1.1\"]\n\
             reason = \"Unscoped, so another package.\"\n",
        )
        .unwrap();
        assert_eq!(
            licenses.stale(&installed),
            ["gone", "inter"],
            "an entry whose package is installed, in two versions, isn't stale"
        );
        assert!(
            WebLicenses::default().stale(&installed).is_empty(),
            "no entries, nothing stale"
        );
    }
}
