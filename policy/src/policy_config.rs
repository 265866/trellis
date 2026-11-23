//! The settings of the checks, in the root `Cargo.toml`.

use std::collections::BTreeMap;

use toml::{Table, Value};

use crate::interface::Interface;
use crate::policy_error::PolicyError;
use crate::restricted_crate::RestrictedCrate;
use crate::toml_file;

/// The settings of the checks, from the `[workspace.metadata.policy]` table of the root
/// `Cargo.toml`. A missing key takes the strictest setting: it allows nothing, or, for the keys
/// that say what the docs layout requires, it is a violation until they are set. A key the checks
/// don't know is an error, so a misspelled setting can't leave a check on its default.
#[derive(Debug, Default)]
pub(crate) struct PolicyConfig {
    /// The crates allowed a build script (`build_scripts`).
    build_script_crates: Vec<String>,
    /// The proc-macro packages members may depend on directly (`proc_macros`).
    proc_macro_crates: Vec<String>,
    /// Each workspace crate, mapped to the workspace crates it may depend on
    /// (`internal_dependencies`).
    internal_dependencies: BTreeMap<String, Vec<String>>,
    /// The test-support crates members may depend on only as dev-dependencies
    /// (`internal_dependencies`).
    dev_only_crates: Vec<String>,
    /// The docs files that must exist, relative to the root, or `None` if the key is missing
    /// (`docs_layout`).
    docs_files: Option<Vec<String>>,
    /// The level-2 headings of the root `README.md`, in order, or `None` if the key is missing
    /// (`docs_layout`).
    readme_sections: Option<Vec<String>>,
    /// What a person who uses the project sees, the primary interface first, or `None` if the key
    /// is missing (`readme`).
    interfaces: Option<Vec<Interface>>,
    /// Each restricted package, by its key, mapped to its pattern and the members that may reach
    /// it (`restricted_crates`).
    restricted_crates: BTreeMap<String, RestrictedCrate>,
}

/// The keys of `[workspace.metadata.policy]`.
const KEYS: [&str; 8] = [
    "build_script_crates",
    "proc_macro_crates",
    "internal_dependencies",
    "dev_only_crates",
    "docs_files",
    "readme_sections",
    "interfaces",
    "restricted_crates",
];

impl PolicyConfig {
    /// Reads the settings from the root manifest `manifest`.
    pub(crate) fn from_manifest(manifest: &Table) -> Result<Self, PolicyError> {
        let mut config = Self::default();
        let Some(value) = toml_file::lookup(manifest, &["workspace", "metadata", "policy"]) else {
            return Ok(config);
        };
        let Value::Table(table) = value else {
            return Err(PolicyError::Config("it isn't a table".to_owned()));
        };
        for (key, value) in table {
            match key.as_str() {
                "build_script_crates" => config.build_script_crates = crate_names(key, value)?,
                "proc_macro_crates" => config.proc_macro_crates = crate_names(key, value)?,
                "internal_dependencies" => {
                    config.internal_dependencies = dependency_map(key, value)?;
                }
                "dev_only_crates" => config.dev_only_crates = crate_names(key, value)?,
                "docs_files" => config.docs_files = Some(relative_paths(key, value)?),
                "readme_sections" => {
                    config.readme_sections = Some(strings(key, value, "headings")?);
                }
                "interfaces" => config.interfaces = Some(interfaces(key, value)?),
                "restricted_crates" => config.restricted_crates = restricted_crates(key, value)?,
                _ => {
                    return Err(PolicyError::Config(format!(
                        "it has a key `{key}` the checks don't know; the keys are {}",
                        KEYS.map(|known| format!("`{known}`")).join(", ")
                    )));
                }
            }
        }
        config.check_internal_names()?;
        config.check_restricted_names()?;
        Ok(config)
    }

    /// The crates allowed a build script.
    pub(crate) fn build_script_crates(&self) -> &[String] {
        &self.build_script_crates
    }

