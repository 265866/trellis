// edition:2024
// force-host
// no-prefer-dynamic
//! A dependency whose proc macros write lint levels over code their caller wrote, as
//! `#[derive(clap::Parser)]` does over the expressions in `#[arg(..)]` and
//! `#[tracing::instrument(err)]` over the function's body, and over code of their own that they
//! give the caller's spans, as `#[tokio::main]` does.

#![crate_type = "proc-macro"]

extern crate proc_macro;

use proc_macro::{Delimiter, Group, Literal, Span, TokenStream, TokenTree};

/// Writes `impl Type { fn count() -> u32 { <expr> } }` under `allow(clippy::restriction)`, where
/// `<expr>` is the argument of the type's `#[count(..)]`, keeping its spans.
#[proc_macro_derive(Count, attributes(count))]
pub fn derive_count(item: TokenStream) -> TokenStream {
    let mut trees = item.into_iter().peekable();
    let mut expression = TokenStream::new();
    let mut name = None;
    while let Some(tree) = trees.next() {
        match &tree {
            TokenTree::Punct(pound) if pound.as_char() == '#' => {
                if let Some(TokenTree::Group(attribute)) = trees.next() {
                    let mut inner = attribute.stream().into_iter();
                    if let (Some(TokenTree::Ident(path)), Some(TokenTree::Group(arguments))) =
                        (inner.next(), inner.next())
                        && path.to_string() == "count"
                    {
                        expression = arguments.stream();
                    }
                }
            }
            TokenTree::Ident(keyword) if keyword.to_string() == "struct" => {
                if let Some(TokenTree::Ident(ident)) = trees.next() {
                    name = Some(ident);
                }
            }
            _ => {}
        }
    }
    let name = name.expect("`Count` derives only on a struct");
    let mut output: TokenStream = "#[allow(clippy::restriction)] impl".parse().unwrap();
    output.extend([TokenTree::Ident(name)]);
    let mut body: TokenStream = "fn count() -> u32".parse().unwrap();
    body.extend([TokenTree::Group(Group::new(Delimiter::Brace, expression))]);
    output.extend([TokenTree::Group(Group::new(Delimiter::Brace, body))]);
    output
}

