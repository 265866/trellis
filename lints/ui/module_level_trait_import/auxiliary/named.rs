// edition:2024
// force-host
// no-prefer-dynamic
//! A derive that implements a trait of the same name, as serde's `Deserialize` does, naming the
//! trait by its absolute path.

#![crate_type = "proc-macro"]

extern crate proc_macro;

use proc_macro::{TokenStream, TokenTree};

/// Writes `impl crate::facade::Named for Type { .. }`, whose `name` returns the type's name.
#[proc_macro_derive(Named)]
pub fn derive_named(item: TokenStream) -> TokenStream {
    let mut trees = item.into_iter();
    let mut name = None;
    while let Some(tree) = trees.next() {
        if let TokenTree::Ident(keyword) = &tree
            && keyword.to_string() == "struct"
        {
            name = trees.next();
        }
    }
    let name = name.expect("`Named` derives only on a struct");
    format!(
        "impl crate::facade::Named for {name} {{ fn name(&self) -> &'static str {{ \"{name}\" }} }}"
    )
    .parse()
    .unwrap()
}
