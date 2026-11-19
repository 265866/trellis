//! The licenses a project accepts, from its `about.toml`, and the SPDX license expressions they
//! satisfy.

use std::path::Path;

use spdx::{Expression, Licensee, ParseMode};
use toml::Value;

use crate::policy_error::PolicyError;
use crate::toml_file;

/// The SPDX licenses a project accepts: the `accepted` array of its `about.toml`, which
/// cargo-about reads for the Rust crates too, and judges with the same `spdx` crate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AcceptedLicenses {
    /// The accepted licenses, such as `MIT`.
    licensees: Vec<Licensee>,
}

/// Whether a project's [`AcceptedLicenses`] accept a package's license expression.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LicenseVerdict {
    /// The expression is satisfied by accepted licenses alone.
    Accepted,
    /// The expression reads as SPDX license ids joined with `AND` and `OR`, but needs a license
    /// that isn't accepted.
    NotAccepted,
    /// The expression adds an exception to a license with `WITH`, such as
    /// `Apache-2.0 WITH LLVM-exception`, which the check doesn't support.
    Exception,
    /// The expression isn't one the check reads, so no accepted license can satisfy it:
    /// `UNLICENSED`, `SEE LICENSE IN ...`, or anything else that doesn't parse as a strict SPDX
    /// expression.
    Unreadable,
}

impl AcceptedLicenses {
    /// Reads the `accepted` array of the `about.toml` at `path`, ignoring its other keys.
    ///
    /// # Errors
    ///
    /// Fails if the file can't be read, isn't TOML, or has no `accepted` array of SPDX license
    /// ids.
    pub fn read(path: &Path) -> Result<Self, PolicyError> {
        let table = toml_file::read(path)?;
        let malformed = || PolicyError::Malformed {
            path: path.to_owned(),
            message: "`accepted` should be an array of SPDX license ids, such as [\"MIT\"]"
                .to_owned(),
        };
        let Some(Value::Array(values)) = table.get("accepted") else {
            return Err(malformed());
        };
        let mut licensees = Vec::new();
        for value in values {
            let id = value.as_str().ok_or_else(malformed)?;
            licensees.push(Licensee::parse(id).map_err(|_| malformed())?);
        }
        Ok(Self { licensees })
    }

    /// Judges the SPDX license expression `expression`.
    ///
    /// The expression is parsed in the `spdx` crate's strict mode, which fails closed: license
    /// ids are case-sensitive and must be current SPDX ids, so `mit`, `GPL-2.0`, `BSD`,
    /// `Apache 2.0` and `MIT/ISC` don't parse, while `(MIT OR Apache-2.0)` does. Then, as
    /// cargo-about judges a crate, it is accepted when accepted licenses alone satisfy it, an id
    /// with a trailing `+` satisfied by that version or a later one. An expression with a `WITH`
    /// exception anywhere is [`LicenseVerdict::Exception`], and one that doesn't parse is
    /// [`LicenseVerdict::Unreadable`].
    #[must_use]
    pub fn judge(&self, expression: &str) -> LicenseVerdict {
        let Ok(parsed) = Expression::parse_mode(expression, ParseMode::STRICT) else {
            return LicenseVerdict::Unreadable;
        };
        if parsed
            .requirements()
            .any(|requirement| requirement.req.addition.is_some())
        {
            return LicenseVerdict::Exception;
        }
        let accepted = parsed.evaluate(|requirement| {
            self.licensees
                .iter()
                .any(|licensee| licensee.satisfies(requirement))
        });
        if accepted {
            LicenseVerdict::Accepted
        } else {
            LicenseVerdict::NotAccepted
        }
    }