/// Writes, as serde's derives do, an impl under `allow(<level>)`, `unused_qualifications` unless the
/// type's `#[named(level = "..")]` names another, that holds code made of the caller's tokens:
/// the type's name as a path and as a string, each `#[named(rename = "..")]` string as it is,
/// the path each `#[named(path = "..")]` string holds, and, as serde's `with` does, the path each
/// `#[named(module = "..")]` string holds followed by `::label`, the same for
/// `#[named(joined = "..")]` with `::label` at the derive's own span, as serde writes the
/// `deserialize` it adds, and the type each
/// `#[named(ty = "..")]` string holds, at the string's span; an array whose length is the
/// expression of each `#[named_length(..)]`, keeping its spans, as serde copies a field's type;
/// and a function `fn bounded<'de, T>()` whose `where` clause is the bound each
/// `#[named(bound = "..")]` string holds, at the string's span, as serde writes a `bound`.
#[proc_macro_derive(Named, attributes(named, named_length))]
pub fn derive_named(item: TokenStream) -> TokenStream {
    let mut trees = item.into_iter();
    let mut level = "unused_qualifications".to_owned();
    let mut body = TokenStream::new();
    let mut bounded = TokenStream::new();
    let mut name = None;
    while let Some(tree) = trees.next() {
        match &tree {
            TokenTree::Punct(pound) if pound.as_char() == '#' => {
                let Some(TokenTree::Group(attribute)) = trees.next() else {
                    continue;
                };
                let mut inner = attribute.stream().into_iter();
                let (Some(TokenTree::Ident(path)), Some(TokenTree::Group(arguments))) =
                    (inner.next(), inner.next())
                else {
                    continue;
                };
                if path.to_string() == "named_length" {
                    let mut array: TokenStream = "0_u8;".parse().unwrap();
                    array.extend(arguments.stream());
                    body.extend("let _ =".parse::<TokenStream>().unwrap());
                    body.extend([TokenTree::Group(Group::new(Delimiter::Bracket, array))]);
                    body.extend(";".parse::<TokenStream>().unwrap());
                    continue;
                }
                let arguments: Vec<TokenTree> = arguments.stream().into_iter().collect();
                let [TokenTree::Ident(key), _, TokenTree::Literal(literal)] = arguments.as_slice()
                else {
                    continue;
                };
                if path.to_string() != "named" {
                    continue;
                }
                let text = literal.to_string().trim_matches('"').to_owned();
                match key.to_string().as_str() {
                    "level" => level = text,
                    "rename" => {
                        body.extend("let _ =".parse::<TokenStream>().unwrap());
                        body.extend([TokenTree::Literal(literal.clone())]);
                        body.extend(";".parse::<TokenStream>().unwrap());
                    }
                    "path" => body.extend(respan(
                        format!("let _ = {text}();").parse().unwrap(),
                        literal.span(),
                    )),
                    "module" => body.extend(respan(
                        format!("let _ = {text}::label();").parse().unwrap(),
                        literal.span(),
                    )),
                    "joined" => {
                        body.extend(respan(
                            format!("let _ = {text}").parse().unwrap(),
                            literal.span(),
                        ));
                        body.extend("::label();".parse::<TokenStream>().unwrap());
                    }
                    "ty" => body.extend(respan(
                        format!("let _: Option<{text}> = None;").parse().unwrap(),
                        literal.span(),
                    )),
                    "bound" => {
                        bounded.extend("where".parse::<TokenStream>().unwrap());
                        bounded.extend(respan(text.parse().unwrap(), literal.span()));
                    }
                    _ => {}
                }
            }
            TokenTree::Ident(keyword) if keyword.to_string() == "struct" => {
                if let Some(TokenTree::Ident(ident)) = trees.next() {
                    name = Some(ident);
                }
            }
            _ => {}
        }
    }
    let name = name.expect("`Named` derives only on a struct");
    let mut as_string = Literal::string(&name.to_string());
    as_string.set_span(name.span());
    body.extend("let _ =".parse::<TokenStream>().unwrap());
    body.extend([TokenTree::Ident(name.clone())]);
    body.extend("; let _ =".parse::<TokenStream>().unwrap());
    body.extend([TokenTree::Literal(as_string)]);
    body.extend(";".parse::<TokenStream>().unwrap());
    let mut output: TokenStream = format!("#[allow({level})] impl").parse().unwrap();
    output.extend([TokenTree::Ident(name)]);
    let mut function: TokenStream = "fn named()".parse().unwrap();
    function.extend([TokenTree::Group(Group::new(Delimiter::Brace, body))]);
    function.extend("fn bounded<'de, T>()".parse::<TokenStream>().unwrap());
    function.extend(bounded);
    function.extend([TokenTree::Group(Group::new(
        Delimiter::Brace,
        TokenStream::new(),
    ))]);
    output.extend([TokenTree::Group(Group::new(Delimiter::Brace, function))]);
    output
}

