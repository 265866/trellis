//! The npm packages installed under a `node_modules` directory, and the license notices that list
//! them.

use std::collections::HashSet;
use std::fmt::{self, Display, Formatter};
use std::fs;
use std::path::Path;

use crate::npm_package::{NpmPackage, NpmPackageDirectory};
use crate::policy_error::PolicyError;
use crate::unidentified_package::UnidentifiedPackage;

/// The first line of the notices.
const HEADING: &str = "Licenses of the installed npm packages, development and transitive \
                       dependencies included";

/// The line that starts each package's entry in the notices.
const SEPARATOR: &str = "================================================================\
                         ================";

/// The npm packages installed under a `node_modules` directory, sorted by name then version. Its
/// [`Display`] writes their license notices.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WebNotices {
    /// The packages, sorted by name then version. Copies of a package with the same license,
    /// people and license texts appear once; copies that differ in any of them each appear.
    packages: Vec<NpmPackage>,
    /// The places where an installer puts a package that hold something the check can't name,
    /// sorted.
    unidentified: Vec<UnidentifiedPackage>,
}

/// An entry of a directory that the walk looks at.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    /// The entry's name.
    name: String,
    /// What the entry is.
    kind: EntryKind,
}

/// What an [`Entry`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EntryKind {
    /// A directory.
    Directory,
    /// A symbolic link, to anything.
    Link,
}

/// What a directory under `node_modules` is to the directories in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Parent {
    /// A `node_modules` directory: each directory in it is a package, or a scope.
    NodeModules,
    /// A scope such as `@scope` in a `node_modules` directory: each directory in it is a package.
    Scope,
    /// Any other directory.
    Other,
}

impl WebNotices {
    /// Finds every package installed under `node_modules`, without following symbolic links.
    ///
    /// A package's place is `node_modules/<name>` or `node_modules/@scope/<name>`, at any depth,
    /// where `<name>` doesn't start with `.`, as no npm package's name does. A directory there
    /// that holds a `package.json` is a package, named by the `name` that file states. That covers
    /// Bun's hoisted layout, with packages nested in other packages' `node_modules`, and its
    /// isolated one, which installs each package in
    /// `node_modules/.bun/<name>@<version>/node_modules/<name>` and links it into the
    /// `node_modules` that use it. A link in a package's place counts only when it leads to
    /// another package's place the walk reaches, where the package is read; a link to anywhere
    /// else, such as a `link:` or `file:` dependency outside `node_modules`, is unidentified, and
    /// so is a directory that holds files but no `package.json`. An empty directory is skipped,
    /// and so are `.bin` and `.cache` in a `node_modules`.
    ///
    /// # Errors
    ///
    /// Fails if a directory or file can't be read, if a directory's name isn't UTF-8, or if a
    /// package's `package.json` isn't JSON.
    pub fn collect(node_modules: &Path) -> Result<Self, PolicyError> {
        let root = fs::canonicalize(node_modules).map_err(|source| PolicyError::Read {
            path: node_modules.to_owned(),
            source,
        })?;
        let mut packages = Vec::new();
        let mut unidentified = Vec::new();
        let mut places = HashSet::new();
        let mut links = Vec::new();
        let mut directories = vec![(node_modules.to_owned(), Parent::NodeModules)];
        while let Some((directory, parent)) = directories.pop() {
            for Entry { name, kind } in sorted_entries(&directory)? {
                let path = directory.join(&name);
                let is_place = match (parent, name.as_str()) {
                    (Parent::NodeModules, ".bin" | ".cache") => continue,
                    (Parent::NodeModules | Parent::Scope, hidden) if hidden.starts_with('.') => {
                        false
                    }
                    (Parent::NodeModules, scope)
                        if scope.starts_with('@') && kind == EntryKind::Directory =>
                    {
                        directories.push((path, Parent::Scope));
                        continue;
                    }
                    (Parent::NodeModules | Parent::Scope, _) => true,
                    (Parent::Other, _) => false,
                };
                if kind == EntryKind::Link {
                    if is_place {
                        links.push(path);
                    }
                    continue;
                }
                if is_place {
                    if let Ok(relative) = path.strip_prefix(node_modules) {
                        places.insert(root.join(relative));
                    }
                    match NpmPackage::read(&path, &root)? {
                        NpmPackageDirectory::Empty => {}
                        NpmPackageDirectory::Unidentified(problem) => {
                            unidentified.push(UnidentifiedPackage::new(path.clone(), problem));
                        }
                        NpmPackageDirectory::Package(package) => packages.push(package),
                    }
                }
                let kind = if name == "node_modules" {
                    Parent::NodeModules
                } else {
                    Parent::Other
                };
                directories.push((path, kind));
            }
        }
        for link in links {
            let problem = match fs::canonicalize(&link) {
                Ok(target) if places.contains(&target) => continue,
                Ok(target) => format!(
                    "is a link to {}, which isn't a package's place under {} that the check reads",
                    target.display(),
                    node_modules.display()
                ),
                Err(error) => format!("is a link the check can't follow: {error}"),
            };
            unidentified.push(UnidentifiedPackage::new(link, problem));
        }
        packages.sort();
        packages.dedup_by(|later, earlier| later.has_same_contents(earlier));
        unidentified.sort();
        Ok(Self {
            packages,
            unidentified,
        })
    }

