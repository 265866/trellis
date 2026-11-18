use clippy_utils::diagnostics::{span_lint_and_help, span_lint_hir_and_then};
use rustc_ast::visit::{self, Visitor};
use rustc_ast::{AssocItem, Attribute, Crate, Expr, ExprKind, Item, Pat, PatKind, Path, Ty};
use rustc_hir as hir;
use rustc_hir::def::{DefKind, Res};
use rustc_lexer::{FrontmatterAllowed, LiteralKind, TokenKind};
use rustc_lint::{EarlyContext, EarlyLintPass, LateContext, LateLintPass, LintContext};
use rustc_lint_defs::{declare_lint, declare_lint_pass};
use rustc_span::{FileName, Span, Symbol, sym};

use crate::config::CiTarget;
use crate::local_source::{CfgSelectArm, LocalSource, Meta, MetaKind, SourceAttribute, Target};
use crate::string_literal::{self, Literal};
use crate::{attribute_origin, imports};

declare_lint! {
    /// Checks for code that a check never sees:
    ///
    /// - `#[ignore]` and `#[ignore = "..."]`, directly or through `cfg_attr`. An ignored test is
    ///   a failing check that nobody runs. After expansion, an `#[ignore]` that a macro wrote is
    ///   flagged too, unless the macro comes from a crate `trusted_macro_crates` in
    ///   `dylint.toml` lists.
    /// - A test attribute with an argument. A test attribute is `#[test]` or one whose last
    ///   segment is `test` or ends in `_test`, such as `#[sqlx::test]`, and it takes no
    ///   arguments. An argument can set a run count of 0, which never runs the test, retry a test
    ///   until it passes, which hides a flaky one, or run code that can exit the process before
    ///   the failure is reported. The lint can't tell those from harmless arguments, so every
    ///   argument is flagged, including one it can't read.
    /// - A code block in a doc comment that rustdoc would run as a doctest but for `ignore`, such
    ///   as ```` ```ignore ```` or ```` ```rust,ignore ````. An ignored doctest is an example
    ///   nobody compiles. ```` ```no_run ```` still compiles it, and ```` ```text ```` or another
    ///   language marks it as not Rust.
    /// - A `cfg` predicate that is false in every build, in `#[cfg(..)]`, `#[cfg_attr(..)]`,
    ///   `cfg!(..)` or an arm of `cfg_select! { .. }`: `any()`, `false`, `not(all())`, or an
    ///   `all(..)` that contradicts itself, such as `all(unix, windows)` or two values of
    ///   `target_os`. The code it guards is never compiled or never runs.
    /// - A `cfg` predicate that names `clippy` or `dylint_lib`, which set apart the clippy run and
    ///   the run of `trellis_lints`, or `doc`, `doctest`, `miri`, `proc_macro`, `rustfmt` or
    ///   `rust_analyzer`, which neither lint run sets, so code behind it escapes one of the runs
    ///   or both. A `cfg_attr`
    ///   around only lint-level attributes is left to `suppression_policy`, which allows
    ///   `cfg_attr(dylint_lib = "trellis_lints", expect(..))`.
    /// - A `cfg` predicate that names `debug_assertions`, `feature` or `panic`. The lints and tests
    ///   run debug builds with every feature on and `panic = "unwind"`, so code behind such a
    ///   predicate either ships in release builds unchecked or never ships. The workspace's
    ///   features only forward to dependencies, so code never branches on them. The one exception
    ///   is a `cfg_attr` whose only attribute is `windows_subsystem`, which sets how Windows starts
    ///   the binary rather than which code it holds.
    /// - A `cfg` predicate that names a target CI doesn't build, so no check compiles the code
    ///   behind it. CI builds one target for each operating system `ci_targets` in `dylint.toml`
    ///   lists, `linux`, `macos` or `windows`, so a predicate may name `target_os` of a listed
    ///   system, `windows` or `target_family = "windows"` if `windows` is listed, and `unix` or
    ///   `target_family = "unix"` if `linux` or `macos` is, and nothing else about the target: no
    ///   other `target_os` or `target_family`, and no `target_arch`, `target_env`, `target_abi`,
    ///   `target_pointer_width`, `target_vendor`, `target_feature`, `target_endian` or
    ///   `target_has_atomic`, which leave code out of some of those builds, or put it only in
    ///   builds for targets CI never builds. A predicate on those names that is false on every
    ///   listed system, such as `not(target_os = "linux")` when only `linux` is listed, is
    ///   flagged too.
    ///
    ///   ```toml
    ///   [trellis_lints]
    ///   ci_targets = ["linux"]
    ///   ```
    /// - `cfg!(test)`, and `#[cfg(test)]` or `cfg_attr(test, ..)` outside test code, which make
    ///   the code behave differently in tests than in the builds that ship. `#[cfg(test)]` may
    ///   go on a `mod`, which makes it a test module, and code inside a test module or a
    ///   `#[test]` function may use `#[cfg(test)]` and `cfg_attr(test, ..)`. A predicate on
    ///   `test` that can hold outside tests, such as `not(test)` or `any(test, unix)`, is allowed
    ///   nowhere.
    /// - A `cfg` predicate written as a macro variable, `cfg($predicate)`, which only the macro's
    ///   caller knows.
    /// - A `cfg_select!` arm that the rules above flag as a `cfg!`, or that no target CI builds
    ///   selects because the arms before it cover every one, such as `_` after `unix` when
    ///   `ci_targets` lists only Linux, or an arm the lint can't read.
    /// - A `use` of `cfg!` or `cfg_select!`, renamed or not, such as
    ///   `use std::cfg_select as pick;`. The lint finds their calls by name, so a call through an
    ///   import would go unread.
    /// - `include!(..)`. The file it brings in isn't a module, so the lints that read source
    ///   text, such as this one and `code_comment`, never read it. Code that comes from a file
    ///   other than a module, however it got there, is flagged too.
    ///
    /// The lint reads source text, so it sees attributes whatever `cfg` and `cfg_attr` evaluate
    /// to, in the crate root and the file of every `mod foo;` declared in them, including modules
    /// a `cfg` leaves out of the build.
    ///
    /// A `cfg` or `cfg_attr` attribute, a `cfg!` or a `cfg_select!` inside a string that a macro
    /// parses as code, as serde parses the type of `#[serde(into = "..")]`, is flagged too: the
    /// compiler applies it to the macro's output, but the scan reads only a string. A string counts
    /// as parsed if the expanded crate holds code at its span other than the string itself and the
    /// names its `{..}` placeholders read, so a `rename`, or a `format!` or thiserror message, that
    /// mentions `#[cfg(test)]` isn't flagged. A name a macro calls is code even if a placeholder
    /// spells it.
    ///
    /// ```rust,ignore
    /// #[test]
    /// #[ignore = "flaky on CI"]
    /// fn stops_at_the_reorder_point() { ... }
    ///
    /// #[framework::test(retries = 3)]
    /// fn reconnects_after_a_dropped_session() { ... }
    ///
    /// #[cfg(any())]
    /// fn old_parser() { ... }
    ///
    /// #[cfg(not(clippy))]
    /// fn first(values: &[u32]) -> u32 { *values.first().unwrap() }
    ///
    /// #[cfg(not(debug_assertions))]
    /// fn release_only_retry() { ... }
    ///
    /// include!("generated.rs");
    /// ```
    ///
    /// Use instead: fix the test so it passes every time, delete the code that no build uses,
    /// make code the same in every build, and write code in a module.
    pub DISABLED_CODE,
    Warn,
    "an ignored or retried test or doctest, `include!`, or code that `cfg` removes from every \
     build, from a lint run or from release builds"
}

