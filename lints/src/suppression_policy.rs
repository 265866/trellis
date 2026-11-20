use rustc_ast::visit::{self, Visitor};
use rustc_ast::{AttrStyle, Attribute, Crate};
use rustc_lint::{EarlyContext, EarlyLintPass, LintContext};
use rustc_lint_defs::{declare_lint, declare_lint_pass};
use rustc_span::Span;

use crate::attribute_origin;
use crate::command_line_lint::span_lint_from_command_line;
use crate::local_source::{Assembled, LocalSource, Meta, MetaKind, Target};

declare_lint! {
    /// Checks every lint-level attribute in the crate root and its module files, including those in
    /// `cfg_attr`, and flags:
    ///
    /// - `allow` and `warn`, outer or inner. `allow`, unlike `expect`, keeps silencing a lint
    ///   after the code it was written for is gone, and `warn` resets a level the workspace set
    ///   higher. `deny` and `forbid` only tighten levels, so they pass.
    /// - Inner attributes, `#![expect(..)]`, `#![allow(..)]` and `#![warn(..)]`, directly or
    ///   through `#![cfg_attr(..)]`. They change a lint for a whole crate or module; a lint the
    ///   whole crate turns off belongs in `[workspace.lints]` in `Cargo.toml`.
    /// - A `cfg_attr` around a lint-level attribute whose predicate is anything but
    ///   `dylint_lib = "trellis_lints"`. clippy runs without `dylint_lib` and the custom lints
    ///   run with it, so an attribute active in only one run is never checked by the other: a
    ///   `cfg_attr(not(dylint_lib = "trellis_lints"), allow(..))` hides an `allow` from these
    ///   lints, and `cfg_attr(test, ..)` silences a lint in only some builds. The one other
    ///   predicate allowed is exactly `not(test)` around an `expect` of `dead_code` alone, for
    ///   private code that only its tests use until its first caller lands. A plain
    ///   `expect(dead_code, ..)` goes unmet in the test build, where the tests use the code; this
    ///   one checks `dead_code` fully in tests, and fails in the other builds once the caller
    ///   lands, as any `expect` does.
    /// - `expect` without a non-empty `reason = "..."`, which review needs to judge the exception.
    /// - `expect` whose reason has fewer than 20 characters, or fewer than 4 different words
    ///   besides the words of the lints it names, so a reason can't be `"x"` or only the lint's
    ///   name, such as `"needless pass by value"` for `clippy::needless_pass_by_value`. A word is
    ///   a run of letters and digits, compared ignoring case. Whether the reason holds up is
    ///   left to review.
    /// - `expect` of a lint group, such as `warnings`, `unused` or `clippy::pedantic`, which
    ///   silences lints nobody named.
    /// - `expect`, `allow` or `warn` of `code_comment`, `disabled_code`,
    ///   `missing_workspace_lints` or `suppression_policy`. These lints guard the checks
    ///   themselves and have no exceptions; they report from the crate root, so an `expect` on an
    ///   item can't reach them anyway, and a crate-wide one would silence every report.
    /// - `expect`, `allow` or `warn` of `file_named_after_item` or `non_leaf_module_items`, whose
    ///   fix, renaming or moving the code, is always possible.
    /// - `expect`, `allow` or `warn` of `needless_raw_identifier`, whose fix, deleting the `r#`, is
    ///   always possible.
    /// - `expect`, `allow` or `warn` of `macro_definition`. The lints in this library skip the code
    ///   a macro writes, so a macro is allowed only in a crate `macro_crates` in `dylint.toml`
    ///   lists, which is a rule change.
    /// - Any of clippy's own attributes, outer or inner, such as `#[clippy::msrv = "1.50"]`,
    ///   which turns off every clippy lint that needs a newer Rust, or
    ///   `#[clippy::cognitive_complexity = "50"]`, which raises a threshold. They change what
    ///   clippy checks without an `expect`. `clippy::author`, `clippy::dump`,
    ///   `clippy::has_significant_drop`, `clippy::format_args` and `clippy::version` are rejected
    ///   too: a workspace needs none of them.
    /// - A lint level whose lints aren't written as paths, such as `paste`'s `[<...>]`.
    /// - A lint-level attribute on a module, inline or out-of-line: it covers every item in the
    ///   module and the modules inside it. Put the `expect` on the smallest item inside.
    /// - A lint-level attribute on an impl block or a trait that names a lint other than those
    ///   that report at the block itself, such as `operator_overload`, `from_without_constructor`,
    ///   `single_impl_trait` or `extension_trait_suffix` (the full list is `BLOCK_LINTS`). An
    ///   `expect` on a block covers every method in it, so a lint reported on a method takes its
    ///   `expect` on that method.
    /// - An attribute whose path is a macro variable, `#[$level(..)]` or `#![$level(..)]`, also
    ///   inside `cfg_attr`; a lint level naming a lint by a macro variable, `expect($lint, ..)`;
    ///   and a lint level on a macro variable, `#[expect(..)] $item`. Only the macro's caller
    ///   knows what they become, so the lint can't check them.
    /// - Tokens a macro can turn into an attribute the scan can't read: a `#` that doesn't start
    ///   an attribute, as when it is passed to a macro, `accessor! { # pub fn .. }`; an attribute
    ///   that doesn't start with a path, `#![[<al low>](..)]`; and a variable bound as `:tt`
    ///   followed by brackets, `$pound [allow(..)]`.
    /// - After expansion, any `allow`, `warn`, inner lint level or clippy attribute, and any
    ///   `expect` without a reason that explains, or of a group or a lint without exceptions, that
    ///   a macro wrote rather than the source: a macro the crate defines, a macro from a crate
    ///   `trusted_macro_crates` in `dylint.toml` doesn't list, or a macro that built it from
    ///   tokens of the source that aren't an attribute there, outside any item a macro wrote.
    ///   Attribute macros and derives such as `#[tokio::main]` and `#[derive(clap::Parser)]`
    ///   build their `allow`s from the caller's tokens, so that errors point at the caller's
    ///   code; such an attribute counts as written by the macros that wrote the item around it,
    ///   and passes if `trusted_macro_crates` lists each of their crates. One whose tokens all lie
    ///   inside a function-like macro call written in the source doesn't count as theirs, since
    ///   that call may have built it. Nor does one whose tokens all lie inside a string literal,
    ///   which a macro parsed from the string, as serde parses the type of
    ///   `#[serde(into = "..")]`: the caller wrote it where no lint reads it as an attribute, so
    ///   it is flagged however trusted the macro is.
    /// - A lint level that a trusted macro writes over code written in the source, which hides
    ///   that code from the lints it names, unless `trusted_macro_lints` in `dylint.toml` lists
    ///   each of them for the macro's crate; a lint group or one of clippy's own attributes is
    ///   never allowed there, whatever the code is, a literal or a path included.
    ///   `#[derive(clap::Parser)]` writes `allow(clippy::restriction, ..)` over the expressions
    ///   in `#[arg(..)]`, so a derive from a crate that does that can't take any, and
    ///   `trusted_macro_crates` shouldn't list it. Code counts as written in the source if the
    ///   source holds it at its span, not code a macro gives the caller's spans.
    ///
    /// The lint reads source text, so it sees attributes whatever `cfg` and `cfg_attr` evaluate
    /// to, in the crate root and the file of every `mod foo;` declared in them, including modules
    /// a `cfg` leaves out of the build. An attribute that a macro builds from its caller's tokens
    /// exists only after expansion, which is why macro variables in attributes are rejected, and
    /// `include!`, whose file isn't read, is rejected by `disabled_code`. It reports from the
    /// crate root, so no attribute can silence it: `#[expect]` on an item
    /// doesn't reach it, and an inner `#![allow(suppression_policy)]` or `#![allow(warnings)]`,
    /// besides being flagged itself, turns the report into a hard error. Only
    /// `-A suppression_policy` on the command line turns it off.
    ///
    /// ```rust,ignore
    /// #![allow(clippy::cast_possible_truncation)]
    /// #![cfg_attr(dylint_lib = "trellis_lints", expect(code_comment, reason = "legacy"))]
    ///
    /// #[cfg_attr(dylint_lib = "trellis_lints", allow(get_prefixed_getter))]
    /// #[expect(clippy::pedantic)]
    /// fn cents(dollars: f64) -> u32 {
    ///     (dollars * 100.0) as u32
    /// }
    /// ```
    ///
    /// Use instead an `expect` with a reason on the narrowest item, naming each lint:
    ///
    /// ```rust,ignore
    /// #[expect(clippy::cast_possible_truncation, reason = "balances stay below u32::MAX cents")]
    /// fn cents(dollars: f64) -> u32 {
    ///     (dollars * 100.0) as u32
    /// }
    /// ```
    ///
    /// For the lints in this library, which ordinary builds don't know, wrap the `expect` in
    /// `cfg_attr(dylint_lib = "trellis_lints", ..)`:
    ///
    /// ```rust,ignore
    /// #[cfg_attr(
    ///     dylint_lib = "trellis_lints",
    ///     expect(get_prefixed_getter, reason = "mirrors the field name of the vendor's API")
    /// )]
    /// fn get_balance(&self) -> u64 { self.balance }
    /// ```
    ///
    /// If a rule is wrong for the whole workspace, change it in `[workspace.lints]`.
    pub SUPPRESSION_POLICY,
    Warn,
    "a lint-level attribute that silences lints beyond one item, without a reason, or in only \
     some builds"
}