    /// The proc-macro packages members may depend on directly.
    pub(crate) fn proc_macro_crates(&self) -> &[String] {
        &self.proc_macro_crates
    }

    /// The workspace crates that the workspace crate `name` may depend on, or `None` if
    /// `internal_dependencies` doesn't map it.
    pub(crate) fn internal_dependencies(&self, name: &str) -> Option<&[String]> {
        self.internal_dependencies.get(name).map(Vec::as_slice)
    }

    /// The docs files that must exist, relative to the root, or `None` if `docs_files` isn't
    /// set.
    pub(crate) fn docs_files(&self) -> Option<&[String]> {
        self.docs_files.as_deref()
    }

    /// The level-2 headings the root `README.md` has, in order, or `None` if `readme_sections`
    /// isn't set.
    pub(crate) fn readme_sections(&self) -> Option<&[String]> {
        self.readme_sections.as_deref()
    }

    /// What a person who uses the project sees, the primary interface first, or `None` if
    /// `interfaces` isn't set.
    pub(crate) fn interfaces(&self) -> Option<&[Interface]> {
        self.interfaces.as_deref()
    }

    /// Each restricted package's key, mapped to its pattern and the members that may reach it.
    pub(crate) fn restricted_crates(&self) -> &BTreeMap<String, RestrictedCrate> {
        &self.restricted_crates
    }

    /// Fails if a list in `restricted_crates` names a crate that isn't a key of
    /// `internal_dependencies`, which catches a misspelled name.
    fn check_restricted_names(&self) -> Result<(), PolicyError> {
        for (key, entry) in &self.restricted_crates {
            if let Some(name) = entry
                .allowed()
                .iter()
                .find(|name| !self.internal_dependencies.contains_key(*name))
            {
                return Err(PolicyError::Config(format!(
                    "the list `restricted_crates.\"{key}\"` names `{name}`, which isn't a key of \
                     `internal_dependencies`; fix the spelling, or add `{name}` to \
                     `internal_dependencies`"
                )));
            }
        }
        Ok(())
    }

    /// Whether `name` is a test-support crate that members may depend on only as a
    /// dev-dependency.
    pub(crate) fn is_dev_only(&self, name: &str) -> bool {
        self.dev_only_crates
            .iter()
            .any(|crate_name| crate_name == name)
    }

    /// Fails if a crate that `internal_dependencies` lets another depend on, or a crate in
    /// `dev_only_crates`, isn't itself a key of `internal_dependencies`, which catches a
    /// misspelled name. It also fails if a list in `internal_dependencies` names a dev-only crate,
    /// which members reach only through the dev-dependency rule.
    fn check_internal_names(&self) -> Result<(), PolicyError> {
        for name in &self.dev_only_crates {
            if !self.internal_dependencies.contains_key(name) {
                return Err(PolicyError::Config(format!(
                    "`dev_only_crates` lists `{name}`, which isn't a key of \
                     `internal_dependencies`; fix the spelling, or add `{name}` to \
                     `internal_dependencies` with the workspace crates it may depend on"
                )));
            }
        }
        for (crate_name, dependencies) in &self.internal_dependencies {
            for name in dependencies {
                if !self.internal_dependencies.contains_key(name) {
                    return Err(PolicyError::Config(format!(
                        "the list `internal_dependencies.{crate_name}` names `{name}`, which \
                         isn't a key of `internal_dependencies`; fix the spelling, or add \
                         `{name}` to `internal_dependencies` with the workspace crates it may \
                         depend on"
                    )));
                }
                if self.is_dev_only(name) {
                    return Err(PolicyError::Config(format!(
                        "the list `internal_dependencies.{crate_name}` names `{name}`, which \
                         `dev_only_crates` lists; remove it from the list, since every crate may \
                         depend on a dev-only crate as a dev-dependency and none may otherwise"
                    )));
                }
            }
        }
        Ok(())
    }
}

/// The crate names in `value`, the setting `key`, which must be an array of strings.
fn crate_names(key: &str, value: &Value) -> Result<Vec<String>, PolicyError> {
    strings(key, value, "crate names")
}

