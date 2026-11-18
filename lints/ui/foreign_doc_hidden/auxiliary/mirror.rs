// edition:2024
// force-host
// no-prefer-dynamic
//! A dependency whose derive mirrors a type of another crate, as serde does for
//! `#[serde(remote = "...")]`: it builds that type and reads its fields at spans of its own,
//! naming each field with the caller's identifier for it.

#![crate_type = "proc-macro"]

extern crate proc_macro;

use proc_macro::{Group, Ident, Span, TokenStream, TokenTree};

/// For `#[mirror(remote = "<path>")]` over a struct, writes `impl Type { fn mirrored(value:
/// <path>) { .. } }`, which reads each of the struct's fields from `value`, builds a `<path>`
/// from them, and takes one apart with a pattern, every field named with the caller's
/// identifier. It also reads `value.own` with an identifier of its own.
#[proc_macro_derive(Mirror, attributes(mirror))]
pub fn derive_mirror(item: TokenStream) -> TokenStream {
    let mut trees = item.into_iter();
    let mut remote = None;
    let mut name = None;
    let mut fields = Vec::new();
    while let Some(tree) = trees.next() {
        match &tree {
            TokenTree::Punct(pound) if pound.as_char() == '#' => {
                if let Some(TokenTree::Group(attribute)) = trees.next() {
                    remote = remote.or_else(|| remote_path(&attribute));
                }
            }
            TokenTree::Ident(keyword) if keyword.to_string() == "struct" => {
                if let Some(TokenTree::Ident(ident)) = trees.next() {
                    name = Some(ident);
                }
            }
            TokenTree::Group(body) if name.is_some() => fields = field_names(body),
            _ => {}
        }
    }
    let name = name.expect("`Mirror` derives only on a struct");
    let remote = remote.expect("`Mirror` needs `#[mirror(remote = \"...\")]`");
    let mut reads = String::new();
    let mut built = String::new();
    let mut taken = String::new();
    for index in 0..fields.len() {
        reads.push_str(&format!("let _ = value.FIELD{index};"));
        built.push_str(&format!("FIELD{index}: value.FIELD{index},"));
        taken.push_str(&format!("FIELD{index}: _,"));
    }
    let output = format!(
        "impl {name} {{ fn mirrored(value: REMOTE) {{ {reads} let _ = value.own; \
         let value = REMOTE {{ {built} ..value }}; let REMOTE {{ {taken} .. }} = value; }} }}"
    );
    substitute(output.parse().unwrap(), &fields, &remote)
}

/// The path in `mirror(remote = "<path>")`, at the string's span.
fn remote_path(attribute: &Group) -> Option<TokenStream> {
    let mut inner = attribute.stream().into_iter();
    let (Some(TokenTree::Ident(path)), Some(TokenTree::Group(arguments))) =
        (inner.next(), inner.next())
    else {
        return None;
    };
    if path.to_string() != "mirror" {
        return None;
    }
    let arguments: Vec<TokenTree> = arguments.stream().into_iter().collect();
    let [TokenTree::Ident(key), _, TokenTree::Literal(literal)] = arguments.as_slice() else {
        return None;
    };
    if key.to_string() != "remote" {
        return None;
    }
    let text = literal.to_string();
    Some(respan(
        text.trim_matches('"').parse().unwrap(),
        literal.span(),
    ))
}

/// The caller's identifiers for the fields of the struct whose body is `body`.
fn field_names(body: &Group) -> Vec<Ident> {
    let trees: Vec<TokenTree> = body.stream().into_iter().collect();
    trees
        .windows(2)
        .filter_map(|pair| match pair {
            [TokenTree::Ident(field), TokenTree::Punct(colon)] if colon.as_char() == ':' => {
                Some(field.clone())
            }
            _ => None,
        })
        .collect()
}

/// `stream` with each `FIELD<n>` replaced by the caller's identifier for field `n`, and each
/// `REMOTE` by `remote`.
fn substitute(stream: TokenStream, fields: &[Ident], remote: &TokenStream) -> TokenStream {
    let mut output = TokenStream::new();
    for tree in stream {
        match tree {
            TokenTree::Group(group) => {
                let mut substituted =
                    Group::new(group.delimiter(), substitute(group.stream(), fields, remote));
                substituted.set_span(group.span());
                output.extend([TokenTree::Group(substituted)]);
            }
            TokenTree::Ident(ident) if ident.to_string() == "REMOTE" => {
                output.extend(remote.clone());
            }
            TokenTree::Ident(ident) if ident.to_string().starts_with("FIELD") => {
                let index: usize = ident.to_string()["FIELD".len()..].parse().unwrap();
                output.extend([TokenTree::Ident(fields[index].clone())]);
            }
            other => output.extend([other]),
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