declare_lint_pass!(SuppressionPolicy => [SUPPRESSION_POLICY]);

/// The groups `expect` must not name, besides the groups the compiler registered. clippy and
/// rustdoc register theirs only in their own drivers.
const TOOL_GROUPS: &[&str] = &[
    "clippy::all",
    "clippy::cargo",
    "clippy::complexity",
    "clippy::correctness",
    "clippy::nursery",
    "clippy::pedantic",
    "clippy::perf",
    "clippy::restriction",
    "clippy::style",
    "clippy::suspicious",
    "rustdoc::all",
];

const FIX_THE_CODE: &str = "fix what the lint reports; if the rule itself is wrong for the \
                            workspace, change it in trellis's `lints/` and `docs/style.md`";
const RENAME_OR_MOVE: &str = "rename or move the code as the lint says; if the rule itself is \
                              wrong for the workspace, change it in trellis's `lints/` and `docs/style.md`";
const LIST_THE_CRATE: &str = "use a function, type or trait; a macro goes only in a crate that \
                              `macro_crates` in `dylint.toml` lists, which is a rule change";

/// The lints in this library that have no exceptions, each with what to do instead. `code_comment`,
/// `disabled_code`, `missing_workspace_lints` and `suppression_policy` guard the checks
/// themselves: they report from the crate root, where an item's `expect` can't reach them, and a
/// crate-wide one would silence every report. The fix for `file_named_after_item`,
/// `needless_raw_identifier` and `non_leaf_module_items`, renaming or moving the code or deleting
/// an `r#`, is always possible. An exception to
/// `macro_definition` would let the code a macro writes past the lints, which skip it, so only
/// `macro_crates`, a rule change, allows a macro.
const NO_EXCEPTION_LINTS: &[(&str, &str)] = &[
    ("code_comment", FIX_THE_CODE),
    ("disabled_code", FIX_THE_CODE),
    ("missing_workspace_lints", FIX_THE_CODE),
    ("suppression_policy", FIX_THE_CODE),
    ("file_named_after_item", RENAME_OR_MOVE),
    ("non_leaf_module_items", RENAME_OR_MOVE),
    ("macro_definition", LIST_THE_CRATE),
    ("needless_raw_identifier", FIX_THE_CODE),
];

