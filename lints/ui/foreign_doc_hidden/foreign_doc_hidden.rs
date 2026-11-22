// aux-build:hidden_fields.rs
// aux-build:defaulted.rs
// aux-build:mirror.rs
// aux-build:hidden_root.rs
// edition:2024
// `Eq::assert_receiver_is_total_eq`, the hidden item these cases reach, is also deprecated.
#![allow(deprecated)]

// The test's `dylint.toml` sets `owned_crate_prefixes = ["alloc"]`.

extern crate defaulted;
extern crate hidden_fields;
extern crate mirror;
extern crate hidden_root;

use hidden_fields::{Context, Event, Wrapper};
// Flagged: the import reaches a hidden function.
use hidden_fields::helper;

// Flagged: a hidden function named in a string a derive parses as a path, as serde does for
// `#[serde(default = "...")]`. The caller wrote the path there.
#[derive(defaulted::Defaulted)]
#[defaulted(path = "hidden_fields::helper")]
struct Named;

// Flagged: the same through text that differs from the path: a comment and a raw identifier.
#[derive(defaulted::Defaulted)]
#[defaulted(path = "hidden_fields::/**/helper")]
struct Commented;

#[derive(defaulted::Defaulted)]
#[defaulted(path = "hidden_fields::r#helper")]
struct Raw;

// Flagged: a hidden module and a hidden type named in a string a derive parses as a path and
// adds a segment of its own to at its own span, as serde adds `deserialize` for
// `#[serde(with = "...")]`, which gives the path the derive's span.
#[derive(defaulted::Defaulted)]
#[defaulted(with = "hidden_fields::internals")]
struct Module;

#[derive(defaulted::Defaulted)]
#[defaulted(with = "hidden_fields::Internal")]
struct Type;

// Flagged: a hidden field of another crate's type that a derive mirrors, as serde does for
// `#[serde(remote = "...")]`. The derive reads it, builds it and takes it apart at spans of its
// own, with the caller's name for the field, so each is reported at that name. Not flagged: the
// documented field, and the hidden `own` field the derive reads with a name of its own.
#[derive(mirror::Mirror)]
#[mirror(remote = "hidden_fields::Remote")]
struct Mirrored {
    shown: u32,
    concealed: u32,
}

// Not flagged: a hidden function the derive calls at a string's span, which the string doesn't
// hold, as thiserror calls its hidden `AsDisplay` at an `#[error("..")]` message.
#[derive(defaulted::Defaulted)]
#[defaulted(own = "a message")]
struct Messaged;

fn main() {
    let value = 7_u8;

    // Flagged: a method call that resolves to a hidden trait method in `core`.
    value.assert_receiver_is_total_eq();

    // Flagged: the same method through a path, a qualified path and a type-relative path.
    Eq::assert_receiver_is_total_eq(&value);
    <u8 as Eq>::assert_receiver_is_total_eq(&value);
    u8::assert_receiver_is_total_eq(&value);

    // Flagged: a hidden function written by hand as a raw identifier, whose text isn't its name.
    // Only the code a derive writes is skipped for that.
    r#helper();

    // Not flagged: `from_elem` is hidden, but its crate, `alloc`, is configured as owned.
    let zeros = std::vec::from_elem(0_u8, 3);

    // Not flagged: `vec!` and `#[derive(Eq)]` reach hidden items from their expansions.
    let ones = vec![1_u8; 3];

    // Not flagged: this crate owns its own hidden items.
    local_hidden();

    fields(Context {
        visible: 1,
        // Flagged: a hidden field in a struct literal.
        hidden: 2,
    });
}

fn fields(context: Context) -> u32 {
    // Not flagged: a documented field.
    let visible = context.visible;
    // Flagged: a hidden field access, also through a reference.
    let hidden = context.hidden + (&context).hidden;
    // Flagged: a hidden tuple-struct field.
    let wrapped = Wrapper(3).0;
    // Flagged: a hidden field in a struct pattern; the documented one is not.
    let Event::Order { shelf, seed } = (Event::Order { shelf: 4, seed: 5 });
    visible + hidden + wrapped + shelf + seed + excepted(&context)
}

// Not flagged: an exception on the function, with a reason.
#[cfg_attr(
    dylint_lib = "trellis_lints",
    expect(foreign_doc_hidden, reason = "the crate has no documented way to read it")
)]
fn excepted(context: &Context) -> u32 {
    context.hidden
}

#[derive(PartialEq, Eq)]
struct Derived;

struct Manual;

impl PartialEq for Manual {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

// Not flagged: implementing `Eq` doesn't name its hidden method.
impl Eq for Manual {}

#[doc(hidden)]
fn local_hidden() {}

// Not flagged: a crate whose root is hidden from its docs, as `windows` hides its own, still
// documents the items under it.
use hidden_root::api::Handle;
use hidden_root::documented;

// Flagged: a hidden function in that crate, and a hidden module, are hidden all the same.
use hidden_root::api::internal;
use hidden_root::internals::helper as root_helper;

fn hidden_root_paths() -> u32 {
    let _handle = Handle;
    // Not flagged: the crate root as a path's first segment.
    let total = hidden_root::documented() + documented();
    // Flagged: a hidden function named through the crate root.
    total + hidden_root::api::internal() + internal() + root_helper()
}
