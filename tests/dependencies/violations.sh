#!/usr/bin/env bash
# Applies one guide-violating change at a time to the project tests/dependencies/setup.sh set up,
# each a way past the lints that a review found, and checks that `just dylint` fails on it with the
# reports that name it. The macros trellis doesn't trust, clap's derives, `#[tokio::main]`,
# `#[tokio::test]` and `#[tracing::instrument]`, each get the code a review once hid under them.
# The project must pass every check before this runs; each change is undone before the next.
#
# Usage: tests/dependencies/violations.sh <project-dir>
#
# The reports quote code between backticks, which the expected texts below keep literal.
# shellcheck disable=SC2016
set -euo pipefail

if [[ $# -ne 1 ]]; then
    echo "usage: $0 <project-dir>" >&2
    exit 2
fi
cd "$1"
core=crates/demo_binary_core/src
binary=crates/demo_binary/src
if [[ ! -f $core/settings.rs || ! -f $binary/cli.rs ]]; then
    echo "$1 isn't a project tests/dependencies/setup.sh set up" >&2
    exit 2
fi

backup=$(mktemp -d)
trap 'rm -rf "$backup"' EXIT
cp -R crates Cargo.toml Cargo.lock dylint.toml "$backup/"
restore() {
    rm -rf crates
    cp -R "$backup/crates" "$backup/Cargo.toml" "$backup/Cargo.lock" "$backup/dylint.toml" .
}

failures=0
# `check <name> <report>...`: runs `just dylint` on the change just applied, which must fail with
# a report containing each `report`, then undoes the change.
check() {
    local name=$1
    shift
    local output
    if output=$(just dylint 2>&1); then
        echo "FAIL $name: \`just dylint\` passed" >&2
        failures=$((failures + 1))
    else
        local report missing=0
        for report in "$@"; do
            if [[ $output != *"$report"* ]]; then
                echo "FAIL $name: no report containing: $report" >&2
                missing=1
            fi
        done
        if ((missing)); then
            printf '%s\n' "$output" | grep -E '^(error|warning)' >&2 || true
            failures=$((failures + 1))
        else
            echo "ok   $name"
        fi
    fi
    restore
}

# `add_core`: adds the code on standard input to the library crate, in a module of its own.
add_core() {
    printf 'mod extra;\n' >>"$core/lib.rs"
    {
        printf '//! Code that breaks a rule.\n\n'
        cat
    } >"$core/extra.rs"
}

# `add_binary`: adds the code on standard input to the binary crate, in a module of its own.
add_binary() {
    printf 'mod extra;\n' >>"$binary/main.rs"
    {
        printf '//! Code that breaks a rule.\n\n'
        cat
    } >"$binary/extra.rs"
}

# `clap_derive <field>...`: turns on clap's derives and adds a parser with the fields given, each
# with its doc comment and `#[arg(..)]`, to the binary crate, followed by the items in
# `$clap_items`.
clap_items=''
clap_derive() {
    cargo add --quiet --package demo_binary clap --features derive
    {
        printf '/// Options.\n#[derive(Debug, clap::Parser)]\npub(crate) struct Options {\n'
        printf '%s\n' "$@"
        printf '}\n%s' "$clap_items"
    } | add_binary
}

# `edit <file> <old> <new>`: replaces the line `old` of `file` with `new`, which must be there.
edit() {
    local file=$1 old=$2 new=$3 line found=0
    local edited=()
    while IFS= read -r line || [[ -n $line ]]; do
        if [[ $line == "$old" ]]; then
            edited+=("$new")
            found=1
        else
            edited+=("$line")
        fi
    done <"$file"
    if ((!found)); then
        echo "no line \`$old\` in $file" >&2
        exit 1
    fi
    printf '%s\n' "${edited[@]}" >"$file"
}

add_core <<'RUST'
/// Two.
#[must_use]
pub const fn two() -> u8 {
    // Two is one more than one.
    1 + 1
}
RUST
check "a code comment" 'code comment'

add_core <<'RUST'
/// Two.
#[must_use]
pub const fn two() -> u8 {
    /* Two is one more than one. */
    1 + 1
}
RUST
check "a block comment" 'code comment'

# cargo-dylint before 6.3.2 hid the recipe's `-D warnings` from cargo, so after a run without it
# cargo kept that run's checks, and `just dylint` replayed their warnings and passed. The other two
# cases also fail if the levels move to `RUSTFLAGS`, which cargo ignores when
# `CARGO_ENCODED_RUSTFLAGS` is set and doesn't pass to a build script when a target is named.
#
# `warn_only <name>`: runs the lints without `-D warnings`, as a plain `cargo dylint` does, which
# must pass.
warn_only() {
    if ! cargo dylint --all --fail-on-no-libraries -- --locked --workspace --all-targets --all-features \
        >/dev/null 2>&1; then
        echo "FAIL $1: \`cargo dylint\` failed" >&2
        failures=$((failures + 1))
    fi
}
commented='/// Two.
#[must_use]
pub const fn two() -> u8 {
    // Two is one more than one.
    1 + 1
}'

add_core <<<"$commented"
warn_only "a code comment after a run that only warned"
check "a code comment after a run that only warned" 'code comment'

add_core <<<"$commented"
CARGO_ENCODED_RUSTFLAGS='' warn_only "a code comment, with CARGO_ENCODED_RUSTFLAGS set"
CARGO_ENCODED_RUSTFLAGS='' check "a code comment, with CARGO_ENCODED_RUSTFLAGS set" 'code comment'

host=$(rustc --version --verbose | sed -n 's/^host: //p')
cat >crates/demo_binary_core/build.rs <<'RUST'
//! Builds nothing.

fn main() {
    // Nothing to build.
}
RUST
CARGO_BUILD_TARGET=$host warn_only "a code comment in a build script, with a target named"
CARGO_BUILD_TARGET=$host check "a code comment in a build script, with a target named" 'code comment'

# Raw identifiers once hid code from the lints that compare a node with its source text.
add_core <<'RUST'
use std::sync::Arc;

use crate::Settings;

/// Shared settings.
pub struct Shared {
    /// The settings.
    pub settings: r#Arc<Settings>,
}
RUST
check "raw identifier: shared_ownership" 'raw identifier `r#Arc`' '`Arc` in a field type'

add_core <<'RUST'
/// Two counts.
#[must_use]
pub fn counts() -> (r#usize, usize) {
    (1, 2)
}
RUST
check "raw identifier: multi_field_tuple" 'raw identifier `r#usize`' 'return type holds a tuple of 2'

add_core <<'RUST'
/// Shouts.
#[must_use]
pub fn shout(names: &[String], loud: bool) -> usize {
    if loud { names.len() } else { 0 }
}

/// Shouts loudly.
#[must_use]
pub fn shout_loudly(names: &[String]) -> usize {
    r#shout(names, true)
}
RUST
check "raw identifier: bool_literal_argument" 'raw identifier `r#shout`' '`true` passed as a `bool` argument'

add_core <<'RUST'
use crate::Settings;

/// Saves the settings.
pub fn save(settings: &Settings) {
    let r#_saved = settings.to_json();
}
RUST
check "raw identifier: discarded_result" 'raw identifier `r#_saved`' 'binds a `Result` and never reads it'

add_core <<'RUST'
/// Parses a count.
///
/// # Errors
///
/// Returns an error if `text` isn't a number.
pub fn count(text: &str) -> Result<u64, std::num::ParseIntError> {
    let count = r#text.parse()?;
    Ok(count)
}
RUST
check "raw identifier: inferred_conversion_binding" 'raw identifier `r#text`' \
    '`let` binds the result of `.parse()` without a type'

add_core <<'RUST'
/// Counts the letters of the names that aren't empty.
#[must_use]
pub fn count(names: &[String]) -> usize {
    let mut count = 0;
    for name in r#names.iter().filter(|name| !name.is_empty()) {
        count += name.len();
    }
    count
}
RUST
check "raw identifier: iterator_chain_in_for" 'raw identifier `r#names`' \
    '`for` loop header builds a combinator chain'

add_core <<'RUST'
/// Doubles the values.
#[must_use]
pub fn doubled(values: &[u32]) -> Vec<u32> {
    let mut doubled = Vec::new();
    r#values.iter().for_each(|value| doubled.push(value * 2));
    doubled
}
RUST
check "raw identifier: mutating_iterator_closure" 'raw identifier `r#values`' \
    'closure passed to `for_each` mutates'

add_core <<'RUST'
use std::fs::read_to_string;
use std::io;
use std::path::Path;

/// Reads the file at `path`.
///
/// # Errors
///
/// Returns an error if the file can't be read.
pub fn read(path: &Path) -> io::Result<String> {
    r#read_to_string(path)
}
RUST
check "raw identifier: qualified_function_calls" 'raw identifier `r#read_to_string`' \
    'call to imported function `read_to_string` without its module'

add_core <<'RUST'
use crate::Settings;

/// Labels the text with the settings' name.
#[must_use]
pub fn label(text: String, settings: &Settings) -> String {
    r#text + " (" + &settings.name + ")"
}
RUST
check "raw identifier: string_concat_chain" 'raw identifier `r#text`' '4 strings joined with `+`'

# clap's derives write `allow(clippy::restriction, ..)` and other lint groups over the expressions
# in `#[arg(..)]`, so clippy never sees them; trellis doesn't trust them. A review found each of
# these forms got past an exemption for literals, paths and calls of paths.
untrusted_clap='which the macro `clap::Parser` from `clap_derive` writes'
clap_derive '    /// The port.' '    #[arg(long, default_value_t = "8080".parse::<u16>().unwrap())]' \
    '    pub(crate) port: u16,'
check "clap: default_value_t calls unwrap" "$untrusted_clap"

clap_derive '    /// The port.' '    #[arg(long, value_parser = |port: &str| -> Result<u16, String> {' \
    '        dbg!(port);' '        println!("{port}");' '        let _ = port.parse::<u32>();' \
    '        todo!()' '    })]' '    pub(crate) port: u16,'
check "clap: value_parser closure with dbg!, println!, todo! and let _" "$untrusted_clap"

clap_derive '    /// The name.' '    #[arg(long, default_value_t = String::from(""))]' \
    '    pub(crate) name: String,'
check "clap: String::from(\"\"), which clippy::manual_string_new reports" "$untrusted_clap"

clap_derive '    /// The port.' '    #[arg(long, default_value_t = u16::from(8080_u16))]' \
    '    pub(crate) port: u16,'
check "clap: u16::from(8080_u16), a useless conversion" "$untrusted_clap"

clap_derive '    /// The port.' \
    '    #[arg(long, default_value_t = std::panic::catch_unwind(|| 8080_u16).unwrap_or_default())]' \
    '    pub(crate) port: u16,'
check "clap: panic::catch_unwind, which clippy::disallowed_methods reports" "$untrusted_clap"

clap_derive '    /// The limit.' '    #[arg(long, default_value_t = 1000000)]' '    pub(crate) limit: u32,' \
    '    /// The mask.' '    #[arg(long, default_value_t = 0xAbCd)]' '    pub(crate) mask: u16,'
check "clap: unreadable and mixed-case literals" "$untrusted_clap"

clap_derive '    /// The port.' '    #[arg(long, default_value_t = (8080_u32)as u16)]' \
    '    pub(crate) port: u16,'
check "clap: a truncating cast written without spaces" "$untrusted_clap"

clap_derive '    /// The port.' '    #[arg(long, default_value_t = std:: dbg!(8080_u16))]' \
    '    pub(crate) port: u16,'
check "clap: dbg! through a path with a space" "$untrusted_clap"

clap_items='
/// The port `PORT` names.
const fn port<const PORT: u16>() -> u16 {
    PORT
}
'
clap_derive '    /// The port.' \
    '    #[arg(long, default_value_t = port::<{ std::num::NonZeroU16::new(8080).unwrap().get() }>())]' \
    '    pub(crate) port: u16,'
check "clap: unwrap in a generic const argument" "$untrusted_clap"

clap_items='
/// A port of at least `MIN`.
#[derive(Debug, Clone)]
pub(crate) struct Port<const MIN: u16>(u16);

impl<const MIN: u16> std::str::FromStr for Port<MIN> {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        text.parse::<u16>().map(Self).map_err(|error| error.to_string())
    }
}
'
clap_derive '    /// The port.' \
    '    #[arg(long, value_parser = clap::value_parser!(Port<{ std::num::NonZeroU16::new(1).unwrap().get() }>))]' \
    '    pub(crate) port: Port<1>,'
