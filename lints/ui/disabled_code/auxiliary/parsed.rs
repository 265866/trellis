// edition:2024
// force-host
// no-prefer-dynamic
//! A derive that, as serde does, parses code from strings its caller writes and gives the code the
//! string's span, and copies other strings into its output as they are.

#![crate_type = "proc-macro"]

extern crate proc_macro;

use proc_macro::{Delimiter, Group, Span, TokenStream, TokenTree};

/// Writes, for each `#[parsed(ty = "..")]`, a constant holding a `let` of the type the string
/// holds, at the string's span, as serde parses `#[serde(into = "..")]`; for each
/// `#[parsed(call = "..")]`, a function that calls the function the string names, with
/// parentheses of its own, as serde calls the function of `deserialize_with`; and, for each
/// `#[parsed(text = "..")]`, a function that matches the string, as it is, against itself, as
/// serde matches a `rename`.
#[proc_macro_derive(Parsed, attributes(parsed))]
pub fn derive_parsed(item: TokenStream) -> TokenStream {
    let mut output = TokenStream::new();
    let mut trees = item.into_iter();
    while let Some(tree) = trees.next() {
        let TokenTree::Punct(pound) = &tree else {
            continue;
        };
        if pound.as_char() != '#' {
            continue;
        }
        let Some(TokenTree::Group(attribute)) = trees.next() else {
            continue;
        };
        let mut inner = attribute.stream().into_iter();
        let (Some(TokenTree::Ident(path)), Some(TokenTree::Group(arguments))) =
            (inner.next(), inner.next())
        else {
            continue;
        };
        if path.to_string() != "parsed" {
            continue;
        }
        let mut arguments = arguments.stream().into_iter();
        let (Some(TokenTree::Ident(key)), Some(_), Some(TokenTree::Literal(literal))) =
            (arguments.next(), arguments.next(), arguments.next())
        else {
            continue;
        };
        let text = literal.to_string();
        let value = text[1..text.len() - 1].replace("\\\"", "\"");
        match key.to_string().as_str() {
            "ty" => {
                output.extend("const _: () =".parse::<TokenStream>().unwrap());
                output.extend(respan(
                    format!("{{ let _: Option<{value}> = None; }}").parse().unwrap(),
                    literal.span(),
                ));
                output.extend(";".parse::<TokenStream>().unwrap());
            }
            "call" => {
                let mut called = respan(value.parse().unwrap(), literal.span());
                called.extend("(())".parse::<TokenStream>().unwrap());
                let mut body = "core::mem::drop".parse::<TokenStream>().unwrap();
                body.extend([TokenTree::Group(Group::new(Delimiter::Parenthesis, called))]);
                body.extend(";".parse::<TokenStream>().unwrap());
                output.extend("fn _call()".parse::<TokenStream>().unwrap());
                output.extend([TokenTree::Group(Group::new(Delimiter::Brace, body))]);
            }
            "text" => output.extend(
                format!("const _: () = {{ fn _text() {{ match {text} {{ {text} => {{}} _ => {{}} }} }} }};")
                    .parse::<TokenStream>()
                    .unwrap()
                    .into_iter()
                    .map(|tree| match tree {
                        TokenTree::Literal(mut copied) => {
                            copied.set_span(literal.span());
                            TokenTree::Literal(copied)
                        }
                        other => other,
                    }),
            ),
            _ => {}
        }
    }
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