declare_lint_pass!(DisabledCode => [DISABLED_CODE]);

/// The `cfg` names that tell the clippy run and the run of `trellis_lints` apart, or that
/// neither run sets. The compiler sets `proc_macro` only for a proc-macro crate, which no crate
/// of the workspace may be.
const LINT_RUN_CFGS: &[&str] = &[
    "clippy",
    "dylint_lib",
    "doc",
    "doctest",
    "miri",
    "proc_macro",
    "rustfmt",
    "rust_analyzer",
];

/// The `cfg` names whose value differs between the lint runs, which build for debugging with
/// every feature, and release builds.
const BUILD_PROFILE_CFGS: &[&str] = &["debug_assertions", "feature", "panic"];

/// The `cfg` names about the target that a predicate may not name at all. CI builds one Windows,
/// one Linux and one macOS target, on two architectures, so code behind them is left out of some
/// of those builds, or built only for targets CI never builds. `target_has_atomic` stands for
/// every name that starts with it.
const UNBUILT_TARGET_CFGS: &[&str] = &[
    "target_abi",
    "target_arch",
    "target_endian",
    "target_env",
    "target_feature",
    "target_has_atomic",
    "target_pointer_width",
    "target_vendor",
];

/// The `cfg` keys a target has exactly one value of, so `all(..)` with two values of one is
/// false.
const SINGLE_VALUED_CFGS: &[&str] = &[
    "panic",
    "target_arch",
    "target_endian",
    "target_env",
    "target_os",
    "target_pointer_width",
    "target_vendor",
];

impl EarlyLintPass for DisabledCode {
    fn check_crate(&mut self, cx: &EarlyContext<'_>, krate: &Crate) {
        let source = LocalSource::get(cx, krate);
        for attribute in &source.attributes {
            check_attribute(cx, &attribute.meta, attribute);
        }
        for (span, predicate) in &source.cfg_macros {
            check_predicate(cx, predicate, *span, "cfg!");
            check_target(cx, predicate, *span, "cfg!");
            if names(predicate, &["test"]).is_some() {
                report_test(cx, *span, "cfg!", false);
            }
        }
        for arms in &source.cfg_selects {
            check_cfg_select(cx, arms);
        }
        for &span in &source.ignored_doctests {
            span_lint_and_help(
                cx,
                DISABLED_CODE,
                span,
                "doctest marked `ignore`",
                None,
                "an ignored doctest is an example nobody compiles: make it run, mark it `no_run` \
                 if it must compile but not run, or `text` if it isn't Rust",
            );
        }
        for include in &source.includes {
            report_include(cx, include.span);
        }
        let mut outside = OutsideModules {
            cx,
            source,
            reported: Vec::new(),
        };
        visit::walk_crate(&mut outside, krate);
        visit::walk_crate(&mut ExpandedIgnores { cx, source }, krate);
        check_parsed_strings(cx, source, krate);
    }
}

