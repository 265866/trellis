use std::backtrace::BacktraceStatus;
use std::io::ErrorKind;
use std::num::IntErrorKind;

fn describe(kind: IntErrorKind) -> &'static str {
    // Flagged: four variants fall to the wildcard unnamed.
    match kind {
        IntErrorKind::Empty => "empty",
        _ => "invalid",
    }
}

fn describe_ref(kind: &IntErrorKind) -> &'static str {
    // Flagged: a binding catches the rest just as `_` does, behind a reference too.
    match kind {
        IntErrorKind::Empty | IntErrorKind::Zero => "empty",
        IntErrorKind::InvalidDigit => "digit",
        other => "overflow",
    }
}

fn status(status: BacktraceStatus) -> bool {
    // Flagged: the wildcard alternative is inside an or-pattern that names one variant.
    match status {
        BacktraceStatus::Captured => true,
        BacktraceStatus::Disabled | _ => false,
    }
}

fn is_not_found(kind: ErrorKind) -> bool {
    // Flagged: the help lists the first few unnamed variants and counts the rest.
    match kind {
        ErrorKind::NotFound => true,
        _ => false,
    }
}

fn describe_all(kind: IntErrorKind) -> &'static str {
    // Not flagged: every variant is named, the ignored ones in the wildcard arm.
    match kind {
        IntErrorKind::Empty => "empty",
        IntErrorKind::InvalidDigit
        | IntErrorKind::PosOverflow
        | IntErrorKind::NegOverflow
        | IntErrorKind::Zero
        | _ => "invalid",
    }
}

#[non_exhaustive]
enum Local {
    A,
    B,
}

fn local(value: Local) -> bool {
    // Not flagged: this crate's own enum, which it can match exhaustively.
    match value {
        Local::A => true,
        _ => false,
    }
}

fn only_empty(kind: IntErrorKind) -> bool {
    // Not flagged: `if let` and `matches!` are the guide's way to pick out some variants.
    if let IntErrorKind::Empty = kind {
        return true;
    }
    matches!(kind, IntErrorKind::Zero)
}

fn nested(kind: Option<IntErrorKind>) -> bool {
    // Flagged: a `_` arm on an `Option` of the enum hides its variants too.
    match kind {
        Some(IntErrorKind::Empty) => true,
        _ => false,
    }
}

fn nested_wildcards(kind: Option<IntErrorKind>, result: Result<IntErrorKind, ()>) -> u8 {
    // Flagged: `Some(_)` and `Ok(..)` hide the variants inside.
    let first = match kind {
        Some(IntErrorKind::Empty | IntErrorKind::Zero) => 1,
        Some(_) => 2,
        None => 3,
    };
    let second = match result {
        Ok(IntErrorKind::InvalidDigit) => 1,
        Ok(..) => 2,
        Err(()) => 3,
    };
    first + second
}

fn passed_on(kind: Option<IntErrorKind>) -> &'static str {
    // Not flagged: the match names no variant, and passes the value on whole.
    match kind {
        Some(kind) => describe(kind),
        None => "none",
    }
}

fn main() {
    let _ = (
        describe(IntErrorKind::Zero),
        describe_ref(&IntErrorKind::Zero),
        describe_all(IntErrorKind::Zero),
    );
    let _ = (
        status(BacktraceStatus::Disabled),
        is_not_found(ErrorKind::Other),
        local(Local::B),
    );
    let _ = (only_empty(IntErrorKind::Zero), nested(None), passed_on(None));
    let _ = nested_wildcards(None, Err(()));
    let _ = tuple_structs(Wrapper(1), Ok(std::num::Wrapping(2)), None);
    let _ = unit_and_braced_structs(Err(TooDeep), Span { start: 1 }, 0..1);
    let _ = (through_an_alias(IntErrorKind::Zero), IntErrorKind::Zero.describe());
    let _ = struct_constructors_outside_match(None, std::marker::PhantomPinned, Err(()));
}

// Not reported: flagged, but an item-level `expect` in the documented form takes it.
#[cfg_attr(
    dylint_lib = "trellis_lints",
    expect(
        non_exhaustive_wildcard,
        reason = "the fixture checks an item-level expect"
    )
)]
fn excused(kind: IntErrorKind) -> bool {
    match kind {
        IntErrorKind::Empty => true,
        _ => false,
    }
}

struct Wrapper(u8);

struct TooDeep;

struct Span {
    start: u8,
}

fn unit_and_braced_structs(
    text: Result<u8, TooDeep>,
    span: Span,
    range: std::ops::Range<u8>,
) -> u8 {
    // Not flagged: a unit struct's and a braced struct's patterns, local or another crate's, name
    // no variant.
    let length = match text {
        Ok(length) => length,
        Err(TooDeep) => 0,
    };
    let start = match span {
        Span { start } => start,
    };
    match range {
        std::ops::Range { start: first, .. } => length + start + first,
    }
}

fn tuple_structs(
    local: Wrapper,
    foreign: Result<std::num::Wrapping<u8>, IntErrorKind>,
    reversed: Option<std::cmp::Reverse<IntErrorKind>>,
) -> u8 {
    // Not flagged: a tuple struct's constructor, local or another crate's, isn't a variant.
    let first = match local {
        Wrapper(value) => value,
    };
    let second = match foreign {
        Ok(std::num::Wrapping(value)) => value,
        Err(_) => first,
    };
    // Flagged: the struct around the enum doesn't hide that a wildcard catches its variants.
    match reversed {
        Some(std::cmp::Reverse(IntErrorKind::Empty)) => second,
        _ => 0,
    }
}

fn struct_constructors_outside_match(
    wrapped: Option<std::num::Wrapping<u8>>,
    pinned: std::marker::PhantomPinned,
    reversed: Result<std::cmp::Reverse<IntErrorKind>, ()>,
) -> u8 {
    // Not flagged: only a `match` is checked, and a struct's constructor isn't a variant.
    let Some(std::num::Wrapping(value)) = wrapped else {
        return 0;
    };
    let std::marker::PhantomPinned = pinned;
    if let Ok(std::cmp::Reverse(IntErrorKind::Empty)) = reversed {
        return value;
    }
    match pinned {
        std::marker::PhantomPinned => value,
    }
}

type KindAlias = IntErrorKind;

fn through_an_alias(kind: IntErrorKind) -> &'static str {
    // Flagged: `KindAlias::Empty` names `Empty`, so the help leaves it out.
    match kind {
        KindAlias::Empty => "empty",
        IntErrorKind::Zero => "zero",
        _ => "other",
    }
}

trait Describe {
    fn describe(&self) -> &'static str;
}

impl Describe for IntErrorKind {
    fn describe(&self) -> &'static str {
        // Flagged: a variant named only through `Self::` still makes the match one to check.
        match self {
            Self::Empty => "empty",
            _ => "other",
        }
    }
}