    /// Judges the alternatives of an npm `licenses` array, each an SPDX license expression of its
    /// own, as [`judge`](Self::judge) judges each.
    ///
    /// An alternative that doesn't parse makes the whole [`LicenseVerdict::Unreadable`], and
    /// otherwise one with an exception makes it [`LicenseVerdict::Exception`], as they would in
    /// one expression. Otherwise the alternatives are accepted when any one is. No alternatives
    /// are [`LicenseVerdict::Unreadable`].
    #[must_use]
    pub fn judge_any(&self, alternatives: &[String]) -> LicenseVerdict {
        let mut verdicts = Vec::new();
        for alternative in alternatives {
            verdicts.push(self.judge(alternative));
        }
        if verdicts.is_empty() || verdicts.contains(&LicenseVerdict::Unreadable) {
            LicenseVerdict::Unreadable
        } else if verdicts.contains(&LicenseVerdict::Exception) {
            LicenseVerdict::Exception
        } else if verdicts.contains(&LicenseVerdict::Accepted) {
            LicenseVerdict::Accepted
        } else {
            LicenseVerdict::NotAccepted
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn accepted() -> AcceptedLicenses {
        AcceptedLicenses {
            licensees: ["Apache-2.0", "BSD-3-Clause", "ISC", "MIT"]
                .map(|id| Licensee::parse(id).unwrap())
                .to_vec(),
        }
    }

    fn assert_verdicts(expressions: &[&str], verdict: LicenseVerdict) {
        for expression in expressions {
            assert_eq!(
                accepted().judge(expression),
                verdict,
                "{expression:?} is {verdict:?}"
            );
        }
    }

    #[test]
    fn expressions_of_accepted_licenses_are_accepted() {
        assert_verdicts(
            &[
                "MIT",
                " ISC ",
                "MIT OR GPL-3.0-only",
                "GPL-3.0-only OR MIT",
                "MIT AND ISC",
                "(MIT AND ISC)",
                "((MIT))",
                "(MIT OR Apache-2.0)",
                "(MIT OR GPL-3.0-only) AND (ISC OR MPL-2.0)",
                "GPL-3.0-only AND MPL-2.0 OR MIT",
                "MIT OR GPL-3.0-only AND MPL-2.0",
                "Apache-2.0+",
                "(Apache-2.0+ OR GPL-2.0-only)AND MIT",
                "MIT or ISC",
            ],
            LicenseVerdict::Accepted,
        );
    }

    #[test]
    fn expressions_that_need_another_license_are_not_accepted() {
        assert_verdicts(
            &[
                "GPL-3.0-only",
                "MIT AND GPL-3.0-only",
                "GPL-3.0-only AND MIT OR MPL-2.0",
                "MIT AND (GPL-3.0-only OR MPL-2.0)",
                "GPL-2.0-or-later",
                "LicenseRef-proprietary",
            ],
            LicenseVerdict::NotAccepted,
        );
    }

    #[test]
    fn expressions_with_an_exception_are_exceptions() {
        assert_verdicts(
            &[
                "Apache-2.0 WITH LLVM-exception",
                "MIT OR (Apache-2.0 WITH LLVM-exception)",
                "GPL-2.0-only WITH Classpath-exception-2.0 AND MIT",
            ],
            LicenseVerdict::Exception,
        );
    }

    #[test]
    fn expressions_the_check_cant_read_fail_closed() {
        assert_verdicts(
            &[
                "",
                "   ",
                "mit",
                "Mit",
                "MIT OR isc",
                "MIT/ISC",
                "MIT, ISC",
                "GPL-2.0",
                "GPL-2.0+",
                "BSD",
                "Apache 2.0",
                "UNLICENSED",
                "MIT OR UNLICENSED",
                "SEE LICENSE IN LICENSE.md",
                "MIT WITH",
                "MIT WITH Unknown-exception",
                "MIT OR",
                "OR MIT",
                "MIT AND",
                "MIT ISC",
                "(MIT",
                "MIT)",
                "()",
                "+",
                "MIT++",
                "AND",
            ],
            LicenseVerdict::Unreadable,
        );
    }

    #[test]
    fn deep_nesting_is_judged_without_overflowing() {
        let deep = format!("{}MIT{}", "(".repeat(50_000), ")".repeat(50_000));
        assert_eq!(
            accepted().judge(&deep),
            LicenseVerdict::Accepted,
            "50,000 levels of parentheses parse without recursion"
        );
        assert_eq!(
            accepted().judge(&"(".repeat(50_000)),
            LicenseVerdict::Unreadable,
            "50,000 open parentheses don't parse"
        );
    }

    #[test]
    fn alternatives_are_judged_each_on_its_own() {
        let judge = |alternatives: &[&str]| {
            let alternatives: Vec<String> = alternatives
                .iter()
                .map(|alternative| (*alternative).to_owned())
                .collect();
            accepted().judge_any(&alternatives)
        };
        for (alternatives, verdict) in [
            (&["MIT"][..], LicenseVerdict::Accepted),
            (&["GPL-3.0-only", "MIT"], LicenseVerdict::Accepted),
            (&["GPL-3.0-only", "MPL-2.0"], LicenseVerdict::NotAccepted),
            (&["(GPL-3.0-only", "MIT)"], LicenseVerdict::Unreadable),
            (&["MIT", ""], LicenseVerdict::Unreadable),
            (
                &["MIT", "Apache-2.0 WITH LLVM-exception"],
                LicenseVerdict::Exception,
            ),
            (&[], LicenseVerdict::Unreadable),
        ] {
            assert_eq!(
                judge(alternatives),
                verdict,
                "{alternatives:?} is {verdict:?}"
            );
        }
    }
}