/// Reports each string literal of the source that holds a `cfg` or `cfg_attr` attribute, a `cfg!`
/// or a `cfg_select!` and that a macro parsed as code, as serde parses the type of
/// `#[serde(into = "..")]`. The compiler evaluates such an attribute on the macro's output, so it
/// leaves code out of a lint run as one written outside a string would, but the scan of the
/// source sees only a string. A macro parsed the string if the expanded crate holds code at a
/// span inside it other than a literal or a name one of its `{..}` placeholders reads, so a
/// message such as `format!("{name} needs #[cfg(test)]")` or thiserror's `#[error("..")]` is
/// only text. A name a macro calls is code even if a placeholder spells it.
fn check_parsed_strings(cx: &EarlyContext<'_>, source: &LocalSource, krate: &Crate) {
    let mut strings = ParsedStrings {
        candidates: cfg_strings(source),
        parsed: Vec::new(),
    };
    if strings.candidates.is_empty() {
        return;
    }
    visit::walk_crate(&mut strings, krate);
    for candidate in strings.parsed {
        span_lint_and_help(
            cx,
            DISABLED_CODE,
            candidate.span,
            format!(
                "{} in a string that a macro parses as code",
                candidate.found
            ),
            None,
            "the lints read the string as text, so they miss what the `cfg` does to the code: \
             write the code outside the string, in a function or a type with a name, and name \
             that in the string",
        );
    }
}

/// A string literal of the source whose value holds a `cfg` or `cfg_attr` attribute, a `cfg!` or
/// a `cfg_select!`.
struct CfgString {
    span: Span,
    value: String,
    /// What the string holds, as the report names it, such as "`cfg` attribute".
    found: &'static str,
}

/// The string literals of the source whose value holds a `cfg` or `cfg_attr` attribute, a `cfg!`
/// or a `cfg_select!`. A literal with an escape the lints can't read is matched as written.
fn cfg_strings(source: &LocalSource) -> Vec<CfgString> {
    let mut found = Vec::new();
    for file in &source.files {
        for token in &file.tokens {
            let TokenKind::Literal { kind, .. } = token.kind else {
                continue;
            };
            if !matches!(
                kind,
                LiteralKind::Str { .. }
                    | LiteralKind::RawStr { .. }
                    | LiteralKind::ByteStr { .. }
                    | LiteralKind::RawByteStr { .. }
                    | LiteralKind::CStr { .. }
                    | LiteralKind::RawCStr { .. }
            ) {
                continue;
            }
            let text = &file.source[token.range.clone()];
            let text = text.trim_start_matches(['b', 'c']);
            let value = match string_literal::literal(text) {
                Some(Literal::Value(value)) => value,
                _ => text.to_owned(),
            };
            if let Some(cfg) = cfg_code(&value) {
                found.push(CfgString {
                    span: file.span(token.range.clone()),
                    value,
                    found: cfg,
                });
            }
        }
    }
    found
}

/// What the first `cfg` or `cfg_attr` attribute, `cfg!` or `cfg_select!` in `code` is, as the
/// report names it, if it holds one: `#[cfg`, `#![cfg`, or `cfg!` or `cfg_select!` followed by
/// `(`, `[` or `{`, with any whitespace or comments between the tokens.
fn cfg_code(code: &str) -> Option<&'static str> {
    let mut start = 0;
    let mut tokens: Vec<(TokenKind, &str)> = Vec::new();
    for token in rustc_lexer::tokenize(code, FrontmatterAllowed::No) {
        let end = start + token.len as usize;
        if !matches!(
            token.kind,
            TokenKind::Whitespace | TokenKind::LineComment { .. } | TokenKind::BlockComment { .. }
        ) {
            tokens.push((token.kind, &code[start..end]));
        }
        start = end;
    }
    tokens.windows(3).enumerate().find_map(|(index, window)| {
        if let [
            (TokenKind::Ident | TokenKind::RawIdent, name),
            (TokenKind::Bang, _),
            (TokenKind::OpenParen | TokenKind::OpenBracket | TokenKind::OpenBrace, _),
        ] = window
        {
            match name.trim_start_matches("r#") {
                "cfg_select" => return Some("`cfg_select!`"),
                "cfg" => return Some("`cfg!`"),
                _ => {}
            }
        }
        let [
            (TokenKind::OpenBracket, _),
            (TokenKind::Ident | TokenKind::RawIdent, name),
            _,
        ] = window
        else {
            return None;
        };
        let pound = match index.checked_sub(1).map(|before| tokens[before].0) {
            Some(TokenKind::Bang) => index.checked_sub(2).map(|before| tokens[before].0),
            before => before,
        };
        if pound != Some(TokenKind::Pound) {
            return None;
        }
        match name.trim_start_matches("r#") {
            "cfg" => Some("`cfg` attribute"),
            "cfg_attr" => Some("`cfg_attr` attribute"),
            _ => None,
        }
    })
}

/// Finds which of `candidates`, string literals of the source, a macro parsed code from: the
/// expanded crate holds a node at a span inside one, other than a literal or a name one of the
/// string's `{..}` placeholders reads.
struct ParsedStrings {
    candidates: Vec<CfgString>,
    parsed: Vec<CfgString>,
}

impl ParsedStrings {
    /// The candidate whose string holds `span`, if one does.
    fn candidate(&self, span: Span) -> Option<usize> {
        if span.from_expansion() || span.is_dummy() {
            return None;
        }
        self.candidates
            .iter()
            .position(|string| string.span.lo() <= span.lo() && span.hi() <= string.span.hi())
    }

    fn record(&mut self, span: Span) {
        if let Some(index) = self.candidate(span) {
            self.parsed.push(self.candidates.swap_remove(index));
        }
    }