/// The fewest distinct words, besides the words of the lints it names, that an `expect`'s reason
/// may have. A word is a run of letters and digits, compared ignoring case.
const REASON_MIN_WORDS: usize = 4;

/// The fewest characters an `expect`'s reason may have.
const REASON_MIN_CHARS: usize = 20;

/// The lints that report at an impl block or a trait itself rather than at an item inside it, so
/// an `expect` of them can only go on the block. An `expect` on a block covers everything inside
/// it, so it may name only these. Each clippy lint here was checked to fire at the block: an
/// `expect` of it on a method inside stays unfulfilled.
const BLOCK_LINTS: &[&str] = &[
    "extension_trait_suffix",
    "from_without_constructor",
    "operator_overload",
    "single_impl_trait",
    "clippy::expl_impl_clone_on_copy",
    "clippy::from_over_into",
    "clippy::infallible_try_from",
    "clippy::into_iter_without_iter",
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum LintLevel {
    Allow,
    Warn,
    Expect,
}

impl LintLevel {
    /// The level a lint-level attribute named `path` sets, if it can lower one. `deny` and
    /// `forbid` can't, so they give `None` like any other attribute.
    fn lowering(path: &str) -> Option<Self> {
        match path {
            "allow" => Some(Self::Allow),
            "warn" => Some(Self::Warn),
            "expect" => Some(Self::Expect),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Warn => "warn",
            Self::Expect => "expect",
        }
    }
}

impl EarlyLintPass for SuppressionPolicy {
    fn check_crate(&mut self, cx: &EarlyContext<'_>, krate: &Crate) {
        let groups: Vec<String> = rustc_lint::unerased_lint_store(cx.sess())
            .get_lint_groups()
            .map(|(name, _, _)| name.to_string())
            .chain(["warnings".to_owned()])
            .chain(TOOL_GROUPS.iter().map(|&group| group.to_owned()))
            .collect();
        let source = LocalSource::get(cx, krate);
        crate::trusted_lint_levels::check(cx, source, krate, &groups);
        let checker = Checker { cx, groups };
        for attribute in &source.attributes {
            let place = Place {
                span: attribute.span,
                inner: attribute.inner,
                target: attribute.target,
                origin: String::new(),
                origin_help: None,
            };
            checker.check(&attribute.meta, &place, &mut Vec::new());
        }
        for assembled in &source.assembled {
            checker.report_assembled(assembled.span, assembled.kind);
        }
        visit::walk_crate(
            &mut Expanded {
                checker: &checker,
                source,
            },
            krate,
        );
    }
}

/// Checks the lint levels of the expanded crate that macros wrote, which the source doesn't hold.
struct Expanded<'a, 'cx> {
    checker: &'a Checker<'a, 'cx>,
    source: &'a LocalSource,
}

impl<'ast> Visitor<'ast> for Expanded<'_, '_> {
    fn visit_attribute(&mut self, attribute: &'ast Attribute) {
        let path = attribute_origin::path_text(attribute);
        if LintLevel::lowering(&path).is_none() && !is_clippy_attribute(&path) {
            return;
        }
        let origin = attribute_origin::origin(self.checker.cx, self.source, attribute);
        if !origin.is_untrusted() {
            return;
        }
        let place = Place {
            span: attribute.span,
            inner: attribute.style == AttrStyle::Inner,
            target: Target::Other,
            origin: origin.describe(),
            origin_help: origin.untrusted_crate_help(),
        };
        self.checker
            .check(&attribute_origin::meta(attribute), &place, &mut Vec::new());
    }
}

/// Where a lint-level attribute is written.
struct Place {
    /// The whole attribute.
    span: Span,
    inner: bool,
    target: Target,
    /// Which macro wrote the attribute, as [`attribute_origin::Origin::describe`] says, or nothing
    /// if it is written in the source.
    origin: String,
    /// What to do about the macro that wrote the attribute, as
    /// [`attribute_origin::Origin::untrusted_crate_help`] says.
    origin_help: Option<String>,
}

impl Place {
    /// The place of tokens at `span` in the source, on no item the scan knows.
    fn written(span: Span) -> Self {
        Self {
            span,
            inner: false,
            target: Target::Other,
            origin: String::new(),
            origin_help: None,
        }
    }
}

struct Checker<'a, 'cx> {
    cx: &'a EarlyContext<'cx>,
    groups: Vec<String>,
}

impl Checker<'_, '_> {
    /// Checks `meta`, written at `place` behind the `cfg_attr` predicates in `predicates`.
    fn check<'m>(&self, meta: &'m Meta, place: &Place, predicates: &mut Vec<&'m Meta>) {
        let span = place.span;
        if let Some((predicate, attributes)) = meta.cfg_attr() {
            predicates.push(predicate);
            for guarded in attributes {
                self.check(guarded, place, predicates);
            }
            predicates.pop();
            return;
        }
        if matches!(meta.kind, MetaKind::Fragment) {
            self.report(
                place,
                meta.span,
                "attribute whose path is a macro variable",
                "the lints can't tell which attribute the macro's caller passes, so it may be a \
                 lint level they never check: write the attribute out in the macro, or put it on \
                 the item at the call",
            );
            return;
        }
        let bang = if place.inner { "!" } else { "" };
        if is_clippy_attribute(&meta.path) {
            self.report(
                place,
                span,
                &format!("`#{bang}[{}]` attribute", meta.path),
                "clippy's own attributes change what clippy checks without an `expect`: \
                 `clippy::msrv` turns off every lint that needs a newer Rust and \
                 `clippy::cognitive_complexity` raises a threshold. Set the Rust version with \
                 `rust-version` in `Cargo.toml` and thresholds in `clippy.toml`, or put an \
                 `expect` with a reason on the item",
            );
            return;
        }
        let Some(level) = LintLevel::lowering(&meta.path) else {
            return;
        };
        let name = level.name();
        match level {
            LintLevel::Allow => self.report(
                place,
                span,
                "`allow` attribute",
                "use `#[expect(lint, reason = \"...\")]` on the narrowest item, which fails once \
                 the lint stops firing",
            ),
            LintLevel::Warn => self.report(
                place,
                span,
                "`warn` attribute",
                "lint levels come from `[workspace.lints]` in `Cargo.toml`; an attribute may only \
                 tighten them, with `deny` or `forbid`",
            ),
            LintLevel::Expect => {}
        }
        if place.inner {
            self.report(
                place,
                span,
                &format!("inner `#![{name}(..)]` attribute"),
                "it applies to everything in the enclosing crate, module or function: put an \
                 outer `#[expect(lint, reason = \"...\")]` on the narrowest item instead, or \
                 change the level in `[workspace.lints]` in `Cargo.toml`",
            );
        }
        self.check_target(meta, place, level);
        let only_dead_code = level == LintLevel::Expect && expects_only_dead_code(meta);
        let allowed = |predicate: &&Meta| {
            predicate.is_own_lint_run() || only_dead_code && predicate.is_not_test()
        };
        if !predicates.iter().all(allowed) {
            self.report(
                place,
                span,
                &format!(
                    "`#{bang}[{name}(..)]` behind a `cfg_attr` that is on in only some builds"
                ),
                "clippy runs without `dylint_lib` and `trellis_lints` runs with it, and tests \
                 build apart from the code that ships, so the attribute escapes some of them; the \
                 only allowed forms are `#[cfg_attr(dylint_lib = \"trellis_lints\", expect(lint, \
                 reason = \"...\"))]`, for the lints in this library, and \
                 `#[cfg_attr(not(test), expect(dead_code, reason = \"...\"))]`, for code only \
                 tests use until its caller lands",
            );
        }
        self.check_lints(meta, place, level);
        if level == LintLevel::Expect {
            self.check_expect(meta, place);
        }
    }

    /// Checks the lints that `meta`, a lint-level attribute at `place`, names: none may be one
    /// without exceptions, and each must be written out.
    fn check_lints(&self, meta: &Meta, place: &Place, level: LintLevel) {
        let name = level.name();
        for item in meta.items() {
            match item.kind {
                MetaKind::Word => {
                    if let Some((lint, help)) = NO_EXCEPTION_LINTS
                        .iter()
                        .find(|(lint, _)| *lint == item.path)
                    {
                        self.report(
                            place,
                            item.span,
                            &format!("`{name}` of `{lint}`, which has no exceptions"),
                            help,
                        );
                    }
                }
                MetaKind::Fragment => self.report(
                    place,
                    item.span,
                    &format!("`{name}` of a lint named by a macro variable"),
                    "the lints can't tell which lint the macro's caller names, so it may be a \
                     group or a lint without exceptions: name the lint in the macro",
                ),
                MetaKind::Other => self.report(
                    place,
                    item.span,
                    &format!("`{name}` of a lint that isn't written as a path"),
                    "the lints can't tell which lint the tokens name once a macro joins them: \
                     write the lint's name out",
                ),
                _ => {}
            }
        }
    }

    /// Checks an `expect` at `place` has a reason long enough to judge and names no lint group.
    fn check_expect(&self, meta: &Meta, place: &Place) {
        let reason = meta.items().iter().find_map(|item| match &item.kind {
            MetaKind::Value(Some(reason)) if item.path == "reason" && !reason.trim().is_empty() => {
                Some((item.span, reason))
            }
            _ => None,
        });
        match reason {
            None => self.report(
                place,
                place.span,
                "`expect` without a reason",
                "add `reason = \"...\"` saying why the lint is wrong here",
            ),
            Some((span, reason)) if !explains(reason, meta.items()) => self.report(
                place,
                span,
                "`expect` whose reason is too short to say why the lint is wrong here",
                &format!(
                    "a reason needs at least {REASON_MIN_WORDS} different words besides the \
                     words of the lint's name, and {REASON_MIN_CHARS} characters: say what about \
                     this code makes the lint wrong here"
                ),
            ),
            Some(_) => {}
        }
        for item in meta.items() {
            if matches!(item.kind, MetaKind::Word) && self.groups.contains(&item.path) {
                self.report(
                    place,
                    item.span,
                    &format!("`expect` of the lint group `{}`", item.path),
                    "expect each lint that fires here by name, so the exception covers only them",
                );
            }
        }
    }

    /// Checks the item that `meta`, a lint-level attribute at `place`, is written on: a module
    /// or a macro variable takes none, and an impl block or a trait only one for a lint that
    /// reports at the block itself.
    fn check_target(&self, meta: &Meta, place: &Place, level: LintLevel) {
        let name = level.name();
        match place.target {
            Target::Module => self.report(
                place,
                place.span,
                &format!("`{name}` on a module"),
                "it covers every item in the module and in the modules inside it: put the \
                 `expect` on the smallest item inside that the lint reports",
            ),
            Target::Fragment => self.report(
                place,
                place.span,
                &format!("`{name}` on a macro variable"),
                "the lints can't tell what the macro's caller passes, which may be a whole \
                 module: put the `expect` on the item at the call",
            ),
            Target::Impl | Target::Trait => {
                let block = if place.target == Target::Impl {
                    "an impl block"
                } else {
                    "a trait"
                };
                for item in meta.items() {
                    if matches!(item.kind, MetaKind::Word)
                        && !BLOCK_LINTS.contains(&item.path.as_str())
                        && !self.groups.contains(&item.path)
                    {
                        self.report(
                            place,
                            item.span,
                            &format!("`{name}` of `{}` on {block}", item.path),
                            "it covers every item in the block: put it on the item inside that \
                             the lint reports. Only the lints that report at an impl block or \
                             trait itself, listed in `BLOCK_LINTS` in \
                             `lints/src/suppression_policy.rs`, may be expected there",
                        );
                    }
                }
            }
            Target::Other => {}
        }
    }

    /// Reports tokens at `span` that a macro can turn into an attribute the scan can't read.
    fn report_assembled(&self, span: Span, kind: Assembled) {
        let (message, help) = match kind {
            Assembled::NoPath => (
                "attribute that doesn't start with a path",
                "the lints read an attribute by its path, so one that a macro such as `paste` \
                 builds from other tokens may be a lint level they never check: write the \
                 attribute out",
            ),
            Assembled::LoosePound => (
                "`#` that doesn't start an attribute",
                "a macro given a `#` can put brackets after it and build an attribute the lints \
                 never read: write the attribute out on the item it applies to",
            ),
            Assembled::TokenTreeBeforeBrackets => (
                "brackets after a `:tt` macro variable, which make an attribute when the caller \
                 passes `#`",
                "the lints can't read an attribute built from a `#` the caller passes: write \
                 `#[...]` out in the macro's body, or bind the variable to a fragment other than \
                 `tt`, such as `expr` or `ident`",
            ),
        };
        self.report(&Place::written(span), span, message, help);
    }

    /// Reports `message` at `span`, about the attribute at `place`.
    fn report(&self, place: &Place, span: Span, message: &str, help: &str) {
        span_lint_from_command_line(
            self.cx,
            SUPPRESSION_POLICY,
            attribute_origin::report_span(self.cx, span),
            &format!("{message}{}", place.origin),
            &[help]
                .into_iter()
                .chain(place.origin_help.as_deref())
                .collect::<Vec<_>>(),
        );
    }
}

