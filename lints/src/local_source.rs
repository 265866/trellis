//! The crate's own Rust source files, tokenized once, and what the source-reading lints look for
//! in them: attributes, `cfg!` predicates, `cfg_select!` arms, `include!` invocations, doctests
//! marked `ignore`, and doc comments written after attributes.
//!
//! The lints that check comments, suppressions and disabled code read source text rather than the
//! AST: the compiler drops an attribute whose `cfg_attr` predicate is false, and an item whose
//! `cfg` is false, before any lint sees it, and the predicate is false in exactly the build that
//! would have to catch it.
//!
//! The files are the crate root and every module file a `mod name;` declaration in them names,
//! found by reading the declarations in the source text and resolving their paths the way the
//! compiler does, so a module file is read even when a `cfg` on its declaration, or an inner
//! `#![cfg]` in the file, leaves it out of the build. Module files the AST shows the crate loaded
//! are read too, which covers a `mod` that a macro writes; a macro's `mod` under a false `cfg` is
//! the one module file not read. Code that a macro builds from tokens is read only as the macro's
//! source, which is why `suppression_policy` rejects attributes whose path is a macro variable,
//! and every token sequence that could become an attribute the scan can't read: a `#` that
//! doesn't start an attribute, an attribute that doesn't start with a path, and a `$name:tt`
//! variable followed by brackets. [`crate::attribute_origin`] checks the attributes macros build
//! after expansion, against what this scan read. A file that `include!` brings in isn't a module
//! and isn't read; `disabled_code` flags every `include!` instead.
//!
//! Only these source-reading lints see code a `cfg` leaves out. The other lints, and clippy, see
//! what the build compiles, which is why `disabled_code` rejects `cfg` on `debug_assertions`,
//! `feature` and `panic`, and why CI checks all three platforms.

use std::collections::HashMap;
use std::ops::Range;
use std::path::{self, Path, PathBuf};
use std::sync::{Arc, OnceLock};

use rustc_ast::visit::{self, Visitor};
use rustc_ast::{AttrId, Attribute, Crate, Inline, Item, ItemKind, ModKind};
use rustc_lexer::{DocStyle, FrontmatterAllowed, LiteralKind, TokenKind};
use rustc_lint::{EarlyContext, LintContext};
use rustc_span::def_id::LOCAL_CRATE;
use rustc_span::source_map::SourceMap;
use rustc_span::{BytePos, ExpnId, FileName, SourceFile, Span};

use crate::code_before_main::Constructor;

/// An attribute, `#[...]` or `#![...]`, as written in a source file.
pub(crate) struct SourceAttribute {
    /// Whether it is an inner attribute, `#![...]`, which applies to the enclosing module, crate
    /// or function.
    pub(crate) inner: bool,
    /// The whole attribute, from `#` to `]`.
    pub(crate) span: Span,
    /// What is inside the brackets.
    pub(crate) meta: Meta,
    /// What an outer attribute is written on; `Target::Other` for an inner one.
    pub(crate) target: Target,
    /// Whether the attribute is inside test-only code: a module under `#[cfg(test)]`, or the body
    /// of a `#[test]` function. The attribute that makes a module test-only is outside it.
    pub(crate) in_test: bool,
    /// The index, among the file's significant tokens, of the `#`.
    pound: usize,
    /// The index, among the file's significant tokens, of the keyword of the item an outer
    /// attribute is written on, past any other attributes, visibility and qualifiers.
    item: Option<usize>,
    /// The index, among the file's significant tokens, of the `]` that closes the attribute.
    close: usize,
}

/// Tokens that may become an attribute once a macro expands, which the scan can't read as one.
pub(crate) struct AssembledAttribute {
    pub(crate) span: Span,
    pub(crate) kind: Assembled,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Assembled {
    /// `#[...]` or `#![...]` whose contents don't start with a path, such as `#[[<al low>](..)]`,
    /// which `paste` turns into `#[allow(..)]`.
    NoPath,
    /// A `#` not followed by `[` or `![`, such as one passed to a macro, which can put brackets
    /// after it.
    LoosePound,
    /// A `$name` bound as `:tt` followed by brackets, `$name [..]` or `$name ! [..]`, or a
    /// repetition of such variables followed by them, which is an attribute when the caller
    /// passes `#`.
    TokenTreeBeforeBrackets,
}

/// A meta item the scan read, at the start of an attribute or inside a `cfg_attr`, as
/// [`LocalSource::written`] finds it.
pub(crate) struct WrittenMeta {
    pub(crate) path: String,
    /// Where the meta item ends.
    pub(crate) end: BytePos,
    /// Whether it is inside a `cfg_attr` rather than the whole attribute.
    pub(crate) in_cfg_attr: bool,
}

/// The kind of item an outer attribute is written on, as far as the tokens after it show.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Target {
    /// `mod name;` or `mod name { ... }`.
    Module,
    /// `impl ... { ... }`.
    Impl,
    /// `trait Name { ... }`.
    Trait,
    /// A macro variable, `$item`, in a macro's body, whose item only the macro's caller knows.
    Fragment,
    /// Anything else.
    Other,
}

/// A meta item: `path`, `path(items, ...)` or `path = value`, as in attributes and `cfg`
/// predicates.
#[derive(Clone)]
pub(crate) struct Meta {
    /// The path, with segments joined by `::` and no whitespace, or empty when the item doesn't
    /// start with one.
    pub(crate) path: String,
    pub(crate) kind: MetaKind,
    pub(crate) span: Span,
}

#[derive(Clone)]
pub(crate) enum MetaKind {
    /// `path` alone.
    Word,
    /// `path(items, ...)`.
    List(Vec<Meta>),
    /// `path = value`, with the contents of the value if it is a single string literal.
    Value(Option<String>),
    /// A macro variable, `$name` or `$(...)`, in a macro's body, which stands for tokens only the
    /// macro's caller knows.
    Fragment,
    /// A fragment in a macro's matcher, `$name:kind`, which matches the caller's tokens.
    Matcher,
    /// Tokens that don't form a meta item.
    Other,
}

impl Meta {
    /// The items of `path(items, ...)`, or nothing for any other form.
    pub(crate) fn items(&self) -> &[Meta] {
        match &self.kind {
            MetaKind::List(items) => items,
            _ => &[],
        }
    }