    /// Records `span` unless `name` is a name one of the `{..}` placeholders of the string at
    /// `span` reads, as `format!` reads `name` for `{name}` and thiserror binds `__field_name`.
    fn record_name(&mut self, span: Span, name: Symbol) {
        let Some(index) = self.candidate(span) else {
            return;
        };
        if !string_literal::reads_placeholder(&self.candidates[index].value, name.as_str()) {
            self.parsed.push(self.candidates.swap_remove(index));
        }
    }
}

/// The name `path` is, if it is one name with no generic arguments.
fn single_name(path: &Path) -> Option<Symbol> {
    match path.segments.as_slice() {
        [segment] if segment.args.is_none() => Some(segment.ident.name),
        _ => None,
    }
}

impl<'ast> Visitor<'ast> for ParsedStrings {
    fn visit_item(&mut self, item: &'ast Item) {
        self.record(item.span);
        visit::walk_item(self, item);
    }

    /// Records each expression but a literal. A name a macro calls is code, as serde calls the
    /// function a `deserialize_with` string names, even if a `{..}` in the string spells it. A
    /// path of one name, and thiserror's `name.as_display()` for `{name}` and `_0.as_display()`
    /// for `{0}`, count unless a placeholder reads the name.
    fn visit_expr(&mut self, expr: &'ast Expr) {
        match &expr.kind {
            ExprKind::Lit(_) => {}
            ExprKind::Call(callee, _) => {
                self.record(callee.span);
                self.record(expr.span);
            }
            ExprKind::Path(None, path) if let Some(name) = single_name(path) => {
                self.record_name(expr.span, name);
            }
            ExprKind::MethodCall(call)
                if call.args.is_empty()
                    && call.seg.args.is_none()
                    && call.seg.ident.name.as_str() == "as_display"
                    && let ExprKind::Path(None, path) = &call.receiver.kind
                    && let Some(name) = single_name(path) =>
            {
                self.record_name(expr.span, name);
            }
            _ => self.record(expr.span),
        }
        visit::walk_expr(self, expr);
    }

    fn visit_ty(&mut self, ty: &'ast Ty) {
        self.record(ty.span);
        visit::walk_ty(self, ty);
    }

    /// Records each pattern but a literal or a binding whose name a placeholder reads. serde
    /// matches a `rename` string as a literal pattern at the string's span.
    fn visit_pat(&mut self, pattern: &'ast Pat) {
        match &pattern.kind {
            PatKind::Expr(expr) if matches!(expr.kind, ExprKind::Lit(_)) => {}
            PatKind::Ident(_, ident, None) => self.record_name(pattern.span, ident.name),
            _ => self.record(pattern.span),
        }
        visit::walk_pat(self, pattern);
    }

    fn visit_path(&mut self, path: &'ast Path) {
        match single_name(path) {
            Some(name) => self.record_name(path.span, name),
            None => self.record(path.span),
        }
        visit::walk_path(self, path);
    }
}

declare_lint_pass!(CfgImports => []);

/// Finds each `use` that imports the `cfg!` or `cfg_select!` macro, as the compiler resolved it,
/// so a module or function of the same name isn't one. The lint finds their calls in the source by
/// name, so a call through an import, under whatever name, would go unread. Resolution exists only
/// after expansion, so this is a late pass of its own, reporting under `disabled_code`.
impl<'tcx> LateLintPass<'tcx> for CfgImports {
    fn check_item(&mut self, cx: &LateContext<'tcx>, item: &'tcx hir::Item<'tcx>) {
        for import in imports::imports(cx.tcx, item) {
            let Some(Res::Def(DefKind::Macro(_), macro_id)) = import.res.macro_ns else {
                continue;
            };
            let name = cx.tcx.item_name(macro_id);
            if import.name.is_none()
                || !matches!(name.as_str(), "cfg" | "cfg_select")
                || !matches!(cx.tcx.crate_name(macro_id.krate).as_str(), "core" | "std")
            {
                continue;
            }
            span_lint_hir_and_then(
                cx,
                DISABLED_CODE,
                import.hir_id,
                import.span,
                format!("`use` of `{name}!`"),
                |diagnostic| {
                    diagnostic.help(format!(
                        "call `{name}!` by its own name and drop the `use`: the lints find its \
                         calls by that name, so a call through an import goes unchecked"
                    ));
                },
            );
        }
    }
}

/// Checks each arm of a `cfg_select!`: its own predicate as a `cfg!`'s, and the condition under
/// which the arm is the one compiled, its predicate and none of the ones before it, for whether it
/// holds in some build CI makes. A `_` arm is compiled when no arm before it is.
fn check_cfg_select(cx: &EarlyContext<'_>, arms: &[CfgSelectArm]) {
    const CONSTRUCT: &str = "cfg_select!";
    let mut before: Vec<Meta> = Vec::new();
    for arm in arms {
        if let Some(predicate) = &arm.predicate {
            if matches!(predicate.kind, MetaKind::Other) {
                span_lint_and_help(
                    cx,
                    DISABLED_CODE,
                    arm.span,
                    "`cfg_select!` arm the lints can't read",
                    None,
                    "no check can tell which builds hold the code behind it: write each arm as \
                     `predicate => { .. }` or `_ => { .. }`",
                );
                return;
            }
            check_predicate(cx, predicate, arm.span, CONSTRUCT);
            check_target(cx, predicate, arm.span, CONSTRUCT);
            if names(predicate, &["test"]).is_some() {
                report_test(cx, arm.span, CONSTRUCT, false);
            }
        }
        let earlier = Meta {
            path: "not".to_owned(),
            kind: MetaKind::List(vec![Meta {
                path: "any".to_owned(),
                kind: MetaKind::List(before.clone()),
                span: arm.span,
            }]),
            span: arm.span,
        };
        let selected = Meta {
            path: "all".to_owned(),
            kind: MetaKind::List(arm.predicate.iter().cloned().chain([earlier]).collect()),
            span: arm.span,
        };
        let own_report = arm.predicate.as_ref().is_some_and(|predicate| {
            always_false(predicate)
                || unbuilt_target(predicate, &crate::config::get(cx.sess()).ci_targets).is_some()
                || holds_on_no_target(cx, predicate)
        });
        if !own_report && (always_false(&selected) || holds_on_no_target(cx, &selected)) {
            span_lint_and_help(
                cx,
                DISABLED_CODE,
                arm.span,
                "`cfg_select!` arm that no target CI builds selects",
                None,
                format!(
                    "the arms before it cover every system `ci_targets` in `dylint.toml` lists \
                     ({}), so no check compiles it: delete it, or list the system it is for",
                    listed_targets(cx)
                ),
            );
        }
        before.extend(arm.predicate.iter().cloned());
    }
}