/// Whether `reason`, of an `expect` whose items are `items`, has at least [`REASON_MIN_CHARS`]
/// characters and [`REASON_MIN_WORDS`] distinct words that aren't words of the lints it names, so
/// it can't be only a lint's name, such as `needless pass by value` for
/// `clippy::needless_pass_by_value`.
fn explains(reason: &str, items: &[Meta]) -> bool {
    let words = |text: &str| -> Vec<String> {
        text.split(|character: char| !character.is_alphanumeric())
            .filter(|word| !word.is_empty())
            .map(str::to_lowercase)
            .collect()
    };
    let lint_words: Vec<String> = items
        .iter()
        .filter(|item| matches!(item.kind, MetaKind::Word))
        .flat_map(|item| words(&item.path))
        .collect();
    let mut own_words: Vec<String> = words(reason)
        .into_iter()
        .filter(|word| !lint_words.contains(word))
        .collect();
    own_words.sort_unstable();
    own_words.dedup();
    reason.trim().chars().count() >= REASON_MIN_CHARS && own_words.len() >= REASON_MIN_WORDS
}

/// Whether `path`, an attribute's path, names one of clippy's own attributes, such as
/// `clippy::msrv`.
fn is_clippy_attribute(path: &str) -> bool {
    path.starts_with("clippy::")
}