    /// The items of `cfg_attr(predicate, attributes, ...)`, split into the predicate and the
    /// attributes it guards, if this is a `cfg_attr` with a predicate.
    pub(crate) fn cfg_attr(&self) -> Option<(&Meta, &[Meta])> {
        if self.path != "cfg_attr" {
            return None;
        }
        self.items().split_first()
    }

    /// Whether this predicate is exactly `dylint_lib = "trellis_lints"`, which is true only in
    /// the run of `trellis_lints`.
    pub(crate) fn is_own_lint_run(&self) -> bool {
        self.path == "dylint_lib"
            && matches!(&self.kind, MetaKind::Value(Some(value)) if value == env!("CARGO_PKG_NAME"))
    }

    /// Whether this is `cfg(test)`, or `cfg(all(.., test, ..))`, which leave what they are on
    /// out of every build but tests.
    pub(crate) fn is_cfg_test(&self) -> bool {
        self.path == "cfg" && matches!(self.items(), [predicate] if predicate.requires_test())
    }

    /// Whether this predicate is `test`, or `all(.., test, ..)`, which hold only in tests.
    pub(crate) fn requires_test(&self) -> bool {
        let is_test = |meta: &Meta| meta.path == "test" && matches!(meta.kind, MetaKind::Word);
        is_test(self) || self.path == "all" && self.items().iter().any(is_test)
    }

    /// Whether this predicate is exactly `not(test)`, which holds in every build but tests.
    pub(crate) fn is_not_test(&self) -> bool {
        let is_test = |meta: &Meta| meta.path == "test" && matches!(meta.kind, MetaKind::Word);
        self.path == "not" && matches!(self.items(), [inner] if is_test(inner))
    }

    /// The value of a `path = "value"` item.
    fn string_value(&self) -> Option<&str> {
        match &self.kind {
            MetaKind::Value(Some(value)) => Some(value),
            _ => None,
        }
    }
}

/// A token of a source file, comments and whitespace included, by its byte range in the file.
pub(crate) struct RawToken {
    pub(crate) kind: TokenKind,
    pub(crate) range: Range<usize>,
}

/// One source file of the crate, tokenized.
pub(crate) struct SourceText {
    start: BytePos,
    pub(crate) source: Arc<String>,
    pub(crate) tokens: Vec<RawToken>,
}

impl SourceText {
    /// The span of the bytes `range` of the file.
    pub(crate) fn span(&self, range: Range<usize>) -> Span {
        let at = |offset: usize| {
            self.start + BytePos(u32::try_from(offset).expect("a source file fits in a `u32`"))
        };
        Span::with_root_ctxt(at(range.start), at(range.end))
    }

    /// Whether `span` starts in this file.
    pub(crate) fn contains(&self, span: Span) -> bool {
        let end = self.start + BytePos(u32::try_from(self.source.len()).unwrap_or(u32::MAX));
        (self.start..=end).contains(&span.lo())
    }
}

/// An arm of a `cfg_select!`, `predicate => ..`.
pub(crate) struct CfgSelectArm {
    /// The arm, from its predicate to the end of its code.
    pub(crate) span: Span,
    /// The predicate, or `None` for the catch-all `_`.
    pub(crate) predicate: Option<Meta>,
}

/// An `include!(...)` invocation.
pub(crate) struct Include {
    /// The invocation, from `include` to its closing delimiter.
    pub(crate) span: Span,
    /// The file it names, if its argument is a string literal naming a file that exists.
    pub(crate) file: Option<PathBuf>,
}

/// The source files of the crate being checked, and what the source-reading lints look for in
/// them.
#[derive(Default)]
pub(crate) struct LocalSource {
    pub(crate) files: Vec<SourceText>,
    pub(crate) attributes: Vec<SourceAttribute>,
    /// The span of each `cfg!(...)` invocation and its predicate.
    pub(crate) cfg_macros: Vec<(Span, Meta)>,
    /// The arms of each `cfg_select! { .. }` invocation, in order.
    pub(crate) cfg_selects: Vec<Vec<CfgSelectArm>>,
    pub(crate) includes: Vec<Include>,
    /// The opening fence line of each code block in a doc comment that rustdoc would run as a
    /// doctest but for an `ignore` attribute.
    pub(crate) ignored_doctests: Vec<Span>,
    /// The first line of each outer doc comment written right after an outer attribute.
    pub(crate) docs_after_attributes: Vec<Span>,
    /// Tokens a macro can turn into an attribute that the scan can't read as one.
    pub(crate) assembled: Vec<AssembledAttribute>,
    /// Each meta item at the start of an attribute or inside a `cfg_attr`, by where it starts.
    written_metas: HashMap<BytePos, WrittenMeta>,
    /// The expansion that wrote the innermost item around each attribute of the expanded crate
    /// that is inside an item a macro wrote, as [`crate::attribute_origin::item_expansions`]
    /// finds them.
    item_expansions: HashMap<(AttrId, Span), ExpnId>,
    /// The constructors under each lint level of the expanded crate on an item that holds one, as
    /// [`crate::code_before_main::constructors_under`] finds them.
    constructors_under: HashMap<(AttrId, Span), Vec<Constructor>>,
    /// Each call of a function-like macro written in the source, `name!(..)`, `name![..]` or
    /// `name! {..}`, from its name to its closing delimiter.
    macro_calls: Vec<Span>,
}

/// The directory a module's `mod name;` declarations resolve against, as the compiler tracks it,
/// and whether the module is test-only.
#[derive(Clone)]
struct ModuleDir {
    dir: PathBuf,
    /// The module's name when its file is `name.rs` rather than `mod.rs` or the crate root, whose
    /// child modules then live in `name/`.
    relative: Option<String>,
    /// Whether the module, or one it is inside, is declared under `#[cfg(test)]`.
    test_only: bool,
}