check "clap: unwrap in value_parser!'s type" "$untrusted_clap"
clap_items=''

# `#[tracing::instrument]` writes `allow`s around the function's body, which once hid
# `print_stdout`, `uninlined_format_args`, `items_after_statements`,
# `semicolon_if_nothing_returned`, `needless_return` and `unused_async` there.
add_core <<'RUST'
/// Shouts the name.
#[tracing::instrument]
pub async fn shout(name: &str) {
    let loud = name.to_uppercase();
    println!("{}", loud);
    fn quiet() {}
    quiet();
    return;
}
RUST
check "tracing: #[tracing::instrument] over a body that breaks six lints" \
    'which the macro `tracing::instrument` from `tracing_attributes` writes'

# `#[tokio::main]` and `#[tokio::test]` write `allow`s that once hid `needless_return` and
# `unused_async` in the function's body.
cargo add --quiet --package demo_binary tokio --features macros
cargo update --quiet --package tokio-macros --precise 2.7.2
cat >"$binary/main.rs" <<'RUST'
//! `demo-binary`: A demo binary that trellis generates for its tests.

mod cli;
mod greeting;
mod run;

use crate::cli::Cli;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    return run::run(Cli::parse()).await;
}
RUST
check "tokio: #[tokio::main] over a needless return" \
    'which the macro `tokio::main` from `tokio_macros` writes' \
    'write the code without `tokio::main`, as the idioms in `docs/style.md` show'