/// The strings in `value`, the setting `key`, which must be an array of `what`.
fn strings(key: &str, value: &Value, what: &str) -> Result<Vec<String>, PolicyError> {
    let not_strings = || PolicyError::Config(format!("`{key}` must be an array of {what}"));
    let array = value.as_array().ok_or_else(not_strings)?;
    let mut strings = Vec::with_capacity(array.len());
    for item in array {
        strings.push(item.as_str().ok_or_else(not_strings)?.to_owned());
    }
    Ok(strings)
}

/// The paths in `value`, the setting `key`, which must be an array of file paths relative to the
/// root, with `/` separators and no `.` or `..` parts, so each names one file in the repository.
fn relative_paths(key: &str, value: &Value) -> Result<Vec<String>, PolicyError> {
    let paths = strings(key, value, "file paths")?;
    for path in &paths {
        let relative =
            path.split('/').all(|part| !matches!(part, "" | "." | "..")) && !path.contains('\\');
        if !relative {
            return Err(PolicyError::Config(format!(
                "`{key}` lists `{path}`, which isn't a file path relative to the root; write \
                 it with `/` separators and no leading `/`, `.` or `..` parts, such as \
                 `docs/install.md`"
            )));
        }
    }
    Ok(paths)
}

/// The interfaces in `value`, the setting `key`, which must be a non-empty array of the names
/// [`Interface::ALL`] lists, each at most once.
fn interfaces(key: &str, value: &Value) -> Result<Vec<Interface>, PolicyError> {
    let allowed = || {
        Interface::ALL
            .map(|(name, _)| format!("`{name}`"))
            .join(", ")
    };
    let names = strings(key, value, "interface names")?;
    if names.is_empty() {
        return Err(PolicyError::Config(format!(
            "`{key}` is empty; list what a person who uses the project sees, the primary one \
             first, from {}",
            allowed()
        )));
    }
    let mut interfaces = Vec::with_capacity(names.len());
    for name in &names {
        let Some(interface) = Interface::parse(name) else {
            return Err(PolicyError::Config(format!(
                "`{key}` lists `{name}`, which isn't an interface; use {}",
                allowed()
            )));
        };
        if interfaces.contains(&interface) {
            return Err(PolicyError::Config(format!(
                "`{key}` lists `{name}` twice; list each interface once"
            )));
        }
        interfaces.push(interface);
    }
    Ok(interfaces)
}

/// The map in `value`, the setting `key`: a table from a package name, a name prefix ending in
/// `*`, or a source starting with `git+`, `registry+` or `sparse+`, to the arrays of members that
/// may reach it.
fn restricted_crates(
    key: &str,
    value: &Value,
) -> Result<BTreeMap<String, RestrictedCrate>, PolicyError> {
    let Value::Table(table) = value else {
        return Err(PolicyError::Config(format!(
            "`{key}` must be a table from package names, name prefixes ending in `*` or sources \
             to arrays of member names"
        )));
    };
    let mut map = BTreeMap::new();
    for (pattern, allowed) in table {
        let allowed = strings(&format!("{key}.\"{pattern}\""), allowed, "member names")?;
        let Some(entry) = RestrictedCrate::new(pattern, allowed) else {
            return Err(PolicyError::Config(format!(
                "`{key}` has a key `{pattern}` that names no package; write a package name, such \
                 as `gpui`, a name prefix ending in `*`, such as `gpui-*`, or a source starting \
                 with `git+`, `registry+` or `sparse+`, such as \
                 `git+https://github.com/zed-industries/zed`"
            )));
        };
        map.insert(pattern.clone(), entry);
    }
    Ok(map)
}