impl LocalSource {
    /// The crate's source, read and tokenized the first time a lint asks for it.
    ///
    /// Each compiler process compiles one crate, as `config` also relies on, so the first read
    /// serves every lint pass in the process.
    pub(crate) fn get(cx: &EarlyContext<'_>, krate: &Crate) -> &'static Self {
        static SOURCE: OnceLock<LocalSource> = OnceLock::new();
        SOURCE.get_or_init(|| Self::read(cx, krate))
    }

    fn read(cx: &EarlyContext<'_>, krate: &Crate) -> Self {
        let source_map = cx.sess().source_map();
        let mut reader = Reader {
            source_map,
            loaded: loaded_files(source_map),
            read: Vec::new(),
            queue: Vec::new(),
            local: Self::default(),
        };
        let root = source_map.lookup_source_file(krate.spans.inner_span.lo());
        if let Some(path) = local_path(&root) {
            let dir = path.parent().unwrap_or(Path::new("")).to_owned();
            reader.enqueue(
                root,
                ModuleDir {
                    dir,
                    relative: None,
                    test_only: false,
                },
            );
        }
        reader.drain();
        let mut modules = AstModules {
            source_map,
            files: Vec::new(),
        };
        visit::walk_crate(&mut modules, krate);
        for file in modules.files {
            let Some(path) = local_path(&file) else {
                continue;
            };
            let dir = path.parent().unwrap_or(Path::new("")).to_owned();
            let relative = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .filter(|&stem| stem != "mod")
                .map(str::to_owned);
            reader.enqueue(
                file,
                ModuleDir {
                    dir,
                    relative,
                    test_only: false,
                },
            );
        }
        reader.drain();
        let mut local = reader.local;
        let mut written_metas = HashMap::new();
        for attribute in &local.attributes {
            add_written(&attribute.meta, false, &mut written_metas);
        }
        local.written_metas = written_metas;
        local.item_expansions = crate::attribute_origin::item_expansions(krate);
        local.constructors_under = crate::code_before_main::constructors_under(krate);
        local.macro_calls = local.files.iter().flat_map(macro_calls).collect();
        local
    }

    /// Whether `span` lies inside a call of a function-like macro written in the source.
    pub(crate) fn in_macro_call(&self, span: Span) -> bool {
        self.macro_calls
            .iter()
            .any(|call| call.lo() <= span.lo() && span.hi() <= call.hi())
    }

    /// The span of the string literal written in the source that holds `span`, if one does. A
    /// macro that parses code from a string, as serde parses the type of `#[serde(into = "..")]`,
    /// gives that code the string's span.
    pub(crate) fn string_literal_around(&self, span: Span) -> Option<Span> {
        let file = self.files.iter().find(|file| file.contains(span))?;
        file.tokens
            .iter()
            .filter(|token| {
                matches!(
                    token.kind,
                    TokenKind::Literal {
                        kind: LiteralKind::Str { .. }
                            | LiteralKind::RawStr { .. }
                            | LiteralKind::ByteStr { .. }
                            | LiteralKind::RawByteStr { .. }
                            | LiteralKind::CStr { .. }
                            | LiteralKind::RawCStr { .. },
                        ..
                    }
                )
            })
            .map(|token| file.span(token.range.clone()))
            .find(|literal| literal.lo() <= span.lo() && span.hi() <= literal.hi())
    }

    /// The expansion that wrote the innermost item around `attribute`, if a macro wrote that item.
    pub(crate) fn item_expansion(&self, attribute: &Attribute) -> Option<ExpnId> {
        self.item_expansions
            .get(&(attribute.id, attribute.span))
            .copied()
    }

    /// The statics in a constructor section that `expansion` wrote, if `attribute`, a lint level,
    /// is on an item that holds any.
    pub(crate) fn constructors_under(
        &self,
        attribute: &Attribute,
        expansion: ExpnId,
    ) -> Vec<Constructor> {
        self.constructors_under
            .get(&(attribute.id, attribute.span))
            .into_iter()
            .flatten()
            .filter(|constructor| constructor.expansion == expansion)
            .copied()
            .collect()
    }

    /// The meta item the scan read that starts where `path_span` starts, if its path is `path`:
    /// one written as an attribute, inside a `cfg_attr` or in a macro's body. An attribute that
    /// a macro assembles from other tokens has none.
    pub(crate) fn written(&self, path: &str, path_span: Span) -> Option<&WrittenMeta> {
        self.written_metas
            .get(&path_span.lo())
            .filter(|written| written.path == path)
    }
}

/// The span of each call of a function-like macro in `file`: a name, `!` and a delimited group,
/// other than `macro_rules!`, whose name isn't followed by a group.
fn macro_calls(file: &SourceText) -> Vec<Span> {
    let tokens: Vec<&RawToken> = file
        .tokens
        .iter()
        .filter(|token| {
            !matches!(
                token.kind,
                TokenKind::Whitespace
                    | TokenKind::LineComment { .. }
                    | TokenKind::BlockComment { .. }
                    | TokenKind::Frontmatter { .. }
            )
        })
        .collect();
    let opens = |kind: TokenKind| {
        matches!(
            kind,
            TokenKind::OpenParen | TokenKind::OpenBracket | TokenKind::OpenBrace
        )
    };
    let closes = |kind: TokenKind| {
        matches!(
            kind,
            TokenKind::CloseParen | TokenKind::CloseBracket | TokenKind::CloseBrace
        )
    };
    let mut calls = Vec::new();
    for (index, window) in tokens.windows(3).enumerate() {
        let [name, bang, open] = window else {
            continue;
        };
        if !matches!(name.kind, TokenKind::Ident | TokenKind::RawIdent)
            || bang.kind != TokenKind::Bang
            || !opens(open.kind)
        {
            continue;
        }
        let mut depth = 0_usize;
        for token in &tokens[index + 2..] {
            if opens(token.kind) {
                depth += 1;
            } else if closes(token.kind) {
                depth -= 1;
                if depth == 0 {
                    calls.push(file.span(name.range.start..token.range.end));
                    break;
                }
            }
        }
    }
    calls
}

/// Adds `meta` and the attributes of any `cfg_attr` it is to `written`.
fn add_written(meta: &Meta, in_cfg_attr: bool, written: &mut HashMap<BytePos, WrittenMeta>) {
    written.insert(
        meta.span.lo(),
        WrittenMeta {
            path: meta.path.clone(),
            end: meta.span.hi(),
            in_cfg_attr,
        },
    );
    if let Some((_, attributes)) = meta.cfg_attr() {
        for attribute in attributes {
            add_written(attribute, true, written);
        }
    }
}

/// The crate's real source files the compiler has loaded so far, by canonical path.
fn loaded_files(source_map: &SourceMap) -> HashMap<PathBuf, Arc<SourceFile>> {
    let mut loaded = HashMap::new();
    for file in source_map.files().iter() {
        if file.cnum != LOCAL_CRATE {
            continue;
        }
        if let Some(path) = local_path(file).and_then(|path| path.canonicalize().ok()) {
            loaded.entry(path).or_insert_with(|| Arc::clone(file));
        }
    }
    loaded
}

fn local_path(file: &SourceFile) -> Option<&Path> {
    match &file.name {
        FileName::Real(name) => name.local_path(),
        _ => None,
    }
}

/// Reads module files breadth-first from the crate root, each once.
struct Reader<'a> {
    source_map: &'a SourceMap,
    loaded: HashMap<PathBuf, Arc<SourceFile>>,
    /// The canonical path of each file read or queued.
    read: Vec<PathBuf>,
    queue: Vec<(Arc<SourceFile>, ModuleDir)>,
    local: LocalSource,
}