/// Finds the `#[ignore]` attributes of the expanded crate that macros wrote, which the source
/// doesn't hold.
struct ExpandedIgnores<'a, 'cx> {
    cx: &'a EarlyContext<'cx>,
    source: &'a LocalSource,
}

impl<'ast> Visitor<'ast> for ExpandedIgnores<'_, '_> {
    fn visit_attribute(&mut self, attribute: &'ast Attribute) {
        if !attribute.has_name(sym::ignore) {
            return;
        }
        let origin = attribute_origin::origin(self.cx, self.source, attribute);
        if origin.is_untrusted() {
            report_ignore(self.cx, attribute.span, &origin.describe());
        }
    }
}

/// Reports an `#[ignore]` at `span`, which `origin`, as [`attribute_origin::Origin::describe`]
/// says it, wrote.
fn report_ignore(cx: &EarlyContext<'_>, span: Span, origin: &str) {
    span_lint_and_help(
        cx,
        DISABLED_CODE,
        attribute_origin::report_span(cx, span),
        format!("ignored test{origin}"),
        None,
        "an ignored test is a failing check nobody runs: fix the test, or delete it",
    );
}

fn report_include(cx: &EarlyContext<'_>, span: Span) {
    span_lint_and_help(
        cx,
        DISABLED_CODE,
        span,
        "code brought in by `include!`",
        None,
        "write the code in a module, so every check reads it",
    );
}

/// Finds code written in a file of the crate that isn't one of its modules, which only
/// `include!`, under whatever name, puts there.
struct OutsideModules<'a, 'cx> {
    cx: &'a EarlyContext<'cx>,
    source: &'a LocalSource,
    /// The files already reported, by their first byte.
    reported: Vec<rustc_span::BytePos>,
}

impl OutsideModules<'_, '_> {
    fn check(&mut self, span: Span) {
        if span.is_dummy() || span.from_expansion() {
            return;
        }
        if self.source.files.iter().any(|file| file.contains(span)) {
            return;
        }
        let file = self.cx.sess().source_map().lookup_source_file(span.lo());
        if self.reported.contains(&file.start_pos) {
            return;
        }
        self.reported.push(file.start_pos);
        let path = match &file.name {
            FileName::Real(name) => name.local_path().and_then(|path| path.canonicalize().ok()),
            _ => None,
        };
        let named_by_include = self
            .source
            .includes
            .iter()
            .any(|include| include.file.as_deref() == path.as_deref());
        if path.is_some() && named_by_include {
            return;
        }
        report_include(self.cx, span.shrink_to_lo());
    }
}

impl<'ast> Visitor<'ast> for OutsideModules<'_, '_> {
    fn visit_item(&mut self, item: &'ast Item) {
        self.check(item.span);
        visit::walk_item(self, item);
    }

    fn visit_assoc_item(&mut self, item: &'ast AssocItem, context: visit::AssocCtxt) {
        self.check(item.span);
        visit::walk_assoc_item(self, item, context);
    }

    fn visit_expr(&mut self, expr: &'ast Expr) {
        self.check(expr.span);
        visit::walk_expr(self, expr);
    }
}

/// Checks `meta`, `attribute` itself or an attribute inside its `cfg_attr`.
fn check_attribute(cx: &EarlyContext<'_>, meta: &Meta, attribute: &SourceAttribute) {
    let span = attribute.span;
    if meta.path == "ignore" {
        report_ignore(cx, span, "");
    } else if is_test_attribute(&meta.path) {
        check_test_arguments(cx, meta);
    } else if meta.path == "cfg"
        && let [predicate] = meta.items()
    {
        check_predicate(cx, predicate, span, "cfg");
        check_target(cx, predicate, span, "cfg");
        let in_tests = attribute.in_test || attribute.target == Target::Module;
        if names(predicate, &["test"]).is_some() && !(in_tests && predicate.requires_test()) {
            report_test(cx, span, "cfg", attribute.in_test);
        }
    } else if let Some((predicate, attributes)) = meta.cfg_attr() {
        let only_lint_levels = attributes
            .iter()
            .all(|attribute| is_lint_level(&attribute.path));
        let only_windows_subsystem =
            matches!(attributes, [attribute] if attribute.path == "windows_subsystem");
        if always_false(predicate) {
            report_always_false(cx, span, "cfg_attr");
        } else if has_fragment(predicate) {
            report_fragment(cx, span, "cfg_attr");
        } else if let Some(name) = names(predicate, LINT_RUN_CFGS)
            && !only_lint_levels
        {
            report_lint_run(cx, span, "cfg_attr", name);
        } else if let Some(name) = names(predicate, BUILD_PROFILE_CFGS)
            && !only_lint_levels
            && !only_windows_subsystem
        {
            report_build_profile(cx, span, "cfg_attr", name);
        }
        if !only_lint_levels {
            check_target(cx, predicate, span, "cfg_attr");
            if names(predicate, &["test"]).is_some()
                && !(attribute.in_test && predicate.requires_test())
            {
                report_test(cx, span, "cfg_attr", attribute.in_test);
            }
        }
        for guarded in attributes {
            check_attribute(cx, guarded, attribute);
        }
    }
}