cargo add --quiet --package demo_binary_core --dev tokio --features macros
cargo update --quiet --package tokio-macros --precise 2.7.2
cat >>crates/demo_binary_core/tests/settings.rs <<'RUST'

#[tokio::test]
async fn parses_without_awaiting() {
    return assert!(Settings::from_json("{}").is_err(), "empty settings name nobody");
}
RUST
check "tokio: #[tokio::test] over an async test that never awaits" \
    'which the macro `tokio::test` from `tokio_macros` writes'

# A test attribute takes no arguments.
cargo add --quiet --package demo_binary_core --dev tokio --features macros
cargo update --quiet --package tokio-macros --precise 2.7.2
cat >>crates/demo_binary_core/tests/settings.rs <<'RUST'

#[tokio::test(flavor = "multi_thread")]
async fn parses_on_two_threads() {
    assert!(Settings::from_json("{}").is_err(), "empty settings name nobody");
}
RUST
check "tokio: an argument to a test attribute" 'test attribute with the argument `flavor`'

# thiserror writes `allow(unused_variables, deprecated, ..)` over an enum's messages and their
# format arguments, which once hid a deprecated call in an argument.
add_core <<'RUST'
/// Why a name was refused.
#[derive(Debug, thiserror::Error)]
pub enum NameError {
    /// The name is blank.
    #[error("the name {:?} is blank", str::trim_left(.name))]
    Blank {
        /// The name.
        name: String,
    },
}
RUST
check "thiserror: a format argument after the message" \
    'that a macro from `thiserror_impl` writes, which `trusted_macro_lints` in `dylint.toml` doesn'"'"'t allow there'