impl Reader<'_> {
    fn enqueue(&mut self, file: Arc<SourceFile>, dir: ModuleDir) {
        let Some(path) = local_path(&file).and_then(|path| path.canonicalize().ok()) else {
            return;
        };
        if file.cnum != LOCAL_CRATE || self.read.contains(&path) {
            return;
        }
        self.read.push(path);
        self.queue.push((file, dir));
    }

    /// Enqueues the module file at `path`, which the compiler may not have loaded if a `cfg`
    /// left its module out; loading it gives its text spans that diagnostics can point into.
    fn enqueue_path(&mut self, path: &Path, dir: ModuleDir) {
        let Ok(canonical) = path.canonicalize() else {
            return;
        };
        let file = match self.loaded.get(&canonical) {
            Some(file) => Arc::clone(file),
            None => match self.source_map.load_file(path) {
                Ok(file) => file,
                Err(_) => return,
            },
        };
        self.enqueue(file, dir);
    }

    fn drain(&mut self) {
        while !self.queue.is_empty() {
            let (file, dir) = self.queue.remove(0);
            let Some(source) = file.src.clone() else {
                continue;
            };
            let text = SourceText {
                start: file.start_pos,
                tokens: raw_tokens(&source),
                source,
            };
            let tokens = Tokens::new(&text);
            let mut attributes = tokens.collect_into(&mut self.local, local_path(&file));
            let test_regions = tokens.test_regions(&attributes);
            let in_test = |index: usize| {
                dir.test_only
                    || test_regions
                        .iter()
                        .any(|&(open, close)| (open..close).contains(&index))
            };
            for attribute in &mut attributes {
                attribute.in_test = in_test(attribute.pound);
            }
            for (path, dir) in tokens.module_files(&attributes, &dir, in_test) {
                self.enqueue_path(&path, dir);
            }
            self.local.attributes.extend(attributes);
            self.local.files.push(text);
        }
    }
}

/// Collects the file of each module the crate loaded from a file of its own.
struct AstModules<'a> {
    source_map: &'a SourceMap,
    files: Vec<Arc<SourceFile>>,
}

impl<'ast> Visitor<'ast> for AstModules<'_> {
    fn visit_item(&mut self, item: &'ast Item) {
        if let ItemKind::Mod(_, _, ModKind::Loaded(_, Inline::No { .. }, spans)) = &item.kind
            && !spans.inner_span.is_dummy()
            && !spans.inner_span.from_expansion()
        {
            self.files
                .push(self.source_map.lookup_source_file(spans.inner_span.lo()));
        }
        visit::walk_item(self, item);
    }
}

fn raw_tokens(source: &str) -> Vec<RawToken> {
    let mut tokens = Vec::new();
    let mut offset = 0;
    for token in rustc_lexer::tokenize(source, FrontmatterAllowed::No) {
        let start = offset;
        offset += token.len as usize;
        tokens.push(RawToken {
            kind: token.kind,
            range: start..offset,
        });
    }
    tokens
}

#[derive(Clone, Copy)]
struct Token {
    kind: TokenKind,
    start: u32,
    end: u32,
}

/// The significant tokens of one file, with each opening delimiter matched to its closing one.
struct Tokens<'s> {
    text: &'s SourceText,
    significant: Vec<Token>,
    /// For each token that opens a group, the index of the token that closes it; an unclosed group
    /// runs to the end of the file.
    closes: Vec<Option<usize>>,
}

/// A doc comment line's style, and the number of significant tokens before it, which tells
/// whether two doc comments belong to the same item.
#[derive(Clone, Copy, PartialEq, Eq)]
struct DocPosition {
    style: DocStyle,
    after: usize,
}

/// An open fenced code block in a doc comment: its fence character and length.
struct Fence {
    character: char,
    length: usize,
}

impl<'s> Tokens<'s> {
    fn new(text: &'s SourceText) -> Self {
        let mut tokens = Vec::new();
        for token in &text.tokens {
            if !matches!(
                token.kind,
                TokenKind::Whitespace
                    | TokenKind::LineComment { .. }
                    | TokenKind::BlockComment { .. }
                    | TokenKind::Frontmatter { .. }
            ) {
                tokens.push(Token {
                    kind: token.kind,
                    start: u32::try_from(token.range.start).unwrap_or(u32::MAX),
                    end: u32::try_from(token.range.end).unwrap_or(u32::MAX),
                });
            }
        }
        let mut closes = vec![None; tokens.len()];
        let mut open = Vec::new();
        for (index, token) in tokens.iter().enumerate() {
            match token.kind {
                TokenKind::OpenParen | TokenKind::OpenBracket | TokenKind::OpenBrace => {
                    open.push(index);
                }
                TokenKind::CloseParen | TokenKind::CloseBracket | TokenKind::CloseBrace => {
                    if let Some(opener) = open.pop() {
                        closes[opener] = Some(index);
                    }
                }
                _ => {}
            }
        }
        Self {
            text,
            significant: tokens,
            closes,
        }
    }