/// Whether `meta`, an `expect`, names `dead_code` and no other lint.
fn expects_only_dead_code(meta: &Meta) -> bool {
    let mut lints = meta
        .items()
        .iter()
        .filter(|item| matches!(item.kind, MetaKind::Word));
    lints.clone().next().is_some() && lints.all(|lint| lint.path == "dead_code")
}

#[test]
fn ui() {
    crate::ui_harness::ui_test_of(SUPPRESSION_POLICY, "ui/suppression_policy")
        .rustc_flags(["-A", "unfulfilled_lint_expectations"])
        .run();
}

/// An inner attribute that sets the level of `suppression_policy` itself turns its reports into
/// errors rather than silencing them.
#[test]
fn ui_self_suppressed() {
    crate::ui_harness::ui_test_of(SUPPRESSION_POLICY, "ui/suppression_policy_self_suppressed")
        .run();
}

/// A lint level that a macro from a crate `trusted_macro_crates` doesn't list writes is flagged.
#[test]
fn ui_macro_crates() {
    crate::ui_harness::ui_test_of(SUPPRESSION_POLICY, "ui/suppression_policy_macro_crates")
        .dylint_toml(crate::ui_harness::dylint_toml_with(
            "trusted_macro_crates",
            Some(r#"["alloc", "core", "std"]"#),
        ))
        .run();
}

/// Once `trusted_macro_crates` lists the crate, the lint levels its macros write pass, but over
/// code written in the source only those of lints `trusted_macro_lints` lists for it, never a
/// group or one of clippy's own attributes.
#[test]
fn ui_trusted_macro_crates() {
    crate::ui_harness::ui_test_of(
        SUPPRESSION_POLICY,
        "ui/suppression_policy_trusted_macro_crates",
    )
    .dylint_toml(crate::ui_harness::dylint_toml_with_all(&[
        (
            "trusted_macro_crates",
            Some(r#"["alloc", "core", "std", "quiet"]"#),
        ),
        (
            "trusted_macro_lints",
            Some(r#"{ quiet = ["clippy::unwrap_used"] }"#),
        ),
    ]))
    .run();
}

/// A lint level that a proc macro from a crate `trusted_macro_crates` doesn't list builds from the
/// caller's tokens is flagged, as are one a function-like macro builds outside any item a macro
/// wrote and one written in the source.
#[test]
fn ui_proc_macros() {
    crate::ui_harness::ui_test_of(SUPPRESSION_POLICY, "ui/suppression_policy_proc_macros")
        .dylint_toml(crate::ui_harness::dylint_toml_with(
            "trusted_macro_crates",
            Some(r#"["alloc", "core", "std"]"#),
        ))
        .run();
}

/// Once `trusted_macro_crates` lists the crate, the lint levels its attribute macros and derives
/// build from the caller's tokens pass, here with `trusted_macro_lints` listing the one lint they
/// set over the source.
#[test]
fn ui_trusted_proc_macros() {
    crate::ui_harness::ui_test_of(
        SUPPRESSION_POLICY,
        "ui/suppression_policy_trusted_proc_macros",
    )
    .dylint_toml(crate::ui_harness::dylint_toml_with_all(&[
        (
            "trusted_macro_crates",
            Some(r#"["alloc", "core", "std", "hush"]"#),
        ),
        ("trusted_macro_lints", Some(r#"{ hush = ["dead_code"] }"#)),
    ]))
    .run();
}

/// A lint level that a trusted macro writes over code written in the source passes only if
/// `trusted_macro_lints` lists its lints for the macro's crate, and never if it names a group,
/// whatever the code is.
#[test]
fn ui_trusted_lints() {
    crate::ui_harness::ui_test_of(SUPPRESSION_POLICY, "ui/suppression_policy_trusted_lints")
        .dylint_toml(crate::ui_harness::dylint_toml_with_all(&[
            (
                "trusted_macro_crates",
                Some(r#"["alloc", "core", "std", "levels"]"#),
            ),
            (
                "trusted_macro_lints",
                Some(r#"{ levels = ["clippy::redundant_closure_call"] }"#),
            ),
        ]))
        .run();
}