    /// The packages, sorted by name then version.
    #[must_use]
    pub fn packages(&self) -> &[NpmPackage] {
        &self.packages
    }

    /// The places where an installer puts a package that hold something the check can't name,
    /// sorted. Their licenses can't be checked.
    #[must_use]
    pub fn unidentified(&self) -> &[UnidentifiedPackage] {
        &self.unidentified
    }
}

impl Display for WebNotices {
    /// Writes the notices: a heading line, then for each package a separator line, a line with
    /// its name, version and license expression, and the text of each of its license files. A
    /// package without one gets a line saying so, which names its license's standard text, and
    /// lines naming its author, contributors and maintainers, or one saying it names none. Each
    /// part follows a blank line.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        writeln!(formatter, "{HEADING}")?;
        for package in &self.packages {
            let name = package.name();
            let version = package.version();
            let license = package
                .license()
                .unwrap_or_else(|| "no license stated".to_owned());
            writeln!(formatter, "\n{SEPARATOR}\n{name} {version}: {license}")?;
            if package.license_texts().is_empty() {
                writeln!(
                    formatter,
                    "\n{name} {version} ships no license file. Its license is the standard text \
                     of {license}."
                )?;
                if let Some(author) = package.author() {
                    writeln!(formatter, "Author: {author}")?;
                }
                for contributor in package.contributors() {
                    writeln!(formatter, "Contributor: {contributor}")?;
                }
                for maintainer in package.maintainers() {
                    writeln!(formatter, "Maintainer: {maintainer}")?;
                }
                if package.author().is_none()
                    && package.contributors().is_empty()
                    && package.maintainers().is_empty()
                {
                    writeln!(formatter, "It states no copyright holder.")?;
                }
            }
            for text in package.license_texts() {
                writeln!(formatter, "\n{}", text.trim_end())?;
            }
        }
        Ok(())
    }
}

/// The directories and symbolic links in `directory`, sorted by name.
///
/// # Errors
///
/// Fails if `directory` can't be read, or if the name of a directory or link in it isn't UTF-8.
fn sorted_entries(directory: &Path) -> Result<Vec<Entry>, PolicyError> {
    let read_error = |source| PolicyError::Read {
        path: directory.to_owned(),
        source,
    };
    let mut entries = Vec::new();
    for entry in fs::read_dir(directory).map_err(read_error)? {
        let entry = entry.map_err(read_error)?;
        let file_type = entry.file_type().map_err(read_error)?;
        let kind = if file_type.is_dir() {
            EntryKind::Directory
        } else if file_type.is_symlink() {
            EntryKind::Link
        } else {
            continue;
        };
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| PolicyError::Malformed {
                path: entry.path(),
                message: "the name isn't UTF-8, so the packages in it can't be named".to_owned(),
            })?;
        entries.push(Entry { name, kind });
    }
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(entries)
}