    /// Adds what the file holds to `local`, except its attributes, which it returns for
    /// [`Self::module_files`] to read first.
    fn collect_into(&self, local: &mut LocalSource, path: Option<&Path>) -> Vec<SourceAttribute> {
        let mut attributes = Vec::new();
        let token_tree_variables = self.token_tree_variables();
        let mut assembled = |span: Span, kind: Assembled| {
            local.assembled.push(AssembledAttribute { span, kind });
        };
        for index in 0..self.significant.len() {
            match self.significant[index].kind {
                TokenKind::Pound => match self.brackets_after(index) {
                    None => assembled(self.span(index, index), Assembled::LoosePound),
                    Some(open) => match self.attribute_at(index, open) {
                        Some(attribute) => {
                            if attribute.meta.path.is_empty()
                                && matches!(attribute.meta.kind, MetaKind::Other)
                            {
                                assembled(attribute.span, Assembled::NoPath);
                            }
                            attributes.push(attribute);
                        }
                        None => assembled(
                            self.span(index, self.close(open).min(self.significant.len() - 1)),
                            Assembled::NoPath,
                        ),
                    },
                },
                TokenKind::Dollar => {
                    if let Some(last) =
                        self.token_tree_before_brackets(index, &token_tree_variables)
                    {
                        assembled(self.span(index, last), Assembled::TokenTreeBeforeBrackets);
                    }
                }
                TokenKind::Ident | TokenKind::RawIdent => match self.name(index) {
                    "cfg" => local.cfg_macros.extend(self.cfg_macro_at(index)),
                    "cfg_select" => local.cfg_selects.extend(self.cfg_select_at(index)),
                    "include" => local.includes.extend(self.include_at(index, path)),
                    _ => {}
                },
                _ => {}
            }
        }
        self.collect_doc_comments(local, &attributes);
        attributes
    }

    /// The index of the `[` that opens an attribute after the `#` at `pound`, past a `!`, if one
    /// follows it.
    fn brackets_after(&self, pound: usize) -> Option<usize> {
        let open = if self.kind(pound + 1) == Some(TokenKind::Bang) {
            pound + 2
        } else {
            pound + 1
        };
        (self.kind(open) == Some(TokenKind::OpenBracket)).then_some(open)
    }

    /// The token range of each `macro_rules!` body in the file, with the names of the variables
    /// it binds as `:tt`.
    fn token_tree_variables(&self) -> Vec<(usize, usize, Vec<&'s str>)> {
        let mut macros = Vec::new();
        for index in 0..self.significant.len() {
            if self.name(index) != "macro_rules"
                || self.kind(index + 1) != Some(TokenKind::Bang)
                || !matches!(
                    self.kind(index + 3),
                    Some(TokenKind::OpenParen | TokenKind::OpenBracket | TokenKind::OpenBrace)
                )
            {
                continue;
            }
            let open = index + 3;
            let close = self.close(open);
            let names = (open..close)
                .filter(|&at| {
                    self.kind(at) == Some(TokenKind::Dollar)
                        && self.kind(at + 1) == Some(TokenKind::Ident)
                        && self.kind(at + 2) == Some(TokenKind::Colon)
                        && self.name(at + 3) == "tt"
                })
                .map(|at| self.name(at + 1))
                .collect();
            macros.push((open, close, names));
        }
        macros
    }

    /// The index of the last token of the brackets after the `$` at `dollar`, if it starts a
    /// variable bound as `:tt`, `$name`, or a repetition holding one, `$(..)*`, that brackets
    /// follow, as `$name [..]` or `$name ! [..]`.
    fn token_tree_before_brackets(
        &self,
        dollar: usize,
        macros: &[(usize, usize, Vec<&str>)],
    ) -> Option<usize> {
        let (_, _, names) = macros
            .iter()
            .rev()
            .find(|(open, close, _)| (*open..*close).contains(&dollar))?;
        let is_bound = |at: usize| {
            self.kind(at) == Some(TokenKind::Dollar)
                && self.kind(at + 1) == Some(TokenKind::Ident)
                && names.contains(&self.name(at + 1))
        };
        let after = match self.kind(dollar + 1)? {
            TokenKind::Ident if is_bound(dollar) => dollar + 2,
            TokenKind::OpenParen => {
                let close = self.close(dollar + 1);
                if !(dollar + 2..close).any(is_bound) {
                    return None;
                }
                let operator = |at: usize| {
                    matches!(
                        self.kind(at),
                        Some(TokenKind::Star | TokenKind::Plus | TokenKind::Question)
                    )
                };
                if operator(close + 1) {
                    close + 2
                } else if operator(close + 2) {
                    close + 3
                } else {
                    return None;
                }
            }
            _ => return None,
        };
        let open = if self.kind(after) == Some(TokenKind::Bang) {
            after + 1
        } else {
            after
        };
        (self.kind(open) == Some(TokenKind::OpenBracket))
            .then(|| self.close(open).min(self.significant.len() - 1))
    }

    /// The token ranges of the file's test-only code, by the attributes in it: the body of each
    /// inline module under `#[cfg(test)]` and of each `#[test]` function, `#[test]` meaning any
    /// attribute whose path ends in `test`, such as `#[tokio::test]`.
    fn test_regions(&self, attributes: &[SourceAttribute]) -> Vec<(usize, usize)> {
        let mut regions = Vec::new();
        for attribute in attributes {
            let Some(item) = attribute.item else {
                continue;
            };
            let meta = &attribute.meta;
            let is_test_function = matches!(meta.kind, MetaKind::Word | MetaKind::List(_))
                && (meta.path == "test" || meta.path.ends_with("::test"));
            if attribute.target == Target::Module && meta.is_cfg_test() {
                if self.kind(item + 2) == Some(TokenKind::OpenBrace) {
                    regions.push((item + 2, self.close(item + 2)));
                }
            } else if is_test_function && let Some(body) = self.body_after(item) {
                regions.push((body, self.close(body)));
            }
        }
        regions
    }

    /// The index of the `{` that opens the body of the item whose first token is at `item`,
    /// past its parameters and any other group, or nothing if a `;` ends the item first.
    fn body_after(&self, item: usize) -> Option<usize> {
        let mut index = item;
        loop {
            match self.kind(index)? {
                TokenKind::OpenBrace => return Some(index),
                TokenKind::Semi => return None,
                TokenKind::OpenParen | TokenKind::OpenBracket => index = self.close(index) + 1,
                _ => index += 1,
            }
        }
    }