/// Whether an attribute with the path `path` marks a test: `#[test]`, or an attribute whose last
/// segment is `test` or ends in `_test`, such as `#[tokio::test]` or `#[wasm_bindgen_test]`.
fn is_test_attribute(path: &str) -> bool {
    let last = path.rsplit("::").next().unwrap_or(path);
    last == "test" || last.ends_with("_test")
}

/// Checks the arguments of `meta`, a test attribute, which takes none: an argument can skip the
/// test, rerun it until it passes, or run code before its failure is reported.
fn check_test_arguments(cx: &EarlyContext<'_>, meta: &Meta) {
    let arguments: &[Meta] = match &meta.kind {
        MetaKind::List(arguments) => arguments,
        MetaKind::Word => &[],
        _ => {
            report_test_argument(
                cx,
                meta.span,
                "arguments the lints can't read",
                TEST_ARGUMENT_HELP,
            );
            return;
        }
    };
    for argument in arguments {
        let name = if argument.path.is_empty() {
            "an argument the lints can't read".to_owned()
        } else {
            format!("the argument `{}`", argument.path)
        };
        report_test_argument(cx, argument.span, &name, TEST_ARGUMENT_HELP);
    }
}

const TEST_ARGUMENT_HELP: &str = "a test attribute takes no arguments, so the test runs once and \
                                every failure is reported: drop the argument";

/// Reports `what`, given to a test attribute at `span`.
fn report_test_argument(cx: &EarlyContext<'_>, span: Span, what: &str, help: &'static str) {
    span_lint_and_help(
        cx,
        DISABLED_CODE,
        span,
        format!("test attribute with {what}, which can skip the test or hide a failure"),
        None,
        help,
    );
}

/// Checks that `predicate`, of a `construct` at `span`, names only targets CI builds, as
/// `ci_targets` in `dylint.toml` lists them, and holds on at least one of them.
fn check_target(cx: &EarlyContext<'_>, predicate: &Meta, span: Span, construct: &str) {
    let targets = &crate::config::get(cx.sess()).ci_targets;
    let listed = listed_targets(cx);
    if let Some(name) = unbuilt_target(predicate, targets) {
        span_lint_and_help(
            cx,
            DISABLED_CODE,
            span,
            if UNBUILT_TARGET_CFGS.iter().any(|cfg| name.starts_with(cfg)) {
                format!("`{construct}` predicate on `{name}`, which CI doesn't build every way")
            } else {
                format!("`{construct}` predicate on `{name}`, a target CI doesn't build")
            },
            None,
            format!(
                "CI builds one target for each system `ci_targets` in `dylint.toml` lists \
                 ({listed}), so no check compiles code that only other targets or \
                 architectures build: name only the `target_os` of a listed system, or `unix` or \
                 `windows` when it covers one"
            ),
        );
    } else if !always_false(predicate) && holds_on_no_target(cx, predicate) {
        report_no_target(cx, span, construct);
    }
}