/// The map in `value`, the setting `key`, which must be a table from crate names to arrays of
/// crate names.
fn dependency_map(key: &str, value: &Value) -> Result<BTreeMap<String, Vec<String>>, PolicyError> {
    let Value::Table(table) = value else {
        return Err(PolicyError::Config(format!(
            "`{key}` must be a table from crate names to arrays of crate names"
        )));
    };
    let mut map = BTreeMap::new();
    for (crate_name, dependencies) in table {
        map.insert(
            crate_name.clone(),
            crate_names(&format!("{key}.{crate_name}"), dependencies)?,
        );
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(text: &str) -> Result<PolicyConfig, PolicyError> {
        PolicyConfig::from_manifest(&toml_file::parse(text).unwrap())
    }

    #[test]
    fn a_missing_table_allows_no_build_scripts_or_proc_macros() {
        let config = config("[workspace]\n").unwrap();
        assert!(
            config.build_script_crates().is_empty(),
            "no table should allow no build scripts"
        );
        assert!(
            config.proc_macro_crates().is_empty(),
            "no table should allow no proc macros"
        );
    }

    #[test]
    fn build_script_and_proc_macro_crates_are_read() {
        let config = config(
            "[workspace.metadata.policy]\nbuild_script_crates = [\"a\"]\n\
             proc_macro_crates = [\"b\", \"c\"]\n",
        )
        .unwrap();
        assert_eq!(
            config.build_script_crates(),
            ["a".to_owned()],
            "the listed crate should be allowed a build script"
        );
        assert_eq!(
            config.proc_macro_crates(),
            ["b".to_owned(), "c".to_owned()],
            "the listed proc macros should be allowed"
        );
    }

    #[test]
    fn an_unknown_key_fails() {
        let error = config("[workspace.metadata.policy]\nbuild_script_crate = []\n").unwrap_err();
        assert!(
            error.to_string().contains("`build_script_crate`"),
            "a misspelled key should be named in the error, not ignored: {error}"
        );
    }

    #[test]
    fn internal_dependencies_and_dev_only_crates_are_read() {
        let config = config(
            "[workspace.metadata.policy]\ndev_only_crates = [\"testing\"]\n\n\
             [workspace.metadata.policy.internal_dependencies]\ncore = []\n\
             app = [\"core\"]\ntesting = [\"core\"]\n",
        )
        .unwrap();
        assert_eq!(
            config.internal_dependencies("app"),
            Some(["core".to_owned()].as_slice()),
            "the listed dependencies should be allowed"
        );
        assert_eq!(
            config.internal_dependencies("core"),
            Some([].as_slice()),
            "an empty list maps the crate and allows it nothing"
        );
        assert_eq!(
            config.internal_dependencies("other"),
            None,
            "a crate the table leaves out isn't mapped"
        );
        assert!(
            config.is_dev_only("testing"),
            "the listed crate should be dev-only"
        );
        assert!(
            !config.is_dev_only("core"),
            "an unlisted crate isn't dev-only"
        );
    }

    #[test]
    fn a_missing_internal_dependencies_table_maps_no_crate() {
        let config = config("[workspace.metadata.policy]\n").unwrap();
        assert_eq!(
            config.internal_dependencies("app"),
            None,
            "no table should map no crate, so every member fails"
        );
    }

    #[test]
    fn an_internal_dependency_the_map_doesnt_name_fails() {
        let error = config(
            "[workspace.metadata.policy.internal_dependencies]\napp = [\"cor\"]\ncore = []\n",
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("the list `internal_dependencies.app` names `cor`"),
            "the error should name the list and the misspelled dependency: {error}"
        );
        assert!(
            error.to_string().contains("fix the spelling, or add `cor`"),
            "the error should say how to fix it: {error}"
        );
    }

    #[test]
    fn a_dev_only_crate_outside_the_map_or_inside_a_list_fails() {
        let error = config(
            "[workspace.metadata.policy]\ndev_only_crates = [\"testing\"]\n\n\
             [workspace.metadata.policy.internal_dependencies]\napp = []\n",
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("`dev_only_crates` lists `testing`"),
            "a dev-only crate the map leaves out should fail: {error}"
        );
        let error = config(
            "[workspace.metadata.policy]\ndev_only_crates = [\"testing\"]\n\n\
             [workspace.metadata.policy.internal_dependencies]\napp = [\"testing\"]\n\
             testing = []\n",
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("which `dev_only_crates` lists"),
            "a dev-only crate in a list of dependencies should fail: {error}"
        );
    }

    #[test]
    fn docs_files_and_readme_sections_are_read_and_stay_unset_when_missing() {
        let set = config(
            "[workspace.metadata.policy]\ndocs_files = [\"README.md\", \"docs/design.md\"]\n\
             readme_sections = [\"Install\", \"Documentation\"]\n",
        )
        .unwrap();
        assert_eq!(
            set.docs_files(),
            Some(["README.md".to_owned(), "docs/design.md".to_owned()].as_slice()),
            "the listed files should be read"
        );
        assert_eq!(
            set.readme_sections(),
            Some(["Install".to_owned(), "Documentation".to_owned()].as_slice()),
            "the listed sections should be read in order"
        );
        let unset = config("[workspace.metadata.policy]\n").unwrap();
        assert_eq!(
            unset.docs_files(),
            None,
            "a missing key stays unset, so the rule can ask for it"
        );
        assert_eq!(
            unset.readme_sections(),
            None,
            "a missing key stays unset, so the rule can ask for it"
        );
    }

    #[test]
    fn a_docs_file_outside_the_root_fails() {
        for path in [
            "/etc/passwd",
            "../README.md",
            "docs/../x.md",
            "./README.md",
            "docs\\a.md",
        ] {
            let error = config(&format!(
                "[workspace.metadata.policy]\ndocs_files = [{path:?}]\n"
            ))
            .unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("which isn't a file path relative to the root"),
                "{path} should fail: {error}"
            );
        }
        let error =
            config("[workspace.metadata.policy]\nreadme_sections = \"Install\"\n").unwrap_err();
        assert!(
            error
                .to_string()
                .contains("`readme_sections` must be an array of headings"),
            "a string where a list belongs should fail: {error}"
        );
    }

    #[test]
    fn interfaces_are_read_in_order_and_stay_unset_when_missing() {
        let set = config("[workspace.metadata.policy]\ninterfaces = [\"web\", \"cli\"]\n").unwrap();
        assert_eq!(
            set.interfaces(),
            Some([Interface::Web, Interface::Cli].as_slice()),
            "the listed interfaces should be read, the primary one first"
        );
        let unset = config("[workspace.metadata.policy]\n").unwrap();
        assert_eq!(
            unset.interfaces(),
            None,
            "a missing key stays unset, so a rule can ask for it"
        );
    }

    #[test]
    fn an_unknown_empty_or_repeated_interface_fails() {
        let error = config("[workspace.metadata.policy]\ninterfaces = [\"tui\"]\n").unwrap_err();
        assert!(
            error.to_string().contains(
                "`interfaces` lists `tui`, which isn't an interface; use `web`, `desktop`, \
                 `terminal`, `cli`, `service`, `library`"
            ),
            "an unknown interface should fail and name the allowed ones: {error}"
        );
        let error = config("[workspace.metadata.policy]\ninterfaces = []\n").unwrap_err();
        assert!(
            error.to_string().contains("`interfaces` is empty"),
            "an empty list should fail: {error}"
        );
        let error =
            config("[workspace.metadata.policy]\ninterfaces = [\"web\", \"web\"]\n").unwrap_err();
        assert!(
            error.to_string().contains("lists `web` twice"),
            "a repeated interface should fail: {error}"
        );
        let error = config("[workspace.metadata.policy]\ninterfaces = \"web\"\n").unwrap_err();
        assert!(
            error
                .to_string()
                .contains("`interfaces` must be an array of interface names"),
            "a string where a list belongs should fail: {error}"
        );
    }

    #[test]
    fn a_name_that_isnt_a_string_fails() {
        let error = config("[workspace.metadata.policy]\nbuild_script_crates = [1]\n").unwrap_err();
        assert!(
            error.to_string().contains("array of crate names"),
            "a number in the list should fail: {error}"
        );
    }
}