    /// The attribute starting with the `#` at `pound`, whose `[` is at `open`, if its brackets
    /// hold anything.
    fn attribute_at(&self, pound: usize, open: usize) -> Option<SourceAttribute> {
        let inner = open == pound + 2;
        let close = self.close(open);
        let meta = self.meta(open + 1, close)?;
        let item = if inner { None } else { self.item_after(close) };
        let target = match item {
            Some(index) => match (self.kind(index), self.name(index)) {
                (Some(TokenKind::Ident), "mod") => Target::Module,
                (Some(TokenKind::Ident), "impl") => Target::Impl,
                (Some(TokenKind::Ident), "trait") => Target::Trait,
                (Some(TokenKind::Dollar), _) if self.name(index + 1) != "crate" => Target::Fragment,
                _ => Target::Other,
            },
            None => Target::Other,
        };
        Some(SourceAttribute {
            inner,
            span: self.span(pound, close.min(self.significant.len() - 1)),
            meta,
            target,
            in_test: false,
            pound,
            item,
            close,
        })
    }

    /// The index of the first token after the attribute closing at `close` that isn't another
    /// outer attribute, a visibility or an item qualifier such as `unsafe`.
    fn item_after(&self, close: usize) -> Option<usize> {
        let mut index = close + 1;
        loop {
            match self.kind(index)? {
                TokenKind::Pound if self.kind(index + 1) == Some(TokenKind::OpenBracket) => {
                    index = self.close(index + 1) + 1;
                }
                TokenKind::Ident
                    if matches!(
                        self.name(index),
                        "pub" | "unsafe" | "safe" | "default" | "auto" | "const"
                    ) =>
                {
                    index += 1;
                    if self.name(index - 1) == "pub"
                        && self.kind(index) == Some(TokenKind::OpenParen)
                    {
                        index = self.close(index) + 1;
                    }
                }
                _ => return Some(index),
            }
        }
    }

    /// The `cfg!(...)` invocation starting with the `cfg` at `name`, if one does.
    fn cfg_macro_at(&self, name: usize) -> Option<(Span, Meta)> {
        let open = self.macro_open(name)?;
        let close = self.close(open);
        let predicate = self.meta(open + 1, close)?;
        Some((
            self.span(name, close.min(self.significant.len() - 1)),
            predicate,
        ))
    }

    /// The arms of the `cfg_select! { .. }` invocation starting with the `cfg_select` at `name`,
    /// if one does. An arm whose predicate isn't followed by `=>` ends the arms with one whose
    /// predicate is the tokens the lint couldn't read, which reports them.
    fn cfg_select_at(&self, name: usize) -> Option<Vec<CfgSelectArm>> {
        let open = self.macro_open(name)?;
        let close = self.close(open).min(self.significant.len());
        let mut arms = Vec::new();
        let mut index = open + 1;
        while index < close {
            let start = index;
            let mut arrow = None;
            while index < close {
                match self.significant[index].kind {
                    TokenKind::OpenParen | TokenKind::OpenBracket | TokenKind::OpenBrace => {
                        index = self.close(index) + 1;
                    }
                    TokenKind::Eq if self.kind(index + 1) == Some(TokenKind::Gt) => {
                        arrow = Some(index);
                        break;
                    }
                    _ => index += 1,
                }
            }
            let Some(arrow) = arrow else {
                arms.push(CfgSelectArm {
                    span: self.span(start, close.saturating_sub(1).max(start)),
                    predicate: Some(Meta {
                        path: String::new(),
                        kind: MetaKind::Other,
                        span: self.span(start, close.saturating_sub(1).max(start)),
                    }),
                });
                break;
            };
            let predicate = if arrow == start + 1 && self.text(start) == "_" {
                None
            } else {
                Some(self.meta(start, arrow).unwrap_or(Meta {
                    path: String::new(),
                    kind: MetaKind::Other,
                    span: self.span(start, arrow),
                }))
            };
            index = arrow + 2;
            if self.kind(index) == Some(TokenKind::OpenBrace) {
                index = self.close(index);
            } else {
                while index < close && self.kind(index) != Some(TokenKind::Comma) {
                    if matches!(
                        self.significant[index].kind,
                        TokenKind::OpenParen | TokenKind::OpenBracket | TokenKind::OpenBrace
                    ) {
                        index = self.close(index);
                    }
                    index += 1;
                }
                index = index.saturating_sub(1);
            }
            arms.push(CfgSelectArm {
                span: self.span(start, index.min(close - 1)),
                predicate,
            });
            index += 1;
            if self.kind(index) == Some(TokenKind::Comma) {
                index += 1;
            }
        }
        Some(arms)
    }

    /// The `include!(...)` invocation starting with the `include` at `name`, if one does, in the
    /// file at `path`.
    fn include_at(&self, name: usize, path: Option<&Path>) -> Option<Include> {
        let open = self.macro_open(name)?;
        let close = self.close(open);
        let file = (open + 2 == close)
            .then(|| self.string_literal(open + 1))
            .flatten()
            .zip(path.and_then(Path::parent))
            .and_then(|(name, dir)| dir.join(name).canonicalize().ok());
        Some(Include {
            span: self.span(name, close.min(self.significant.len() - 1)),
            file,
        })
    }

    /// The index of the opening delimiter of the macro invocation whose name is at `name`.
    fn macro_open(&self, name: usize) -> Option<usize> {
        if self.kind(name + 1) != Some(TokenKind::Bang) {
            return None;
        }
        matches!(
            self.kind(name + 2)?,
            TokenKind::OpenParen | TokenKind::OpenBracket | TokenKind::OpenBrace
        )
        .then_some(name + 2)
    }