# A path serde parses from a string the caller wrote has the string's span, which a derive's own
# paths once shared as a way past the lints.
add_core <<'RUST'
use serde::Deserialize;

/// A label.
#[derive(Debug, Deserialize)]
pub struct Label {
    /// The text.
    #[serde(default = "std::string::String::new")]
    pub text: String,
}
RUST
check "serde: a type through its module in a string path" \
    '`String` is named through its module' \
    'that a macro from `serde_derive` writes, which `trusted_macro_lints` in `dylint.toml` doesn'"'"'t allow there'

add_core <<'RUST'
use serde::Deserialize;

/// Labels.
#[derive(Debug, Deserialize)]
pub struct Labels {
    /// The texts.
    #[serde(default = "serde_json::__private::vec::Vec::new")]
    pub texts: Vec<String>,
}
RUST
check "serde: a hidden item in a string path" \
    'is `#[doc(hidden)]` in `serde_json`'

# serde adds `deserialize` to the path in a `with` string at the derive's span, which gives the
# path, and the type in it, that span, so the path once passed as code serde wrote.
add_core <<'RUST'
use serde::Deserialize;

/// Labels.
#[derive(Debug, Deserialize)]
pub struct Labels {
    /// The texts.
    #[serde(with = "serde_json::__private::vec::Vec")]
    pub texts: Vec<String>,
}
RUST
check "serde: a hidden module and a type through its module in a with string" \
    '`serde_json::__private` is `#[doc(hidden)]` in `serde_json`' '`Vec` is named through its module'

