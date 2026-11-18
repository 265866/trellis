// edition:2024
// force-host
// no-prefer-dynamic
//! A dependency with a function-like macro that substitutes one identifier for another in its
//! input, as `duplicate!` does, so every token of its output keeps a span of its call.

#![crate_type = "proc-macro"]

extern crate proc_macro;

use proc_macro::{Group, Ident, TokenStream, TokenTree};

/// `swap! { from to tokens.. }` writes `tokens` with each `from` replaced by `to`.
#[proc_macro]
pub fn swap(input: TokenStream) -> TokenStream {
    let mut trees = input.into_iter();
    let (Some(TokenTree::Ident(from)), Some(TokenTree::Ident(to))) = (trees.next(), trees.next())
    else {
        panic!("`swap!` starts with the identifier to replace and its replacement")
    };
    substitute(trees.collect(), &from.to_string(), &to)
}

fn substitute(stream: TokenStream, from: &str, to: &Ident) -> TokenStream {
    stream
        .into_iter()
        .map(|tree| match tree {
            TokenTree::Ident(ident) if ident.to_string() == from => TokenTree::Ident(to.clone()),
            TokenTree::Group(group) => {
                let mut replaced =
                    Group::new(group.delimiter(), substitute(group.stream(), from, to));
                replaced.set_span(group.span());
                TokenTree::Group(replaced)
            }
            other => other,
        })
        .collect()
}
