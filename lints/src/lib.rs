//! Lints for the Google Rust Style Guide rules that clippy doesn't check. `cargo dylint` loads
//! this library into the compiler and runs these lints over the workspace.

#![feature(default_field_values, rustc_private)]
#![warn(unused_extern_crates)]

extern crate rustc_ast;
extern crate rustc_attr_ir;
extern crate rustc_data_structures;
extern crate rustc_hir;
extern crate rustc_lexer;
extern crate rustc_lint;
extern crate rustc_lint_defs;
extern crate rustc_middle;
extern crate rustc_session;
extern crate rustc_span;
extern crate rustc_structures;

mod attribute_origin;
mod bool_literal_argument;
mod code_before_main;
mod code_comment;
mod command_line_lint;
mod config;
mod derive_output;
mod disabled_code;
mod discarded_result;
mod doc_after_attribute;
mod doc_attribute_style;
mod doc_starts_lowercase;
mod enum_glob_outside_fn;
mod error_type_copy;
mod error_type_must_use;
mod extension_trait_suffix;
mod file_named_after_item;
mod foreign_doc_hidden;
mod foreign_prelude_glob;
mod from_without_constructor;
mod get_prefixed_getter;
mod impl_into_params;
mod imports;
mod inferred_conversion_binding;
mod invisible_characters;
mod iterator_chain_in_for;
mod iterator_methods;
mod lifetime_naming;
mod local_doc_hidden;
mod local_source;
mod macro_definition;
mod missing_workspace_lints;
mod module_level_trait_import;
mod multi_field_tuple;
mod mutating_iterator_closure;
mod needless_raw_identifier;
mod non_exhaustive_wildcard;
mod non_leaf_module_items;
mod operator_overload;
mod options_struct_default;
mod prelude_module;
mod primitive_type_alias;
mod qualified_function_calls;
mod qualified_type_path;
mod rustfmt_skip_justification;
mod shared_ownership;
mod single_impl_trait;
mod str_replace_on_literal;
mod string_concat_chain;
mod string_literal;
mod suppression_policy;
mod test_code;
mod trait_impls;
mod trusted_lint_levels;
mod try_new_result;
#[cfg(test)]
mod ui_harness;
mod unsafe_outside_unsafe_crate;

use rustc_lint::{EarlyLintPass, LateLintPass, LintStore};
use rustc_lint_defs::Lint;
use rustc_session::Session;

dylint_linting::dylint_library!();

/// A lint pass, built fresh for each crate the compiler checks.
enum Pass {
    Early(fn() -> Box<dyn EarlyLintPass>),
    Late(fn() -> Box<dyn for<'tcx> LateLintPass<'tcx>>),
}

