// edition:2024
// force-host
// no-prefer-dynamic
//! A dependency whose derive parses paths from strings its caller wrote, as serde does for
//! `#[serde(default = "...")]`, and writes paths of its own at a string's span, as thiserror does
//! at an `#[error("..")]` message.

#![crate_type = "proc-macro"]

extern crate proc_macro;

use proc_macro::{Group, Span, TokenStream, TokenTree};

/// For `#[defaulted(path = "...")]`, writes `impl Type { fn made() { let _ = <path>(); } }`, the
/// path at the string's span. For `#[defaulted(own = "...")]`, writes a call of the path the
/// string holds, `own` being the derive's own choice of what to call, at the string's span. For
/// `#[defaulted(with = "...")]`, writes a call of `<path>::deserialize`, the path at the string's
/// span and `deserialize` at the derive's, as serde does for `#[serde(with = "...")]`.
#[proc_macro_derive(Defaulted, attributes(defaulted))]
pub fn derive_defaulted(item: TokenStream) -> TokenStream {
    let mut trees = item.into_iter();
    let mut calls = TokenStream::new();
    let mut name = None;
    while let Some(tree) = trees.next() {
        match &tree {
            TokenTree::Punct(pound) if pound.as_char() == '#' => {
                if let Some(TokenTree::Group(attribute)) = trees.next() {
                    calls.extend(call(&attribute));
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
    let name = name.expect("`Defaulted` derives only on a struct");
    let mut output: TokenStream = "impl".parse().unwrap();
    output.extend([TokenTree::Ident(name)]);
    let mut body: TokenStream = "fn made()".parse().unwrap();
    body.extend([TokenTree::Group(Group::new(
        proc_macro::Delimiter::Brace,
        calls,
    ))]);
    output.extend([TokenTree::Group(Group::new(
        proc_macro::Delimiter::Brace,
        body,
    ))]);
    output
}

/// `let _ = <path>();` for the attribute `defaulted(path = "<path>")`, a call of the path `own`
/// names for `defaulted(own = "..")`, at the string's span, or of `<path>::deserialize` for
/// `defaulted(with = "<path>")`.
fn call(attribute: &Group) -> TokenStream {
    let mut inner = attribute.stream().into_iter();
    let (Some(TokenTree::Ident(path)), Some(TokenTree::Group(arguments))) =
        (inner.next(), inner.next())
    else {
        return TokenStream::new();
    };
    if path.to_string() != "defaulted" {
        return TokenStream::new();
    }
    let arguments: Vec<TokenTree> = arguments.stream().into_iter().collect();
    let [TokenTree::Ident(key), _, TokenTree::Literal(literal)] = arguments.as_slice() else {
        return TokenStream::new();
    };
    let text = literal.to_string();
    let path = match key.to_string().as_str() {
        "path" => text.trim_matches('"').to_owned(),
        "own" => OWN.to_owned(),
        "with" => {
            let mut call = respan("let _ =".parse().unwrap(), literal.span());
            call.extend(respan(
                text.trim_matches('"').parse().unwrap(),
                literal.span(),
            ));
            call.extend("::deserialize();".parse::<TokenStream>().unwrap());
            return call;
        }
        _ => return TokenStream::new(),
    };
    respan(
        format!("let _ = {path}();").parse().unwrap(),
        literal.span(),
    )
}

/// The path `own` calls, which the caller never wrote.
const OWN: &str = "::std::string::String::new";

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
