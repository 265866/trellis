// aux-build:levels.rs
// The test's `dylint.toml` trusts `levels` and lets its macros set only
// `clippy::redundant_closure_call` over code written in the source.

extern crate levels;

fn three() -> u32 {
    3
}

// Flagged: a literal, and a call of a path with no arguments, under a lint group, which reaches
// whatever they sit in.
#[derive(levels::Count)]
#[count(3)]
struct Literal;

#[derive(levels::Count)]
#[count(three())]
struct Call;

// Flagged: a cast written without spaces, and a macro call with a space in its path.
#[derive(levels::Count)]
#[count((3_u8)as u32)]
struct Cast;

#[derive(levels::Count)]
#[count(std:: dbg!(3))]
struct Debugged;

// Flagged: an expression clippy would check, under a lint group.
#[derive(levels::Count)]
#[count("3".parse::<u32>().unwrap())]
struct Parsed;

// Flagged: a closure, under a lint group.
#[derive(levels::Count)]
#[count((|| 3)())]
struct Closure;

// Not flagged: the body under a lint the configuration lists for the crate.
#[levels::wrapped(allow(clippy::redundant_closure_call))]
fn listed() -> u32 {
    three() + 1
}

// Flagged: the body under a lint it doesn't list.
#[levels::wrapped(allow(clippy::needless_return))]
fn unlisted() -> u32 {
    return three();
}

// Not flagged: a lint level over code of the macro's own at the caller's spans.
#[levels::runtime]
fn generated() {
    let _ = three();
}

mod defaults {
    pub(crate) fn label() {}
}

// Not flagged: the type's name, as a path and as a string, and a string the caller wrote, which a
// derive writes under `unused_qualifications`, a lint that never reads a literal.
#[derive(levels::Named)]
#[named(rename = "label")]
struct Label;

// Flagged: the same string under a lint that reads literals.
#[derive(levels::Named)]
#[named(level = "clippy::octal_escapes")]
#[named(rename = "\033[0m")]
struct Escaped;

// Flagged: a path written in a string, which the derive parses and calls under a lint that
// reports paths.
#[derive(levels::Named)]
#[named(path = "self::defaults::label")]
struct Defaulted;

fn zero() {}

// Not flagged: a function named in a string by a bare name, under the level serde writes, whose
// lints can't fire on a path of one segment.
#[derive(levels::Named)]
#[named(path = "zero")]
struct Bare;

#[derive(levels::Named)]
#[named(level = "non_upper_case_globals, unused_attributes, unused_qualifications, clippy::absolute_paths")]
#[named(path = "zero")]
struct Present;

// Not flagged: a module named by a bare name, to which the derive adds a segment of its own.
#[derive(levels::Named)]
#[named(level = "non_upper_case_globals, unused_attributes, unused_qualifications, clippy::absolute_paths")]
#[named(module = "defaults")]
struct Module;

// Not flagged: a function through its module, and a type, under the level serde writes, whose
// lints can't fire on two plain segments whose last isn't in scope by itself.
#[derive(levels::Named)]
#[named(level = "non_upper_case_globals, unused_attributes, unused_qualifications, clippy::absolute_paths")]
#[named(path = "defaults::label")]
struct Qualified;

#[derive(levels::Named)]
#[named(level = "non_upper_case_globals, unused_attributes, unused_qualifications, clippy::absolute_paths")]
#[named(ty = "u8")]
struct Typed;

// Flagged: a lint level written in a string the derive parses as a type, where no lint reads it
// as an attribute, however trusted the derive.
#[derive(levels::Named)]
#[named(ty = "[(); { #[allow(clippy::all)] fn hidden() { let _ = 1_u8.checked_sub(2).unwrap(); } 0 }]")]
struct Muted;

#[derive(levels::Named)]
#[named(level = "non_upper_case_globals, unused_attributes, unused_qualifications, clippy::absolute_paths")]
#[named(joined = "defaults")]
struct Joined;