# serde reads and builds the type a `remote` names at its own spans, with the caller's names for
# its fields, so a hidden field once passed as one serde named.
cargo add --quiet --package demo_binary serde
add_binary <<'RUST'
use std::cell::RefCell;
use std::thread::LocalKey;

use serde::{Serialize, Serializer};

/// A key's mirror.
#[derive(Serialize)]
#[serde(remote = "tokio::task::LocalKey<u8>")]
pub(crate) struct Key {
    /// The key it wraps.
    #[serde(serialize_with = "unit")]
    inner: LocalKey<RefCell<Option<u8>>>,
}

/// Writes nothing for `key`.
fn unit<S: Serializer>(_key: &LocalKey<RefCell<Option<u8>>>, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_unit()
}
RUST
check "serde: a hidden field of a remote type" \
    '`tokio::task::LocalKey::inner` is `#[doc(hidden)]` in `tokio`'

# A string whose text differs from the path serde parses from it once hid the path from the lints
# that compare the two: a comment, an escape, a raw identifier or an empty turbofish.
serde_string() {
    add_core <<RUST
use serde::Deserialize;

/// A count.
#[derive(Debug, Deserialize)]
pub struct Count {
    /// The number.
    #[serde(default = "$1")]
    pub number: $2,
}

pub(crate) mod defaults {
    pub(crate) fn zero() -> u32 {
        0
    }
}
RUST
}
serde_string 'std::string::/**/String::new' String
check "serde: a comment in a string path" '`String` is named through its module' \
    'that a macro from `serde_derive` writes, which `trusted_macro_lints` in `dylint.toml` doesn'"'"'t allow there'

serde_string 'std::string::Str\u{69}ng::new' String
check "serde: an escape in a string path" '`String` is named through its module' \
    'that a macro from `serde_derive` writes, which `trusted_macro_lints` in `dylint.toml` doesn'"'"'t allow there'

serde_string 'serde_json::/**/__private::vec::/**/Vec::new' 'Vec<String>'
check "serde: a hidden item behind comments in a string path" 'is `#[doc(hidden)]` in `serde_json`'

for path in 'crate::extra::defaults::zero::<>' 'crate::extra::defaults::r#zero' 'defaults::/**/zero'; do
    serde_string "$path" u32
    check "serde: $path" \
        'that a macro from `serde_derive` writes, which `trusted_macro_lints` in `dylint.toml` doesn'"'"'t allow there'
done

# serde calls a path in a string under `allow(unused_qualifications, clippy::absolute_paths)`, so a
# path of three segments fails, and one of two whose last segment by itself names the same item.
serde_string 'crate::extra::defaults::zero' u32
check "serde: a path of three segments" \
    'that a macro from `serde_derive` writes, which `trusted_macro_lints` in `dylint.toml` doesn'"'"'t allow there'

add_core <<'RUST'
use serde::Deserialize;

use self::defaults::zero;

/// A count.
#[derive(Debug, Deserialize)]
pub struct Count {
    /// The number.
    #[serde(default = "defaults::zero")]
    pub number: u32,
}

/// Zero, the other way.
#[must_use]
pub fn none() -> u32 {
    zero()
}

pub(crate) mod defaults {
    pub(crate) fn zero() -> u32 {
        0
    }
}
RUST
check "serde: a path whose last segment is also imported" \
    'that a macro from `serde_derive` writes, which `trusted_macro_lints` in `dylint.toml` doesn'"'"'t allow there'