/// Writes `impl Type { fn forwarded(value: u8) -> u8 { match value { <name> => <name> } } }` under
/// `allow(deprecated)`, for the name the type's `#[forwarded(<name>)]` gives: the binding at the
/// derive's own span and the read at the name's, as thiserror binds `transparent` and reads it at
/// `#[error(transparent)]`.
#[proc_macro_derive(Forwarded, attributes(forwarded))]
pub fn derive_forwarded(item: TokenStream) -> TokenStream {
    let mut trees = item.into_iter();
    let mut binding = None;
    let mut name = None;
    while let Some(tree) = trees.next() {
        match &tree {
            TokenTree::Punct(pound) if pound.as_char() == '#' => {
                if let Some(TokenTree::Group(attribute)) = trees.next() {
                    let mut inner = attribute.stream().into_iter();
                    if let (Some(TokenTree::Ident(path)), Some(TokenTree::Group(arguments))) =
                        (inner.next(), inner.next())
                        && path.to_string() == "forwarded"
                    {
                        binding = arguments.stream().into_iter().next();
                    }
                }
            }
            TokenTree::Ident(keyword) if keyword.to_string() == "struct" => {
                if let Some(TokenTree::Ident(ident)) = trees.next() {
                    name = Some(ident);
                }
            }
            _ => {}
        }
    }
    let name = name.expect("`Forwarded` derives only on a struct");
    let binding = binding.expect("`Forwarded` needs `#[forwarded(<name>)]`");
    let mut own = binding.clone();
    own.set_span(Span::call_site());
    let mut arm = TokenStream::from(own);
    arm.extend("=>".parse::<TokenStream>().unwrap());
    arm.extend([binding]);
    let mut body: TokenStream = "match value".parse().unwrap();
    body.extend([TokenTree::Group(Group::new(Delimiter::Brace, arm))]);
    let mut function: TokenStream = "fn forwarded(value: u8) -> u8".parse().unwrap();
    function.extend([TokenTree::Group(Group::new(Delimiter::Brace, body))]);
    let mut output: TokenStream = "#[allow(deprecated)] impl".parse().unwrap();
    output.extend([TokenTree::Ident(name)]);
    output.extend([TokenTree::Group(Group::new(Delimiter::Brace, function))]);
    output
}

/// Writes the function's body inside `(move || <body>)()` under the lint level its argument
/// names, such as `allow(clippy::redundant_closure_call)`, keeping the body's spans.
#[proc_macro_attribute]
pub fn wrapped(level: TokenStream, item: TokenStream) -> TokenStream {
    let mut trees: Vec<TokenTree> = item.into_iter().collect();
    let Some(TokenTree::Group(body)) = trees.pop() else {
        panic!("`wrapped` goes on a function")
    };
    let mut closure: TokenStream = "move ||".parse().unwrap();
    closure.extend([TokenTree::Group(body)]);
    let mut wrapped: TokenStream = format!("#[{level}] let value =").parse().unwrap();
    wrapped.extend([TokenTree::Group(Group::new(Delimiter::Parenthesis, closure))]);
    wrapped.extend("(); value".parse::<TokenStream>().unwrap());
    let mut output: TokenStream = trees.into_iter().collect();
    output.extend([TokenTree::Group(Group::new(Delimiter::Brace, wrapped))]);
    output
}

/// Appends a block of its own under `allow(clippy::expect_used)` to the function's body, every
/// token of it at the span of the body's last token, as `#[tokio::main]` writes its runtime.
#[proc_macro_attribute]
pub fn runtime(_: TokenStream, item: TokenStream) -> TokenStream {
    let mut trees: Vec<TokenTree> = item.into_iter().collect();
    let Some(TokenTree::Group(body)) = trees.pop() else {
        panic!("`runtime` goes on a function")
    };
    let span = body
        .stream()
        .into_iter()
        .last()
        .map_or_else(Span::call_site, |tree| tree.span());
    let mut stream = body.stream();
    stream.extend(respan(
        "; #[allow(clippy::expect_used)] { Some(1_u8).expect(\"one\"); }"
            .parse()
            .unwrap(),
        span,
    ));
    let mut output: TokenStream = trees.into_iter().collect();
    output.extend([TokenTree::Group(Group::new(Delimiter::Brace, stream))]);
    output
}

/// `stream` with every token at `span`.
fn respan(stream: TokenStream, span: Span) -> TokenStream {
    stream
        .into_iter()
        .map(|tree| match tree {
            TokenTree::Group(group) => {
                let mut respanned = Group::new(group.delimiter(), respan(group.stream(), span));
                respanned.set_span(span);
                TokenTree::Group(respanned)
            }
            mut other => {
                other.set_span(span);
                other
            }
        })
        .collect()
}