// Flagged: the same through `self`, the derive adding a segment of its own at its own span.
#[derive(levels::Named)]
#[named(level = "non_upper_case_globals, unused_attributes, unused_qualifications, clippy::absolute_paths")]
#[named(joined = "self::defaults")]
struct SelfJoined;

// Flagged: under the level serde writes, a path through `self` or `crate`, of three segments, or
// whose text a comment, a raw identifier or an empty turbofish makes differ from the path.
#[derive(levels::Named)]
#[named(level = "non_upper_case_globals, unused_attributes, unused_qualifications, clippy::absolute_paths")]
#[named(module = "self::defaults")]
struct QualifiedModule;

#[derive(levels::Named)]
#[named(level = "non_upper_case_globals, unused_attributes, unused_qualifications, clippy::absolute_paths")]
#[named(path = "crate::defaults::label")]
struct FromCrate;

#[derive(levels::Named)]
#[named(level = "non_upper_case_globals, unused_attributes, unused_qualifications, clippy::absolute_paths")]
#[named(path = "defaults::/**/label")]
struct Commented;

#[derive(levels::Named)]
#[named(level = "non_upper_case_globals, unused_attributes, unused_qualifications, clippy::absolute_paths")]
#[named(path = "defaults::r#label")]
struct Raw;

#[derive(levels::Named)]
#[named(level = "non_upper_case_globals, unused_attributes, unused_qualifications, clippy::absolute_paths")]
#[named(path = "defaults::label::<>")]
struct Turbofish;

// Not flagged: under the level serde writes, a type or bound whose every path is short, generic
// arguments and all, as `#[serde(bound = "..")]`, `from`, `into` and `try_from` take them.
#[derive(levels::Named)]
#[named(level = "non_upper_case_globals, unused_attributes, unused_qualifications, clippy::absolute_paths")]
#[named(ty = "Vec<u8>")]
struct Generic;

#[derive(levels::Named)]
#[named(level = "non_upper_case_globals, unused_attributes, unused_qualifications, clippy::absolute_paths")]
#[named(ty = "(u32, [Option<limits::Width>; 2], &'static str)")]
struct Tuple;

#[derive(levels::Named)]
#[named(level = "non_upper_case_globals, unused_attributes, unused_qualifications, clippy::absolute_paths")]
#[named(bound = "T: Default")]
struct Bound;

#[derive(levels::Named)]
#[named(level = "non_upper_case_globals, unused_attributes, unused_qualifications, clippy::absolute_paths")]
#[named(bound = "T: Marker<'de> + limits::Bounded<'de>, Vec<T>: Clone")]
struct Bounds;

// Flagged: under the level serde writes, a type or bound with a path of three segments, through
// `crate`, or whose last segment is also in scope by itself.
#[derive(levels::Named)]
#[named(level = "non_upper_case_globals, unused_attributes, unused_qualifications, clippy::absolute_paths")]
#[named(ty = "std::primitive::u8")]
struct Absolute;

#[derive(levels::Named)]
#[named(level = "non_upper_case_globals, unused_attributes, unused_qualifications, clippy::absolute_paths")]
#[named(ty = "Vec<std::string::String>")]
struct AbsoluteArgument;

#[derive(levels::Named)]
#[named(level = "non_upper_case_globals, unused_attributes, unused_qualifications, clippy::absolute_paths")]
#[named(bound = "T: crate::Marker<'de>")]
struct CrateBound;

#[derive(levels::Named)]
#[named(level = "non_upper_case_globals, unused_attributes, unused_qualifications, clippy::absolute_paths")]
#[named(bound = "T: Clone + limits::Marker<'de>")]
struct ImportedBound;

trait Marker<'a> {}

mod limits {
    pub(crate) use super::Marker;

    pub(crate) const WIDTH: usize = 4;

    pub(crate) struct Width;

    pub(crate) trait Bounded<'a> {}
}

