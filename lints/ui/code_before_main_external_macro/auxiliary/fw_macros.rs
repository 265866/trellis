// edition:2024
// force-host
// no-prefer-dynamic
//! A framework whose `app!` takes the app's items as free tokens and writes them at its own call
//! site, as a proc macro that builds its output with `Span::call_site()` does, so the macros
//! called among them expand from `app!`.

#![crate_type = "proc-macro"]

extern crate proc_macro;

use proc_macro::{Group, Span, TokenStream, TokenTree};

#[proc_macro]
pub fn app(items: TokenStream) -> TokenStream {
    let mut output: TokenStream = "pub mod respanned_app".parse().unwrap();
    let body = Group::new(proc_macro::Delimiter::Brace, respan(items));
    output.extend([TokenTree::Group(body)]);
    output
}

fn respan(stream: TokenStream) -> TokenStream {
    stream
        .into_iter()
        .map(|tree| {
            let mut tree = match tree {
                TokenTree::Group(group) => {
                    TokenTree::Group(Group::new(group.delimiter(), respan(group.stream())))
                }
                other => other,
            };
            tree.set_span(Span::call_site());
            tree
        })
        .collect()
}