# Types and generic arguments in a serde string sit under the same `allow`s.
add_core <<'RUST'
use serde::Deserialize;

/// A wrapper.
#[derive(Debug, Deserialize)]
#[serde(bound(deserialize = "T: Deserialize<'de> + Into<std::io::Error>"))]
pub struct Wrapper<T> {
    /// The value.
    pub value: T,
}
RUST
check "serde: a bound with generic arguments and a path through its module" \
    'that a macro from `serde_derive` writes, which `trusted_macro_lints` in `dylint.toml` doesn'"'"'t allow there'

add_core <<'RUST'
use serde::{Deserialize, Deserializer};

/// A size.
#[derive(Debug, Deserialize)]
pub struct Size {
    /// The value.
    #[serde(deserialize_with = "sized::<{ std::mem::size_of::<u64>() }, _>")]
    pub value: u64,
}

fn sized<'de, const N: usize, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
    u64::deserialize(deserializer).map(|value| value + N as u64)
}
RUST
check "serde: a generic const argument in a string path" \
    'that a macro from `serde_derive` writes, which `trusted_macro_lints` in `dylint.toml` doesn'"'"'t allow there'

add_core <<'RUST'
use serde::{Deserialize, Deserializer};

/// A flag.
#[derive(Debug, Deserialize)]
pub struct Flag {
    /// Whether it is set.
    #[serde(with = "self::flag")]
    pub set: bool,
}

mod flag {
    use super::{Deserialize, Deserializer};

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<bool, D::Error> {
        u8::deserialize(deserializer).map(|value| value != 0)
    }
}
RUST
check "serde: a module through self in a with string" \
    'that a macro from `serde_derive` writes, which `trusted_macro_lints` in `dylint.toml` doesn'"'"'t allow there'

# The compiler reads an identifier in its composed Unicode form, so a name written decomposed in a
# string, `e` and a combining accent, once never matched the name it spells.
add_core <<'RUST'
use serde::Deserialize;

use self::accents::Café;

/// An order.
#[derive(Debug, Deserialize)]
#[serde(from = "accents::Cafe\u{301}")]
pub struct Order {
    /// The number.
    pub number: u32,
}

impl Order {
    /// The order for `café`.
    #[must_use]
    pub fn from_cafe(café: Café) -> Self {
        Self { number: café.number }
    }
}

impl From<Café> for Order {
    fn from(café: Café) -> Self {
        Self::from_cafe(café)
    }
}

mod accents {
    use serde::Deserialize;

    /// A café's order.
    #[derive(Debug, Deserialize)]
    pub(crate) struct Café {
        /// The number.
        pub(crate) number: u32,
    }
}
RUST
check "serde: a decomposed identifier in a string path" '`Café` is named through its module'

# A macro that swaps one identifier for another keeps every token's span, so the `allow` it builds
# once passed as one the source holds.
cargo add --quiet --package demo_binary_core duplicate@=2.0.1
add_core <<'RUST'
/// One.
#[must_use]
pub fn one() -> u8 {
    duplicate::duplicate! {
        [quiet; [allow]]
        #[quiet(clippy::unwrap_used)]
        let number = "1".parse::<u8>().unwrap();
    }
    number
}
RUST
check "duplicate!: an allow it builds from swapped identifiers" \
    "\`allow\` attribute, which a macro built from tokens that aren't an attribute in the source"

# A Linux-only project, such as a service, builds no Windows or macOS code.
edit dylint.toml 'ci_targets = ["linux", "macos", "windows"]' 'ci_targets = ["linux"]'
add_core <<'RUST'
/// The path separator.
#[cfg(windows)]
pub const SEPARATOR: char = '\\';
/// The path separator.
#[cfg(target_os = "macos")]
pub const SEPARATOR: char = '/';
RUST
check "cfg: a platform the project doesn't build" \
    "predicate on \`windows\`, a target CI doesn't build" \
    "predicate on \`target_os = \"macos\"\`, a target CI doesn't build"

