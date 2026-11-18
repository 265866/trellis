// edition:2024
// The flagged literals below hold the characters they name, written as themselves.
// rustc denies the bidirectional control below on its own; this fixture checks this lint.
#![allow(text_direction_codepoint_in_literal)]

// Flagged: a zero-width space.
const SPACE: &str = "check​out";

// Flagged: a byte-order mark in a character literal.
const MARK: char = '﻿';

// Flagged: a soft hyphen in a raw string, which can't hold an escape.
const RAW: &str = r"check­out";

// Flagged: a word joiner in a C string.
const C_STRING: &std::ffi::CStr = c"check⁠out";

// Flagged: two different invisible characters in one literal, both named in the help.
const TWO: &str = "​heart️";

// Flagged: a right-to-left override.
const OVERRIDE: &str = "‮lacigam";

// Not flagged: the escapes say what the string holds.
const ESCAPED: &str = "check\u{200B}out\u{FE0F}";

// Not flagged: visible characters outside ASCII.
const VISIBLE: &str = "café ♥";

// Not flagged: escaped bytes in a byte string.
const BYTES: &[u8] = b"\xE2\x80\x8B";

// Not flagged: variation selectors and joiners that are part of an emoji: a keycap, a red
// heart, a heart on fire and a family.
const EMOJI: [&str; 4] = ["7️⃣", "❤️", "❤️‍🔥", "👨‍👩‍👧"];

// Flagged: a variation selector after a digit that starts no keycap.
const NOT_KEYCAP: &str = "7️";

// Flagged: a joiner after an emoji but before a letter.
const HALF_JOINED: &str = "🔥‍a";

// Not flagged: an exception on the item, with a reason.
#[cfg_attr(
    dylint_lib = "trellis_lints",
    expect(invisible_characters, reason = "the vendor matches the label byte for byte")
)]
const EXCEPTED: &str = "check​out";

// Flagged: a literal in a macro definition.
macro_rules! joined {
    () => {
        "check‍out"
    };
}

fn label(name: &str) -> String {
    match name {
        // Flagged: a pattern literal.
        "‌" => String::new(),
        // Flagged: a format string, which the format macro parses itself.
        _ => format!("{name}‍"),
    }
}

fn main() {
    more();
    // The literal in `joined!` is flagged where the macro is defined.
    let _ = (
        joined!(),
        label("x"),
        SPACE,
        MARK,
        RAW,
        C_STRING,
        TWO,
        OVERRIDE,
        ESCAPED,
        VISIBLE,
        BYTES,
        EMOJI,
        NOT_KEYCAP,
        HALF_JOINED,
        EXCEPTED,
    );
}

// Flagged: a line separator, a tag character, a Mongolian variation selector and a variation
// selector from the supplement, each written as itself.
const LINE: &str = "check out";

const TAG: &str = "check󠁁out";

const MONGOLIAN: &str = "check᠋out";

const SUPPLEMENT: &str = "check󠄀out";

// Not flagged: the flag of England, whose tag characters follow a black flag.
const ENGLAND: &str = "🏴󠁧󠁢󠁥󠁮󠁧󠁿";

// Flagged: a literal in a macro argument, which the expansion replaces, and in an attribute.
const CONCATENATED: &str = concat!("check", "​out");

#[must_use = "the price​ matters"]
fn price() -> u32 {
    1
}

fn more() {
    let _ = (LINE, TAG, MONGOLIAN, SUPPLEMENT, ENGLAND, CONCATENATED, price());
}
