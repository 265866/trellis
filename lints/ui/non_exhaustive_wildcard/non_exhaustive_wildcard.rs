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