// Not flagged: a constant through its module in an array length the derive copies, as serde copies
// a field's type, under the level serde writes.
#[derive(levels::Named)]
#[named(level = "non_upper_case_globals, unused_attributes, unused_qualifications, clippy::absolute_paths")]
#[named_length(limits::WIDTH)]
struct Header;

// Flagged: the same through `self`, which `unused_qualifications` would report.
#[derive(levels::Named)]
#[named(level = "non_upper_case_globals, unused_attributes, unused_qualifications, clippy::absolute_paths")]
#[named_length(self::limits::WIDTH)]
struct SelfHeader;

// Not flagged: a name the derive binds and reads at the span of the caller's token, as thiserror
// binds `transparent`, which isn't code the caller wrote.
#[derive(levels::Forwarded)]
#[forwarded(transparent)]
struct Forwarded;

mod imported {
    use super::defaults::{self, label};

    // Flagged: two segments whose last the module also imports, which `unused_qualifications`
    // would report.
    #[derive(levels::Named)]
    #[named(level = "non_upper_case_globals, unused_attributes, unused_qualifications, clippy::absolute_paths")]
    #[named(path = "defaults::label")]
    pub(crate) struct Imported;

    pub(crate) fn imported() {
        label();
        let _ = Imported::named();
    }
}

mod tests {
    use super::*;

    // Not flagged: a test module's glob import of its parent, which brings `defaults` but not
    // `label` into scope.
    #[derive(levels::Named)]
    #[named(level = "non_upper_case_globals, unused_attributes, unused_qualifications, clippy::absolute_paths")]
    #[named(path = "defaults::label")]
    pub(crate) struct Globbed;

    pub(crate) fn globbed() {
        let _ = Globbed::named();
    }
}

mod glob_imported {
    use super::defaults::*;

    // Flagged: a glob import that brings the last segment into scope by itself.
    #[derive(levels::Named)]
    #[named(level = "non_upper_case_globals, unused_attributes, unused_qualifications, clippy::absolute_paths")]
    #[named(path = "super::defaults::label")]
    pub(crate) struct ThroughSuper;

    #[derive(levels::Named)]
    #[named(level = "non_upper_case_globals, unused_attributes, unused_qualifications, clippy::absolute_paths")]
    #[named(path = "defaults::label")]
    pub(crate) struct Globbed;

    mod defaults {
        pub(crate) use crate::defaults::label;
    }

    pub(crate) fn globbed() {
        label();
        let _ = (Globbed::named(), ThroughSuper::named());
    }
}

enum Shade {
    Dark,
}

// Not flagged: a glob import of an enum's variants in a function, which doesn't bring `label`.
fn shaded() {
    use Shade::*;

    #[derive(levels::Named)]
    #[named(level = "non_upper_case_globals, unused_attributes, unused_qualifications, clippy::absolute_paths")]
    #[named(path = "defaults::label")]
    struct Shaded;

    let _: Shade = Dark;
    let _ = Shaded::named();
}

// Flagged: two segments whose last a function's body imports, which `unused_qualifications`
// would report there.
fn local() {
    use crate::defaults::{self, label};

    #[derive(levels::Named)]
    #[named(level = "non_upper_case_globals, unused_attributes, unused_qualifications, clippy::absolute_paths")]
    #[named(path = "defaults::label")]
    struct Local;

    label();
    let _ = (Local::named(), defaults::label());
}

mod aliased {
    use std::result;

    type Result<T> = result::Result<T, String>;

    // Not flagged: the module's own `Result` hides the prelude's, so `Result` alone wouldn't name
    // the type the path does.
    #[derive(levels::Named)]
    #[named(level = "non_upper_case_globals, unused_attributes, unused_qualifications, clippy::absolute_paths")]
    #[named(ty = "result::Result<u32, String>")]
    pub(crate) struct Aliased;

    pub(crate) fn aliased() -> Result<()> {
        let _ = Aliased::named();
        Ok(())
    }
}