/// The systems `ci_targets` in `dylint.toml` lists, for a report.
fn listed_targets(cx: &EarlyContext<'_>) -> String {
    crate::config::get(cx.sess())
        .ci_targets
        .iter()
        .map(|target| format!("`{}`", target.os()))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Whether `predicate` is false on every target `ci_targets` in `dylint.toml` lists.
fn holds_on_no_target(cx: &EarlyContext<'_>, predicate: &Meta) -> bool {
    crate::config::get(cx.sess())
        .ci_targets
        .iter()
        .all(|&target| on_target(predicate, target) == Some(false))
}

/// Reports a predicate of a `construct` at `span` that holds on no target CI builds.
fn report_no_target(cx: &EarlyContext<'_>, span: Span, construct: &str) {
    let listed = listed_targets(cx);
    span_lint_and_help(
        cx,
        DISABLED_CODE,
        span,
        format!("`{construct}` predicate that holds on no target CI builds"),
        None,
        format!(
            "CI builds only the systems `ci_targets` in `dylint.toml` lists ({listed}), so no \
             check compiles what it guards: delete that code, or list the system it is for"
        ),
    );
}

/// The first part of `predicate` that names a target CI doesn't build, as written.
fn unbuilt_target(predicate: &Meta, targets: &[CiTarget]) -> Option<String> {
    match (predicate.path.as_str(), &predicate.kind) {
        ("not" | "all" | "any", MetaKind::List(items)) => {
            items.iter().find_map(|item| unbuilt_target(item, targets))
        }
        (name, kind) => {
            if UNBUILT_TARGET_CFGS.contains(&name) || name.starts_with("target_has_atomic") {
                return Some(name.to_owned());
            }
            let built = |value: &str| match name {
                "target_os" => targets.iter().any(|target| target.os() == value),
                _ => targets.iter().any(|target| target.family() == value),
            };
            match (name, kind) {
                ("unix" | "windows", MetaKind::Word) => (!built(name)).then(|| name.to_owned()),
                ("target_os" | "target_family", MetaKind::Value(Some(value))) => {
                    (!built(value)).then(|| format!("{name} = \"{value}\""))
                }
                ("target_os" | "target_family", _) => Some(name.to_owned()),
                _ => None,
            }
        }
    }
}

/// The value of `predicate` on `target`, if the target alone decides it.
fn on_target(predicate: &Meta, target: CiTarget) -> Option<bool> {
    match (predicate.path.as_str(), &predicate.kind) {
        ("not", MetaKind::List(items)) => match items.as_slice() {
            [item] => on_target(item, target).map(|value| !value),
            _ => None,
        },
        ("all", MetaKind::List(items)) => {
            let values: Vec<Option<bool>> =
                items.iter().map(|item| on_target(item, target)).collect();
            if values.contains(&Some(false)) {
                Some(false)
            } else {
                values
                    .iter()
                    .all(|value| *value == Some(true))
                    .then_some(true)
            }
        }
        ("any", MetaKind::List(items)) => {
            let values: Vec<Option<bool>> =
                items.iter().map(|item| on_target(item, target)).collect();
            if values.contains(&Some(true)) {
                Some(true)
            } else {
                values
                    .iter()
                    .all(|value| *value == Some(false))
                    .then_some(false)
            }
        }
        ("unix" | "windows", MetaKind::Word) => Some(target.family() == predicate.path),
        ("target_os", MetaKind::Value(Some(value))) => Some(target.os() == value),
        ("target_family", MetaKind::Value(Some(value))) => Some(target.family() == value),
        _ => None,
    }
}

/// Reports a predicate on `test` of a `construct` at `span`, which is in test code if `in_test`.
fn report_test(cx: &EarlyContext<'_>, span: Span, construct: &str, in_test: bool) {
    if in_test {
        span_lint_and_help(
            cx,
            DISABLED_CODE,
            span,
            format!("`{construct}` predicate on `test` that can hold outside tests, in test code"),
            None,
            "test code builds only in tests, so `not(test)` hides the code behind it from every \
             build: write `cfg(test)`, or remove the predicate",
        );
    } else {
        span_lint_and_help(
            cx,
            DISABLED_CODE,
            span,
            format!("`{construct}` predicate on `test` outside test code"),
            None,
            "code that changes under `cfg(test)` makes the tests check something other than \
             what ships: make the code the same in tests, and put test-only code in a \
             `#[cfg(test)]` module",
        );
    }
}

fn check_predicate(cx: &EarlyContext<'_>, predicate: &Meta, span: Span, construct: &str) {
    if always_false(predicate) {
        report_always_false(cx, span, construct);
    } else if has_fragment(predicate) {
        report_fragment(cx, span, construct);
    } else if let Some(name) = names(predicate, LINT_RUN_CFGS) {
        report_lint_run(cx, span, construct, name);
    } else if let Some(name) = names(predicate, BUILD_PROFILE_CFGS) {
        report_build_profile(cx, span, construct, name);
    }
}

fn report_always_false(cx: &EarlyContext<'_>, span: Span, construct: &str) {
    span_lint_and_help(
        cx,
        DISABLED_CODE,
        span,
        format!("`{construct}` predicate that is false in every build"),
        None,
        "no build compiles or runs what it guards: delete that code, or fix it and remove the \
         predicate",
    );
}

fn report_fragment(cx: &EarlyContext<'_>, span: Span, construct: &str) {
    span_lint_and_help(
        cx,
        DISABLED_CODE,
        span,
        format!("`{construct}` predicate written as a macro variable"),
        None,
        "only the macro's caller knows the predicate, so no check can tell which builds hold the \
         code: write the predicate out",
    );
}

fn report_lint_run(cx: &EarlyContext<'_>, span: Span, construct: &str, name: &str) {
    span_lint_and_help(
        cx,
        DISABLED_CODE,
        span,
        format!("`{construct}` predicate on `{name}`"),
        None,
        "code behind it escapes clippy, `trellis_lints`, or both: make it the same in every \
         build",
    );
}

fn report_build_profile(cx: &EarlyContext<'_>, span: Span, construct: &str, name: &str) {
    span_lint_and_help(
        cx,
        DISABLED_CODE,
        span,
        format!("`{construct}` predicate on `{name}`"),
        None,
        "the lints and tests run debug builds with every feature on, so code behind it ships in \
         release builds unchecked, or never ships: make it the same in every build",
    );
}

/// Whether `predicate` is false whatever the build's configuration.
fn always_false(predicate: &Meta) -> bool {
    evaluate(predicate) == Some(false)
}

/// The value of `predicate` if it is the same in every build.
fn evaluate(predicate: &Meta) -> Option<bool> {
    match (predicate.path.as_str(), &predicate.kind) {
        ("true", MetaKind::Word) => Some(true),
        ("false", MetaKind::Word) => Some(false),
        ("not", MetaKind::List(items)) => match items.as_slice() {
            [item] => evaluate(item).map(|value| !value),
            _ => None,
        },
        ("all", MetaKind::List(items)) => {
            let values: Vec<Option<bool>> = items.iter().map(evaluate).collect();
            if values.contains(&Some(false)) || contradicts_itself(items) {
                Some(false)
            } else {
                values
                    .iter()
                    .all(|value| *value == Some(true))
                    .then_some(true)
            }
        }
        ("any", MetaKind::List(items)) => {
            let values: Vec<Option<bool>> = items.iter().map(evaluate).collect();
            if values.contains(&Some(true)) {
                Some(true)
            } else {
                values
                    .iter()
                    .all(|value| *value == Some(false))
                    .then_some(false)
            }
        }
        _ => None,
    }
}

/// Whether the predicates `items`, all of which must hold, can't all hold on one target: `unix`
/// and `windows`, a predicate and its `not`, or two values of a key a target has one value of.
fn contradicts_itself(items: &[Meta]) -> bool {
    let words: Vec<&str> = items
        .iter()
        .filter(|item| matches!(item.kind, MetaKind::Word))
        .map(|item| item.path.as_str())
        .collect();
    if words.contains(&"unix") && words.contains(&"windows") {
        return true;
    }
    let negated: Vec<&Meta> = items
        .iter()
        .filter(|item| item.path == "not")
        .filter_map(|item| match item.items() {
            [inner] => Some(inner),
            _ => None,
        })
        .collect();
    let same = |left: &Meta, right: &Meta| match (&left.kind, &right.kind) {
        (MetaKind::Word, MetaKind::Word) => left.path == right.path,
        (MetaKind::Value(Some(left_value)), MetaKind::Value(Some(right_value))) => {
            left.path == right.path && left_value == right_value
        }
        _ => false,
    };
    if negated
        .iter()
        .any(|inner| items.iter().any(|item| same(item, inner)))
    {
        return true;
    }
    let values: Vec<(&str, &str)> = items
        .iter()
        .filter(|item| SINGLE_VALUED_CFGS.contains(&item.path.as_str()))
        .filter_map(|item| match &item.kind {
            MetaKind::Value(Some(value)) => Some((item.path.as_str(), value.as_str())),
            _ => None,
        })
        .collect();
    values.iter().any(|&(key, value)| {
        values
            .iter()
            .any(|&(other_key, other_value)| key == other_key && value != other_value)
    })
}

/// The first `cfg` name of `cfgs` that `predicate` mentions.
fn names<'c>(predicate: &Meta, cfgs: &[&'c str]) -> Option<&'c str> {
    match (predicate.path.as_str(), &predicate.kind) {
        ("not" | "all" | "any", MetaKind::List(items)) => {
            items.iter().find_map(|item| names(item, cfgs))
        }
        (name, _) => cfgs.iter().find(|&&cfg| cfg == name).copied(),
    }
}