    /// The files of the modules declared with `mod name;` in this file, of a module whose
    /// declarations resolve against `dir`, each with the directory its own declarations resolve
    /// against. A declaration under a `cfg_attr(.., path = "..")` names both the `path` file and
    /// the default one, so both are read. A module is test-only if `in_test` holds for its `mod`
    /// token or `#[cfg(test)]` is on its declaration.
    fn module_files(
        &self,
        attributes: &[SourceAttribute],
        dir: &ModuleDir,
        in_test: impl Fn(usize) -> bool,
    ) -> Vec<(PathBuf, ModuleDir)> {
        let mut inline: Vec<(usize, usize, ModuleDir)> = Vec::new();
        let mut files = Vec::new();
        for index in 0..self.significant.len() {
            if self.kind(index) != Some(TokenKind::Ident) || self.name(index) != "mod" {
                continue;
            }
            if !matches!(
                self.kind(index + 1),
                Some(TokenKind::Ident | TokenKind::RawIdent)
            ) {
                continue;
            }
            let name = self.name(index + 1).to_owned();
            let parent = inline
                .iter()
                .rev()
                .find(|(open, close, _)| (*open..*close).contains(&index))
                .map_or(dir, |(_, _, dir)| dir);
            let mut paths = Vec::new();
            let mut conditional_paths = Vec::new();
            let mut test_only = in_test(index);
            for attribute in attributes
                .iter()
                .filter(|attribute| attribute.item == Some(index))
            {
                path_values(&attribute.meta, false, &mut paths, &mut conditional_paths);
                test_only |= attribute.meta.is_cfg_test();
            }
            let in_parent = |value: &str| {
                let value = if cfg!(windows) {
                    value.replace('/', path::MAIN_SEPARATOR_STR)
                } else {
                    value.to_owned()
                };
                parent.dir.join(value)
            };
            let own_dir = |relative: Option<&str>| -> PathBuf {
                match relative {
                    Some(relative) => parent.dir.join(relative),
                    None => parent.dir.clone(),
                }
            };
            match self.kind(index + 2) {
                Some(TokenKind::OpenBrace) => {
                    let dir = match paths.first().or(conditional_paths.first()) {
                        Some(path) => in_parent(path),
                        None => own_dir(parent.relative.as_deref()).join(&name),
                    };
                    inline.push((
                        index + 2,
                        self.close(index + 2),
                        ModuleDir {
                            dir,
                            relative: None,
                            test_only,
                        },
                    ));
                }
                Some(TokenKind::Semi) => {
                    for path in paths.iter().chain(&conditional_paths) {
                        let file = in_parent(path);
                        let dir = file.parent().unwrap_or(Path::new("")).to_owned();
                        files.push((
                            file,
                            ModuleDir {
                                dir,
                                relative: None,
                                test_only,
                            },
                        ));
                    }
                    if paths.is_empty() {
                        let dir = own_dir(parent.relative.as_deref());
                        files.push((
                            dir.join(format!("{name}.rs")),
                            ModuleDir {
                                dir: dir.clone(),
                                relative: Some(name.clone()),
                                test_only,
                            },
                        ));
                        files.push((
                            dir.join(&name).join("mod.rs"),
                            ModuleDir {
                                dir: dir.join(&name),
                                relative: None,
                                test_only,
                            },
                        ));
                    }
                }
                _ => {}
            }
        }
        files
    }

    /// Finds the code blocks in the file's doc comments that rustdoc runs as doctests but for an
    /// `ignore`, and the outer doc comments written after an outer attribute, of `attributes`.
    fn collect_doc_comments(&self, local: &mut LocalSource, attributes: &[SourceAttribute]) {
        let attribute_closes: Vec<usize> = attributes
            .iter()
            .filter(|attribute| !attribute.inner)
            .map(|attribute| attribute.close)
            .collect();
        let mut significant = 0;
        let mut previous: Option<DocPosition> = None;
        let mut fence: Option<Fence> = None;
        for token in &self.text.tokens {
            let style = match token.kind {
                TokenKind::LineComment {
                    doc_style: Some(style),
                }
                | TokenKind::BlockComment {
                    doc_style: Some(style),
                    ..
                } => style,
                TokenKind::Whitespace
                | TokenKind::LineComment { .. }
                | TokenKind::BlockComment { .. } => continue,
                _ => {
                    significant += 1;
                    continue;
                }
            };
            let position = DocPosition {
                style,
                after: significant,
            };
            if previous != Some(position) {
                fence = None;
                if style == DocStyle::Outer
                    && significant > 0
                    && attribute_closes.contains(&(significant - 1))
                {
                    local
                        .docs_after_attributes
                        .push(self.text.span(token.range.clone()));
                }
            }
            previous = Some(position);
            let comment = &self.text.source[token.range.clone()];
            let block = matches!(token.kind, TokenKind::BlockComment { .. });
            let body = if block {
                comment
                    .get(3..comment.len().saturating_sub(2))
                    .unwrap_or("")
            } else {
                comment.get(3..).unwrap_or("")
            };
            for line in body.lines() {
                let line = if block {
                    let line = line.trim_start();
                    line.strip_prefix('*').unwrap_or(line)
                } else {
                    line
                };
                if update_fence(&mut fence, line) {
                    local
                        .ignored_doctests
                        .push(self.text.span(token.range.clone()));
                }
            }
        }
    }

    /// Parses the tokens in `start..end` as one meta item, or returns `None` if there are none.
    fn meta(&self, start: usize, end: usize) -> Option<Meta> {
        if start >= end {
            return None;
        }
        let span = self.span(start, end - 1);
        if self.significant[start].kind == TokenKind::Dollar {
            let is_matcher = end - start == 4
                && self.kind(start + 1) == Some(TokenKind::Ident)
                && self.kind(start + 2) == Some(TokenKind::Colon)
                && self.kind(start + 3) == Some(TokenKind::Ident);
            if is_matcher {
                return Some(Meta {
                    path: String::new(),
                    kind: MetaKind::Matcher,
                    span,
                });
            }
            if self.name(start + 1) != "crate" {
                return Some(Meta {
                    path: String::new(),
                    kind: MetaKind::Fragment,
                    span,
                });
            }
        }
        let separator = |index: usize| {
            index + 1 < end
                && self.significant[index].kind == TokenKind::Colon
                && self.significant[index + 1].kind == TokenKind::Colon
        };
        let mut path = String::new();
        let mut index = start;
        if separator(index) {
            path.push_str("::");
            index += 2;
        } else if self.significant[index].kind == TokenKind::Dollar {
            path.push_str("$crate");
            index += 2;
            if !separator(index) {
                return Some(Meta {
                    path,
                    kind: MetaKind::Other,
                    span,
                });
            }
            path.push_str("::");
            index += 2;
        }
        while index < end
            && matches!(
                self.significant[index].kind,
                TokenKind::Ident | TokenKind::RawIdent
            )
        {
            path.push_str(self.name(index));
            index += 1;
            if separator(index) {
                path.push_str("::");
                index += 2;
            } else {
                break;
            }
        }
        let kind = if path.is_empty() || path.ends_with("::") {
            MetaKind::Other
        } else if index == end {
            MetaKind::Word
        } else if self.significant[index].kind == TokenKind::OpenParen
            && self.close(index) + 1 == end
        {
            MetaKind::List(self.metas(index + 1, self.close(index)))
        } else if self.significant[index].kind == TokenKind::Eq {
            let value = (index + 2 == end)
                .then(|| self.string_literal(index + 1))
                .flatten();
            MetaKind::Value(value)
        } else {
            MetaKind::Other
        };
        Some(Meta { path, kind, span })
    }