mod hidden_import {
    use super::defaults::{self, label};

    // Not flagged: the function's own `label` hides the module's import.
    pub(crate) fn hidden() {
        fn label() {}

        #[derive(levels::Named)]
        #[named(level = "non_upper_case_globals, unused_attributes, unused_qualifications, clippy::absolute_paths")]
        #[named(path = "defaults::label")]
        struct Hidden;

        label();
        let _ = (Hidden::named(), defaults::label());
    }

    pub(crate) fn imported() {
        label();
    }
}

mod hidden_glob {
    use super::defaults::{self, *};

    fn label() {}

    // Not flagged: the module's own `label` hides the one the glob import brings.
    #[derive(levels::Named)]
    #[named(level = "non_upper_case_globals, unused_attributes, unused_qualifications, clippy::absolute_paths")]
    #[named(path = "defaults::label")]
    pub(crate) struct Hidden;

    pub(crate) fn hidden() {
        label();
        let _ = Hidden::named();
    }
}

mod private_glob {
    use super::defaults;
    use self::inner::*;

    mod inner {
        use crate::defaults::label;

        pub(crate) fn other() {
            label();
        }
    }

    // Not flagged: the glob import brings only the names `inner` lets this module see, not its
    // private import of `label`.
    #[derive(levels::Named)]
    #[named(level = "non_upper_case_globals, unused_attributes, unused_qualifications, clippy::absolute_paths")]
    #[named(path = "defaults::label")]
    pub(crate) struct Private;

    pub(crate) fn private() {
        other();
        let _ = Private::named();
    }
}

// Flagged: a bare name under a lint that may fire on it, and under a group.
#[derive(levels::Named)]
#[named(level = "unused_qualifications, clippy::redundant_locals")]
#[named(path = "zero")]
struct Unlisted;

#[derive(levels::Named)]
#[named(level = "clippy::style")]
#[named(path = "zero")]
struct Grouped;

mod accents {
    pub(crate) fn zéro() -> u32 {
        0
    }
}

// Flagged: a path through `crate` in a string, and a call through a module under a lint group,
// with the function's name written decomposed, as `e` and a combining accent, which the compiler
// reads as the composed name it interns.
#[derive(levels::Named)]
#[named(level = "non_upper_case_globals, unused_attributes, unused_qualifications, clippy::absolute_paths")]
#[named(path = "crate::accents::zéro")]
struct DecomposedPath;

#[derive(levels::Count)]
#[count(accents::zéro())]
struct DecomposedCall;

use std::fmt::Display;
use std::str::FromStr;

mod wire {
    pub(crate) struct Config;
}

// Not flagged: under the level serde writes, a path through a module to an item that its last
// segment alone doesn't name, as `wire::Config` beside a `Config` of its own, which
// `unused_qualifications` doesn't report.
#[derive(levels::Named)]
#[named(level = "non_upper_case_globals, unused_attributes, unused_qualifications, clippy::absolute_paths")]
#[named(ty = "wire::Config")]
struct Config;

mod differs {
    use super::defaults::*;

    // Not flagged: the glob brings a `label` other than the one the path names.
    #[derive(levels::Named)]
    #[named(level = "non_upper_case_globals, unused_attributes, unused_qualifications, clippy::absolute_paths")]
    #[named(path = "defaults::label")]
    pub(crate) struct Differs;

    mod defaults {
        pub(crate) fn label() {}
    }

    pub(crate) fn differs() {
        label();
        let _ = Differs::named();
    }
}

// Not flagged: under the level serde writes, an associated item through a generic parameter,
// `Self` or a qualified self, which `unused_qualifications` never reports, as in serde's own
// `bound(deserialize = "T: FromStr, T::Err: Display")`, even where the prelude brings a name
// such as `Err` by itself; and a type whose name isn't ASCII.
#[derive(levels::Named)]
#[named(level = "non_upper_case_globals, unused_attributes, unused_qualifications, clippy::absolute_paths")]
#[named(bound = "T: FromStr, T::Err: Display")]
struct Associated;

