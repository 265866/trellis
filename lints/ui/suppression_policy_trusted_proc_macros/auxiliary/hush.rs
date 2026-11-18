// edition:2024
// force-host
// no-prefer-dynamic
//! A dependency whose proc macros write `allow` attributes into the code that uses them, made of
//! tokens that keep the caller's spans, as `#[tokio::main]` and `#[tracing::instrument]` do so
//! that errors point at the caller's code.

#![crate_type = "proc-macro"]

extern crate proc_macro;

use proc_macro::{Group, Span, TokenStream, TokenTree};

/// Writes `#[allow(dead_code)]` on the item it is on.
#[proc_macro_attribute]
pub fn hush(_: TokenStream, item: TokenStream) -> TokenStream {
    let span = first_span(&item);
    let mut output = allow("dead_code", span);
    output.extend(item);
    output
}

/// Writes an impl of the type with a function under `#[allow(dead_code)]`.
#[proc_macro_derive(Hush)]
pub fn derive_hush(item: TokenStream) -> TokenStream {
    let name = type_name(&item);
    let span = name.span();
    let mut output = TokenStream::new();
    output.extend(respan("impl".parse().unwrap(), span));
    output.extend([TokenTree::Ident(name)]);
    let mut body = allow("dead_code", span);
    body.extend(respan("fn hushed() {}".parse().unwrap(), span));
    output.extend([TokenTree::Group(Group::new(proc_macro::Delimiter::Brace, body))]);
    output
}

/// Writes its argument, a block, under `#[allow(unused_variables)]`.
#[proc_macro]
pub fn hushed_block(input: TokenStream) -> TokenStream {
    let span = first_span(&input);
    let mut output = allow("unused_variables", span);
    output.extend(input);
    output
}

fn allow(lint: &str, span: Span) -> TokenStream {
    respan(format!("#[allow({lint})]").parse().unwrap(), span)
}

fn first_span(stream: &TokenStream) -> Span {
    stream
        .clone()
        .into_iter()
        .next()
        .map_or_else(Span::call_site, |tree| tree.span())
}

/// The name of the struct or enum `item` defines.
fn type_name(item: &TokenStream) -> proc_macro::Ident {
    let mut trees = item.clone().into_iter();
    while let Some(tree) = trees.next() {
        if let TokenTree::Ident(keyword) = &tree
            && matches!(keyword.to_string().as_str(), "struct" | "enum")
            && let Some(TokenTree::Ident(name)) = trees.next()
        {
            return name;
        }
    }
    panic!("`Hush` derives only on a struct or an enum")
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