/// Every pass in the library. Adding a lint means adding its pass here; [`lints`] reads the lints
/// each pass declares.
const PASSES: &[Pass] = &[
    Pass::Late(|| Box::new(bool_literal_argument::BoolLiteralArgument)),
    Pass::Late(|| Box::new(code_before_main::CodeBeforeMain)),
    Pass::Early(|| Box::new(code_comment::CodeComment)),
    Pass::Early(|| Box::new(disabled_code::DisabledCode)),
    Pass::Late(|| Box::new(disabled_code::CfgImports)),
    Pass::Late(|| Box::new(discarded_result::DiscardedResult)),
    Pass::Early(|| Box::new(doc_after_attribute::DocAfterAttribute)),
    Pass::Early(|| Box::new(doc_attribute_style::DocAttributeStyle)),
    Pass::Early(|| Box::new(doc_starts_lowercase::DocStartsLowercase)),
    Pass::Late(|| Box::new(enum_glob_outside_fn::EnumGlobOutsideFn)),
    Pass::Late(|| Box::new(error_type_copy::ErrorTypeCopy)),
    Pass::Late(|| Box::new(error_type_must_use::ErrorTypeMustUse)),
    Pass::Late(|| Box::new(extension_trait_suffix::ExtensionTraitSuffix)),
    Pass::Early(|| Box::new(file_named_after_item::FileNamedAfterItem)),
    Pass::Late(|| Box::new(foreign_doc_hidden::ForeignDocHidden)),
    Pass::Late(|| Box::new(foreign_prelude_glob::ForeignPreludeGlob)),
    Pass::Late(|| Box::new(from_without_constructor::FromWithoutConstructor)),
    Pass::Late(|| Box::new(get_prefixed_getter::GetPrefixedGetter)),
    Pass::Late(|| Box::new(impl_into_params::ImplIntoParams)),
    Pass::Late(|| Box::new(inferred_conversion_binding::InferredConversionBinding)),
    Pass::Early(|| Box::<invisible_characters::InvisibleCharacters>::default()),
    Pass::Late(|| Box::new(iterator_chain_in_for::IteratorChainInFor)),
    Pass::Early(|| Box::new(lifetime_naming::LifetimeNaming)),
    Pass::Early(|| Box::<local_doc_hidden::LocalDocHidden>::default()),
    Pass::Early(|| Box::new(macro_definition::MacroDefinition)),
    Pass::Early(|| Box::new(missing_workspace_lints::MissingWorkspaceLints)),
    Pass::Late(|| Box::new(module_level_trait_import::ModuleLevelTraitImport)),
    Pass::Late(|| Box::new(multi_field_tuple::MultiFieldTuple)),
    Pass::Late(|| Box::new(mutating_iterator_closure::MutatingIteratorClosure)),
    Pass::Early(|| Box::new(needless_raw_identifier::NeedlessRawIdentifier)),
    Pass::Late(|| Box::new(non_exhaustive_wildcard::NonExhaustiveWildcard)),
    Pass::Early(|| Box::<non_leaf_module_items::NonLeafModuleItems>::default()),
    Pass::Late(|| Box::new(operator_overload::OperatorOverload)),
    Pass::Late(|| Box::new(options_struct_default::OptionsStructDefault)),
    Pass::Early(|| Box::new(prelude_module::PreludeModule)),
    Pass::Late(|| Box::new(primitive_type_alias::PrimitiveTypeAlias)),
    Pass::Late(|| Box::new(qualified_function_calls::QualifiedFunctionCalls)),
    Pass::Late(|| Box::new(qualified_type_path::QualifiedTypePath)),
    Pass::Early(|| Box::<rustfmt_skip_justification::RustfmtSkipJustification>::default()),
    Pass::Late(|| Box::new(shared_ownership::SharedOwnership)),
    Pass::Late(|| Box::new(single_impl_trait::SingleImplTrait)),
    Pass::Late(|| Box::new(str_replace_on_literal::StrReplaceOnLiteral)),
    Pass::Late(|| Box::new(string_concat_chain::StringConcatChain)),
    Pass::Early(|| Box::new(suppression_policy::SuppressionPolicy)),
    Pass::Late(|| Box::new(try_new_result::TryNewResult)),
    Pass::Early(|| Box::new(unsafe_outside_unsafe_crate::UnsafeOutsideUnsafeCrate)),
];

/// Every lint in the library, as the passes declare them, so tests can turn off all but the one
/// they check.
///
/// Built from each pass's `get_lints` rather than kept in a `const`: rustc tells lints apart by
/// the address of their static, and a `const` slice would hold copies at other addresses, so
/// `-A`, `#[allow]` and `#[expect]` would never match the lints as they fire.
fn lints() -> Vec<&'static Lint> {
    PASSES
        .iter()
        .flat_map(|pass| match pass {
            Pass::Early(new) => new().get_lints(),
            Pass::Late(new) => new().get_lints(),
        })
        .collect()
}

/// Registers the lints with the compiler. cargo-dylint calls this when it loads the library.
#[expect(
    clippy::no_mangle_with_rust_abi,
    reason = "cargo-dylint calls it from Rust"
)]
#[unsafe(no_mangle)]
pub fn register_lints(session: &Session, lint_store: &mut LintStore) {
    config::init(session);
    lint_store.register_lints(&lints());
    for pass in PASSES {
        match *pass {
            Pass::Early(new) => lint_store.register_early_lint_pass(Box::new(new)),
            Pass::Late(new) => lint_store.register_late_lint_pass(Box::new(move |_| new())),
        }
    }
}