#[derive(levels::Named)]
#[named(level = "non_upper_case_globals, unused_attributes, unused_qualifications, clippy::absolute_paths")]
#[named(bound = "T: FromStr, <T as FromStr>::Err: Display")]
struct QualifiedSelf;

#[derive(levels::Named)]
#[named(level = "non_upper_case_globals, unused_attributes, unused_qualifications, clippy::absolute_paths")]
#[named(bound = "Self: Sized, T: Clone")]
struct SelfBound;

#[derive(levels::Named)]
#[named(level = "non_upper_case_globals, unused_attributes, unused_qualifications, clippy::absolute_paths")]
#[named(path = "<u8 as Default>::default")]
struct QualifiedCall;

struct Café;

#[derive(levels::Named)]
#[named(level = "non_upper_case_globals, unused_attributes, unused_qualifications, clippy::absolute_paths")]
#[named(ty = "Café")]
struct Accented;

// Flagged: under the level serde writes, a qualified self through a path of more than two
// segments, and a bound after `Self` whose last segment is also in scope by itself.
#[derive(levels::Named)]
#[named(level = "non_upper_case_globals, unused_attributes, unused_qualifications, clippy::absolute_paths")]
#[named(ty = "<u8 as std::str::FromStr>::Err")]
struct AbsoluteQualifiedSelf;

#[derive(levels::Named)]
#[named(level = "non_upper_case_globals, unused_attributes, unused_qualifications, clippy::absolute_paths")]
#[named(bound = "Self: Sized, T: limits::Marker<'de>")]
struct SelfThenImported;

// Flagged: a type through its module whose last segment the prelude also brings by itself.
use std::string;

#[derive(levels::Named)]
#[named(level = "non_upper_case_globals, unused_attributes, unused_qualifications, clippy::absolute_paths")]
#[named(ty = "string::String")]
struct FromPrelude;

mod braced {
    use super::{defaults, defaults::*};

    // Flagged: a glob inside braces that brings the last segment into scope by itself, which the
    // help names by the whole path of its import.
    #[derive(levels::Named)]
    #[named(level = "non_upper_case_globals, unused_attributes, unused_qualifications, clippy::absolute_paths")]
    #[named(path = "defaults::label")]
    pub(crate) struct Braced;

    pub(crate) fn braced() {
        label();
        let _ = Braced::named();
    }
}

fn main() {
    let _ = (
        Literal::count(),
        Call::count(),
        Cast::count(),
        Debugged::count(),
        Label::named(),
        Escaped::named(),
        Defaulted::named(),
        Bare::named(),
        Present::named(),
        Module::named(),
        Qualified::named(),
        Typed::named(),
        Joined::named(),
        SelfJoined::named(),
        QualifiedModule::named(),
        FromCrate::named(),
        Commented::named(),
        Raw::named(),
        Turbofish::named(),
        Generic::named(),
        Tuple::named(),
        Bound::named(),
        Bounds::named(),
        Absolute::named(),
        AbsoluteArgument::named(),
        CrateBound::named(),
        ImportedBound::named(),
        tests::globbed(),
        glob_imported::globbed(),
        shaded(),
        local(),
        imported::imported(),
        Forwarded::forwarded(1),
        Header::named(),
        SelfHeader::named(),
        Unlisted::named(),
        Grouped::named(),
        DecomposedPath::named(),
        DecomposedCall::count(),
        Parsed::count(),
        Closure::count(),
        listed(),
        unlisted(),
        generated(),
    );
    let _ = (
        Config::named(),
        differs::differs(),
        Associated::named(),
        QualifiedSelf::named(),
        SelfBound::named(),
        QualifiedCall::named(),
        Café,
        Accented::named(),
        AbsoluteQualifiedSelf::named(),
        SelfThenImported::named(),
        FromPrelude::named(),
        braced::braced(),
    );
}