/// Whether `predicate` is, or holds, a macro variable.
fn has_fragment(predicate: &Meta) -> bool {
    match &predicate.kind {
        MetaKind::Fragment => true,
        MetaKind::List(items) => items.iter().any(has_fragment),
        _ => false,
    }
}

fn is_lint_level(path: &str) -> bool {
    matches!(path, "allow" | "warn" | "expect" | "deny" | "forbid")
}

#[test]
fn ui() {
    crate::ui_harness::ui_test_of(DISABLED_CODE, "ui/disabled_code")
        .dylint_toml(crate::ui_harness::dylint_toml_with(
            "ci_targets",
            Some(r#"["linux", "macos", "windows"]"#),
        ))
        .run();
}

/// With only Linux in `ci_targets`, a predicate on Windows or macOS, or one that holds on no
/// Linux build, is flagged.
#[test]
fn ui_linux() {
    crate::ui_harness::ui_test_of(DISABLED_CODE, "ui/disabled_code_linux")
        .dylint_toml(crate::ui_harness::dylint_toml_with(
            "ci_targets",
            Some(r#"["linux"]"#),
        ))
        .run();
}

/// With Linux and macOS, `unix` holds on every build, so `not(unix)` is flagged.
#[test]
fn ui_unix() {
    crate::ui_harness::ui_test_of(DISABLED_CODE, "ui/disabled_code_unix")
        .dylint_toml(crate::ui_harness::dylint_toml_with(
            "ci_targets",
            Some(r#"["linux", "macos"]"#),
        ))
        .run();
}

#[test]
fn finds_cfg_code() {
    assert_eq!(cfg_code("#[cfg(test)] fn f() {}"), Some("`cfg` attribute"));
    assert_eq!(cfg_code("{ #![cfg(unix)] }"), Some("`cfg` attribute"));
    assert_eq!(
        cfg_code("# /* c */ [ r#cfg_attr(unix, allow(x))]"),
        Some("`cfg_attr` attribute")
    );
    assert_eq!(
        cfg_code("#[allow(x)] #[cfg_attr(unix, inline)]"),
        Some("`cfg_attr` attribute")
    );
    assert_eq!(
        cfg_code("{ std::cfg_select /* c */ ! { unix => {} } }"),
        Some("`cfg_select!`")
    );
    assert_eq!(cfg_code("#[allow(warnings)] fn f() {}"), None);
    assert_eq!(cfg_code("cfg(test) [cfg] #cfg"), None);
    assert_eq!(cfg_code("#[config(test)]"), None);
    assert_eq!(cfg_code("cfg_select is a macro"), None);
    assert_eq!(
        cfg_code("[(); { if cfg!(test) { 1 } else { 2 } }]"),
        Some("`cfg!`")
    );
    assert_eq!(cfg_code("a cfg (test) or cfg"), None);
    assert_eq!(cfg_code("[(); { let cfg = 2; (cfg != 1) as usize }]"), None);
    assert_eq!(cfg_code("cfg ! [test]"), Some("`cfg!`"));
}