# `cfg_select!` once hid its arms from the `cfg` rules, and `cfg(proc_macro)` from both lint runs.
add_core <<'RUST'
cfg_select! {
    any(clippy, dylint_lib = "trellis_lints") => {
        /// The answer.
        #[must_use]
        pub fn answer() -> u8 {
            42
        }
    }
    _ => {
        /// The answer.
        #[must_use]
        pub fn answer() -> u8 {
            "42".parse().unwrap()
        }
    }
}
RUST
check "cfg_select!: an arm only one lint run selects" '`cfg_select!` predicate on `clippy`'

edit dylint.toml 'ci_targets = ["linux", "macos", "windows"]' 'ci_targets = ["linux"]'
add_core <<'RUST'
cfg_select! {
    windows => {
        /// The shell.
        pub const SHELL: &str = "cmd";
    }
    _ => {
        /// The shell.
        pub const SHELL: &str = "sh";
    }
}
RUST
check "cfg_select!: an arm on a platform the project doesn't build" \
    "\`cfg_select!\` predicate on \`windows\`, a target CI doesn't build"

edit dylint.toml 'ci_targets = ["linux", "macos", "windows"]' 'ci_targets = ["linux"]'
add_core <<'RUST'
cfg_select! {
    unix => {
        /// The variable that names the home directory.
        pub const HOME: &str = "HOME";
    }
    _ => {
        /// The variable that names the home directory.
        pub const HOME: &str = "USERPROFILE";
    }
}
RUST
check "cfg_select!: a _ arm no built platform selects" \
    '`cfg_select!` arm that no target CI builds selects'

# A `cfg_select!` or `cfg!` imported under another name once hid its predicates from the lint, which
# finds their calls by name.
add_core <<'RUST'
use std::cfg_select as pick;

pick! {
    any(clippy, dylint_lib = "trellis_lints") => {
        /// The answer.
        #[must_use]
        pub fn answer() -> u8 {
            42
        }
    }
    _ => {
        /// The answer.
        #[must_use]
        pub fn answer() -> u8 {
            "42".parse().unwrap()
        }
    }
}
RUST
check "cfg_select!: imported under another name" '`use` of `cfg_select!`'

add_core <<'RUST'
use std::cfg as flag;

/// Whether this is the clippy run.
#[must_use]
pub fn linting() -> bool {
    flag!(clippy)
}
RUST
check "cfg!: imported under another name" '`use` of `cfg!`'

add_core <<'RUST'
/// Expands.
#[cfg(proc_macro)]
pub fn expand() {
    let _ = "1".parse::<u8>();
}
RUST
check "cfg: proc_macro, which no crate of the workspace sets" '`cfg` predicate on `proc_macro`'

# A derive in a function once counted as a use of the module's import of its trait by name alone,
# though the function's own import is the one the derive resolves through.
add_core <<'RUST'
use serde::{Deserialize, Deserializer};
use serde_json::Value;

/// The number in `value`, if it holds one.
#[must_use]
pub fn count(value: Value) -> Option<u32> {
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct Count {
        number: u32,
    }

    serde_json::from_value::<Count>(value).ok().map(|count| count.number)
}

/// The string `deserializer` holds.
///
/// # Errors
///
/// Fails if it holds no string.
pub fn text<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    String::deserialize(deserializer)
}
RUST
check "serde: a derive in a function, through the function's import" \
    'trait `Deserialize` is imported for the whole module but used only in `text`'

# serde parses the type of `into` from the string with the string's span, so an attribute in it
# once passed as one serde wrote: an `allow` as a trusted level, a `cfg` unseen.
add_core <<'RUST'
use serde::Serialize;

/// Nothing.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(into = "[(); { #[allow(warnings, clippy::all)] fn hidden() { let _ = 1_usize.checked_sub(2).unwrap(); } 0 }]")]
pub struct Muted;

impl From<Muted> for [(); 0] {
    fn from(_: Muted) -> Self {
        []
    }
}
RUST
check "serde: an allow in a string serde parses as a type" \
    '`allow` attribute, which a macro built from tokens that aren'"'"'t an attribute in the source'

add_core <<'RUST'
use serde::Serialize;

