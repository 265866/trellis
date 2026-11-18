// edition:2024
// force-host
// no-prefer-dynamic
//! A dependency whose derive writes paths through modules from the caller's tokens, as
//! `#[derive(thiserror::Error)]` writes `::core::convert::From` at the span of a `#[from]`.

#![crate_type = "proc-macro"]

extern crate proc_macro;

use proc_macro::{Group, Span, TokenStream, TokenTree};

/// Writes `impl ::core::convert::From<u32> for Type`, every token at the span of the type's name.
#[proc_macro_derive(Convert)]
pub fn derive_convert(item: TokenStream) -> TokenStream {
    let name = type_name(&item);
    let span = name.span();
    let output: TokenStream = format!(
        "impl ::core::convert::From<u32> for {name} {{ fn from(value: u32) -> Self {{ Self(value) }} }}"
    )
    .parse()
    .unwrap();
    respan(output, span)
}

/// The name of the struct `item` defines.
fn type_name(item: &TokenStream) -> proc_macro::Ident {
    let mut trees = item.clone().into_iter();
    while let Some(tree) = trees.next() {
        if let TokenTree::Ident(keyword) = &tree
            && keyword.to_string() == "struct"
            && let Some(TokenTree::Ident(name)) = trees.next()
        {
            return name;
        }
    }
    panic!("`Convert` derives only on a struct")
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
