//! Where a Rust token is: in an attribute, in a macro's arguments, or elsewhere.

use proc_macro2::{Delimiter, TokenTree};

/// Where a token is, for the checks that read Rust sources as tokens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TokenContext {
    /// Outside attributes and macro arguments.
    Code,
    /// Inside a macro invocation's arguments, outside an attribute.
    MacroArguments,
    /// Inside an attribute's brackets, at any depth.
    Attribute,
}

impl TokenContext {
    /// The context of the tokens inside the group `trees[index]`, which is in this context.
    pub(crate) fn inside(self, trees: &[TokenTree], index: usize) -> Self {
        let Some(TokenTree::Group(group)) = trees.get(index) else {
            return self;
        };
        let opens_attribute = group.delimiter() == Delimiter::Bracket
            && (punct_before(trees, index, 1, '#')
                || (punct_before(trees, index, 1, '!') && punct_before(trees, index, 2, '#')));
        let follows_macro_name = punct_before(trees, index, 1, '!')
            && index
                .checked_sub(2)
                .and_then(|name| trees.get(name))
                .is_some_and(|tree| matches!(tree, TokenTree::Ident(_)));
        if self == Self::Attribute || opens_attribute {
            Self::Attribute
        } else if self == Self::MacroArguments || follows_macro_name {
            Self::MacroArguments
        } else {
            Self::Code
        }
    }
}

/// Whether the token `offset` places before `trees[index]` is the punctuation `expected`.
fn punct_before(trees: &[TokenTree], index: usize, offset: usize, expected: char) -> bool {
    index
        .checked_sub(offset)
        .and_then(|before| trees.get(before))
        .is_some_and(|tree| matches!(tree, TokenTree::Punct(punct) if punct.as_char() == expected))
}

#[cfg(test)]
mod tests {
    use std::str::FromStr as _;

    use proc_macro2::TokenStream;

    use super::*;

    fn context_of_last_group(source: &str) -> TokenContext {
        let trees: Vec<TokenTree> = TokenStream::from_str(source).unwrap().into_iter().collect();
        TokenContext::Code.inside(&trees, trees.len() - 1)
    }

    #[test]
    fn groups_get_their_context() {
        for (source, context) in [
            ("#[path = \"a.rs\"]", TokenContext::Attribute),
            ("#![path = \"a.rs\"]", TokenContext::Attribute),
            ("m!(path = \"a.rs\")", TokenContext::MacroArguments),
            ("a::m! { x }", TokenContext::MacroArguments),
            ("macro_rules! m { () => {} }", TokenContext::Code),
            ("fn f() { x }", TokenContext::Code),
            ("let a = [1]", TokenContext::Code),
            ("!(x)", TokenContext::Code),
        ] {
            assert_eq!(
                context_of_last_group(source),
                context,
                "the last group of {source:?} is in {context:?}"
            );
        }
    }
}