/// Nothing.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(into = "[(); { #[cfg(not(any(clippy, dylint_lib = \"trellis_lints\")))] fn hidden() { let _ = 1_usize.checked_sub(2).unwrap(); } 0 }]")]
pub struct Hidden;

impl From<Hidden> for [(); 0] {
    fn from(_: Hidden) -> Self {
        []
    }
}
RUST
check "serde: a cfg in a string serde parses as a type" \
    '`cfg` attribute in a string that a macro parses as code'

add_core <<'RUST'
use serde::{Deserialize, Deserializer};

/// A number.
#[derive(Debug, Deserialize)]
#[serde(transparent)]
pub struct Quiet {
    /// The number.
    #[serde(deserialize_with = "/*{read}*/ #[cfg_attr(clippy, allow(clippy::all))] read")]
    pub value: u32,
}

fn read<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    u32::deserialize(deserializer)
}
RUST
check "serde: a cfg in a string serde calls, with a placeholder spelling the name" \
    '`cfg_attr` attribute in a string that a macro parses as code' \
    'code written in the source under the lint levels of'

add_core <<'RUST'
use serde::{Deserialize, Deserializer};

/// A number.
#[derive(Debug, Deserialize)]
#[serde(transparent)]
pub struct Spelled {
    /// The number.
    #[serde(deserialize_with = "/*{read}*/ read")]
    pub value: u32,
}

fn read<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    u32::deserialize(deserializer)
}
RUST
check "serde: code a placeholder spells in a string serde calls" \
    'code written in the source under the lint levels of'

add_core <<'RUST'
use serde::Serialize;

/// A number.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Shown(u8);

/// Nothing.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(into = "/*{Shown}*/ Shown")]
pub struct Spelled;

impl From<Spelled> for Shown {
    fn from(_: Spelled) -> Self {
        Self(0)
    }
}
RUST
check "serde: a type a placeholder spells in a string serde parses" \
    'code written in the source under the lint levels of'

add_core <<'RUST'
use serde::Serialize;

/// A number.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Shown(u8);

/// Nothing.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(into = "/*{x:0Shown$}*/ Shown")]
pub struct Widened;

impl From<Widened> for Shown {
    fn from(_: Widened) -> Self {
        Self(0)
    }
}
RUST
check "serde: a type a width placeholder spells in a string serde parses" \
    'code written in the source under the lint levels of'

add_core <<'RUST'
use serde::Serialize;

/// A length.
pub trait Len {
    /// The length.
    const LEN: usize;
}

impl<const N: usize> Len for [(); N] {
    const LEN: usize = N;
}

/// Two.
#[must_use]
pub const fn size() -> usize {
    2
}

/// Nothing.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(into = "/*{LEN}*/ [(); <[(); { crate::extra::size() }]>::LEN]")]
pub struct Qualified;

impl From<Qualified> for [(); 2] {
    fn from(_: Qualified) -> Self {
        [(); 2]
    }
}
RUST
check "serde: code in the qualified self of a constant a placeholder spells" \
    'code written in the source under the lint levels of'

add_core <<'RUST'
use serde::Serialize;

/// A length.
pub trait Len {
    /// The length.
    const LEN: usize;
}

/// Two.
#[derive(Debug, Clone, Copy)]
pub struct Hidden;

impl Len for Hidden {
    const LEN: usize = 2;
}

/// Nothing.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(into = "/*{LEN}{Hidden}*/ [(); <Hidden>::LEN]")]
pub struct Qualified;

impl From<Qualified> for [(); 2] {
    fn from(_: Qualified) -> Self {
        [(); 2]
    }
}
RUST
check "serde: a qualified self a placeholder spells" \
    'code written in the source under the lint levels of'

add_core <<'RUST'
use serde::Serialize;

/// Nothing.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(into = "[(); { if cfg!(test) { 2 } else { 2 } }]")]
pub struct Tested;

impl From<Tested> for [(); 2] {
    fn from(_: Tested) -> Self {
        [(); 2]
    }
}
RUST
check "serde: a cfg! in a string serde parses as a type" \
    '`cfg!` in a string that a macro parses as code'

if ((failures)); then
    echo "$failures violations got past the checks" >&2
    exit 1
fi