    /// Parses the tokens in `start..end` as meta items separated by commas.
    fn metas(&self, start: usize, end: usize) -> Vec<Meta> {
        let mut items = Vec::new();
        let mut item_start = start;
        let mut index = start;
        while index < end {
            match self.significant[index].kind {
                TokenKind::OpenParen | TokenKind::OpenBracket | TokenKind::OpenBrace => {
                    index = self.close(index) + 1;
                }
                TokenKind::Comma => {
                    items.extend(self.meta(item_start, index));
                    index += 1;
                    item_start = index;
                }
                _ => index += 1,
            }
        }
        items.extend(self.meta(item_start, end.min(index)));
        items
    }

    /// The contents of the string literal at `index`, without escapes processed, if it is one.
    fn string_literal(&self, index: usize) -> Option<String> {
        let TokenKind::Literal { kind, suffix_start } = self.significant[index].kind else {
            return None;
        };
        let text = &self.text(index)[..suffix_start as usize];
        let contents = match kind {
            LiteralKind::Str { terminated: true } => text.strip_prefix('"')?.strip_suffix('"')?,
            LiteralKind::RawStr {
                n_hashes: Some(hashes),
            } => {
                let hashes = usize::from(hashes);
                text.get(2 + hashes..text.len() - 1 - hashes)?
            }
            _ => return None,
        };
        Some(contents.to_owned())
    }

    fn kind(&self, index: usize) -> Option<TokenKind> {
        self.significant.get(index).map(|token| token.kind)
    }

    /// The index of the token closing the group opened at `open`, or one past the last token if
    /// the group is never closed.
    fn close(&self, open: usize) -> usize {
        self.closes[open].unwrap_or(self.significant.len())
    }

    fn text(&self, index: usize) -> &'s str {
        let token = self.significant[index];
        &self.text.source[token.start as usize..token.end as usize]
    }

    /// The text of the token at `index` without a raw identifier's `r#`, or nothing past the end.
    fn name(&self, index: usize) -> &'s str {
        if index < self.significant.len() {
            self.text(index).trim_start_matches("r#")
        } else {
            ""
        }
    }

    fn span(&self, first: usize, last: usize) -> Span {
        self.text
            .span(self.significant[first].start as usize..self.significant[last].end as usize)
    }
}

/// Adds each `path = "..."` in `meta`, an attribute behind a `cfg_attr` if `conditional`, to
/// `paths` or `conditional_paths`.
fn path_values(
    meta: &Meta,
    conditional: bool,
    paths: &mut Vec<String>,
    conditional_paths: &mut Vec<String>,
) {
    if let Some((_, attributes)) = meta.cfg_attr() {
        for attribute in attributes {
            path_values(attribute, true, paths, conditional_paths);
        }
    } else if meta.path == "path"
        && let Some(value) = meta.string_value()
    {
        if conditional {
            conditional_paths.push(value.to_owned());
        } else {
            paths.push(value.to_owned());
        }
    }
}

/// Follows one line of a doc comment through the fenced code blocks of its Markdown, `fence`
/// holding the block the line is in, and returns whether the line opens a block that rustdoc
/// would run as a doctest but for an `ignore`.
fn update_fence(fence: &mut Option<Fence>, line: &str) -> bool {
    let line = line.trim_start();
    let Some(character) = line
        .chars()
        .next()
        .filter(|&first| first == '`' || first == '~')
    else {
        return false;
    };
    let length = line.chars().take_while(|&next| next == character).count();
    if length < 3 {
        return false;
    }
    let info = line[length..].trim();
    if let Some(open) = fence {
        if character == open.character && length >= open.length && info.is_empty() {
            *fence = None;
        }
        return false;
    }
    if character == '`' && info.contains('`') {
        return false;
    }
    *fence = Some(Fence { character, length });
    is_ignored_doctest(info)
}

/// Whether a code block with the info string `info` is Rust that rustdoc would test but for an
/// `ignore` or `ignore-<target>` attribute, following rustdoc's reading of the info string: a
/// block is Rust unless a word rustdoc doesn't know, such as `text`, comes before every word it
/// does.
fn is_ignored_doctest(info: &str) -> bool {
    let mut seen_rust = false;
    let mut seen_other = false;
    let mut ignored = false;
    for word in info
        .split(|character: char| character == ',' || character.is_whitespace())
        .filter(|word| !word.is_empty())
    {
        match word {
            "rust" => seen_rust = true,
            "ignore" => {
                ignored = true;
                seen_rust = !seen_other;
            }
            word if word.starts_with("ignore-") => {
                ignored = true;
                seen_rust = !seen_other;
            }
            "should_panic" | "no_run" | "compile_fail" | "test_harness" | "standalone_crate" => {
                seen_rust = !seen_other;
            }
            word if word.starts_with("edition") || is_error_code(word) => {
                seen_rust = !seen_other;
            }
            word if word.starts_with(['{', '.']) || word.contains(['=', '}']) => {}
            _ => seen_other = true,
        }
    }
    ignored && (!seen_other || seen_rust)
}

/// Whether `word` is an error code such as `E0499`, which rustdoc reads as a Rust attribute.
fn is_error_code(word: &str) -> bool {
    word.len() == 5 && word.starts_with('E') && word[1..].bytes().all(|byte| byte.is_ascii_digit())
}
