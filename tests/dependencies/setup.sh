#!/usr/bin/env bash
# Adds the crates most projects start with to a freshly generated binary project named
# `demo-binary`, with a little code that uses each, the way a user would. The project's checks then
# show whether the lints and the policy accept ordinary code that depends on them.
#
# Usage: tests/dependencies/setup.sh <project-dir>
set -euo pipefail

if [[ $# -ne 1 ]]; then
    echo "usage: $0 <project-dir>" >&2
    exit 2
fi
cd "$1"
core=crates/demo_binary_core
binary=crates/demo_binary
if [[ ! -f $core/src/version_line.rs || ! -f $binary/Cargo.toml ]]; then
    echo "$1 isn't a binary project named demo-binary" >&2
    exit 2
fi

# Exact versions keep what the lints see the same from run to run.
dependencies='[workspace.dependencies]
anyhow = "=1.0.104"
clap = "=4.6.8"
futures = "=0.3.34"
serde = { version = "=1.0.229", features = ["derive"] }
serde_json = "=1.0.154"
thiserror = "=2.0.21"
tokio = { version = "=1.54.1", features = ["rt-multi-thread", "time"] }
tracing = "=0.1.44"'
manifest=$(<Cargo.toml)
if [[ $manifest != *$'\n[workspace.dependencies]\n'* ]]; then
    echo "Cargo.toml has no empty [workspace.dependencies] table" >&2
    exit 1
fi
printf '%s\n' "${manifest/$'\n[workspace.dependencies]\n'/$'\n'$dependencies$'\n'}" >Cargo.toml

cargo add --quiet --package demo_binary_core serde serde_json thiserror tracing
cargo add --quiet --package demo_binary_core --dev tokio
cargo add --quiet --package demo_binary anyhow clap futures tokio tracing
# The proc-macro crates their parents don't pin exactly, held at the versions whose output
# `trusted_macro_crates` in the template's `dylint.toml` was read at, and at the version whose
# `#[tracing::instrument]` tests/dependencies/violations.sh checks is rejected.
cargo update --quiet --package serde_derive --precise 1.0.229
cargo update --quiet --package tracing-attributes --precise 0.1.31

cat >$core/src/lib.rs <<'EOF'
//! The logic behind `demo-binary`, apart from its binary so tests can use it.

mod byte_string;
mod defaults;
mod fallback;
mod nickname;
mod settings;
mod settings_error;
mod tags;
mod text;
mod version_line;

pub use byte_string::ByteString;
pub use fallback::Fallback;
pub use nickname::Nickname;
pub use settings::Settings;
pub use settings_error::SettingsError;
pub use tags::Tags;
pub use text::Text;
pub use version_line::version_line;
EOF

cat >$core/src/settings.rs <<'EOF'
//! What `demo-binary` reads from its settings file.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{SettingsError, defaults};

/// The settings `demo-binary` runs with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    /// Who to greet.
    pub name: String,
    /// How many times to try before giving up, three if the file doesn't say.
    #[serde(default = "defaults::retries")]
    pub retries: u32,
    /// What to call them instead, if anything.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nickname: Option<String>,
}

impl Settings {
    /// Reads the settings from the JSON file at `path`.
    ///
    /// # Errors
    ///
    /// Returns an error if the file can't be read or isn't JSON settings.
    pub fn load(path: &Path) -> Result<Self, SettingsError> {
        let _span = tracing::info_span!("load", path = %path.display()).entered();
        let text = fs::read_to_string(path)?;
        let settings = Self::from_json(&text)?;
        tracing::info!(name = %settings.name, "loaded the settings");
        Ok(settings)
    }

    /// Parses settings from JSON.
    ///
    /// # Errors
    ///
    /// Returns an error if `text` isn't JSON settings.
    pub fn from_json(text: &str) -> Result<Self, SettingsError> {
        Ok(serde_json::from_str(text)?)
    }

    /// Whether the settings name nobody in particular.
    #[must_use]
    pub fn is_anonymous(&self) -> bool {
        matches!(self.name.trim(), "" | "anonymous")
    }

    /// The settings as JSON.
    ///
    /// # Errors
    ///
    /// Returns an error if the settings can't be written as JSON.
    pub fn to_json(&self) -> Result<String, SettingsError> {
        Ok(serde_json::to_string(self)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Deserialize)]
    struct Retries {
        #[serde(default = "defaults::retries")]
        retries: u32,
    }

    #[test]
    fn defaults_the_retries_alone() {
        let read: Retries = serde_json::from_str("{}").unwrap();
        assert_eq!(read.retries, 3, "the retries should default to three");
    }
}
EOF

# serde parses types and bounds from strings, which pass as long as every path in them is short.
cat >$core/src/fallback.rs <<'EOF'
//! A value that falls back to its default when the settings don't give it.

use serde::{Deserialize, Serialize};

/// A value that falls back to its default when the settings don't give it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "T: Serialize",
    deserialize = "T: Deserialize<'de> + Default"
))]
pub struct Fallback<T> {
    /// The value, or its default.
    #[serde(default)]
    pub value: T,
}
EOF

cat >$core/src/byte_string.rs <<'EOF'
//! Bytes the settings write as a list of numbers.

use serde::{Deserialize, Serialize};

/// Bytes the settings write as a list of numbers, at most 16 of them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Vec<u8>", into = "Vec<u8>")]
pub struct ByteString(Vec<u8>);

impl ByteString {
    /// The longest a byte string may be.
    pub const MAX_LEN: usize = 16;

    /// `bytes` as a byte string.
    ///
    /// # Errors
    ///
    /// Returns an error if there are more than `MAX_LEN` bytes.
    pub fn try_new(bytes: Vec<u8>) -> Result<Self, String> {
        let len = bytes.len();
        if len > Self::MAX_LEN {
            return Err(format!("{len} bytes is more than {}", Self::MAX_LEN));
        }
        Ok(Self(bytes))
    }
}

impl TryFrom<Vec<u8>> for ByteString {
    type Error = String;

    fn try_from(bytes: Vec<u8>) -> Result<Self, String> {
        Self::try_new(bytes)
    }
}

impl From<ByteString> for Vec<u8> {
    fn from(bytes: ByteString) -> Self {
        bytes.0
    }
}
EOF

cat >$core/src/tags.rs <<'EOF'
//! The tags the settings give, kept sorted.

use serde::{Deserialize, Serialize};

/// The tags the settings give, sorted and without repeats.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "Vec<String>", into = "Vec<String>")]
pub struct Tags(Vec<String>);

impl Tags {
    /// `tags`, sorted and without repeats.
    #[must_use]
    pub fn new(mut tags: Vec<String>) -> Self {
        tags.sort_unstable();
        tags.dedup();
        Self(tags)
    }
}

impl From<Vec<String>> for Tags {
    fn from(tags: Vec<String>) -> Self {
        Self::new(tags)
    }
}

impl From<Tags> for Vec<String> {
    fn from(tags: Tags) -> Self {
        tags.0
    }
}
EOF

cat >$core/src/nickname.rs <<'EOF'
//! What to call someone, if anything.

use serde::Deserialize;

/// What to call someone: a name the settings give, or a placeholder.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(from = "Option<String>")]
pub struct Nickname(String);

impl Nickname {
    /// `name`, or `friend` if there is none.
    #[must_use]
    pub fn new(name: Option<String>) -> Self {
        Self(name.unwrap_or_else(|| "friend".to_owned()))
    }
}

impl From<Option<String>> for Nickname {
    fn from(name: Option<String>) -> Self {
        Self::new(name)
    }
}
EOF

# serde's own example of a bound on an associated type, `T::Err`, on a field whose
# `deserialize_with` function needs the `Deserialize` the derive also needs in scope, and a derive in
# a function that reads an older form through `flatten`.
cat >$core/src/text.rs <<'EOF'
//! Values the settings write as text.

use std::fmt::Display;
use std::str::FromStr;

use serde::{Deserialize, Deserializer};

/// A value the settings write as text, such as a port written `"8080"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct Text<T> {
    /// The value the text spells.
    #[serde(bound(deserialize = "T: FromStr, T::Err: Display"))]
    #[serde(deserialize_with = "from_text")]
    pub value: T,
}

impl Text<u16> {
    /// Reads a port from JSON, ignoring the `name` that older settings gave beside it.
    ///
    /// # Errors
    ///
    /// Returns an error if `json` isn't a port written as text.
    pub fn port_from_json(json: &str) -> Result<Self, serde_json::Error> {
        #[derive(Deserialize)]
        struct Legacy {
            #[serde(flatten)]
            port: Text<u16>,
        }

        let legacy: Legacy = serde_json::from_str(json)?;
        Ok(legacy.port)
    }
}

/// Reads a string and parses it as a `T`.
fn from_text<'de, D: Deserializer<'de>, T: FromStr>(deserializer: D) -> Result<T, D::Error>
where
    T::Err: Display,
{
    use serde::de::Error as _;

    let text = String::deserialize(deserializer)?;
    text.parse().map_err(D::Error::custom)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_port_written_as_text() {
        let port = Text::port_from_json(r#"{"value": "8080", "name": "web"}"#).unwrap();
        assert_eq!(
            port.value, 8080,
            "the port should be the number the text spells"
        );
    }

    #[test]
    fn rejects_text_that_isnt_a_port() {
        assert!(
            Text::port_from_json(r#"{"value": "web"}"#).is_err(),
            "`web` isn't a port"
        );
    }
}
EOF

cat >$core/src/defaults.rs <<'EOF'
//! The values the settings take when the file doesn't give them.

/// How many times to try when the settings don't say.
pub(crate) fn retries() -> u32 {
    3
}
EOF

cat >$core/src/settings_error.rs <<'EOF'
//! Why the settings couldn't be read.

use std::io;

/// Why the settings couldn't be read.
#[derive(Debug, thiserror::Error)]
#[must_use]
pub enum SettingsError {
    /// The settings file couldn't be read.
    #[error("couldn't read the settings file")]
    Read(#[from] io::Error),
    /// The settings aren't valid JSON settings.
    #[error(transparent)]
    Parse(#[from] serde_json::Error),
    /// The settings name nobody to greet.
    #[error("the settings name nobody to greet: {name}")]
    NoName {
        /// The name the settings hold.
        name: String,
    },
    /// The settings set a key that only tests read.
    #[error("`{key}`, written {key:?}, is read only under #[cfg(test)]")]
    TestKey {
        /// The key the settings set.
        key: String,
    },
    /// The settings set a section that only tests read.
    #[error("the section {0:>5} is read only when cfg!(test) holds")]
    TestSection(String),
}
EOF

cat >$core/tests/settings.rs <<'EOF'
//! Tests for `Settings`.

use demo_binary_core::Settings;
use tokio::runtime::Builder;
use tokio::task;

/// Runs `future` to completion on a runtime of its own.
fn block_on<F: Future>(future: F) -> F::Output {
    Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("the test runtime should build")
        .block_on(future)
}

#[test]
fn reads_back_what_it_writes() {
    block_on(async {
        let settings = Settings {
            name: "Ferris".to_owned(),
            retries: 3,
            nickname: None,
        };
        let json = settings.to_json().unwrap();
        let read = task::spawn_blocking(move || Settings::from_json(&json))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(read, settings, "the settings should survive a round trip");
    });
}

#[test]
fn defaults_the_retries() {
    let settings = Settings::from_json(r#"{"name": "Ferris"}"#).unwrap();
    assert_eq!(settings.retries, 3, "the retries should default to three");
}

#[test]
fn rejects_text_that_isnt_json() {
    assert!(
        Settings::from_json("name = 'Ferris'").is_err(),
        "TOML isn't JSON settings"
    );
}
EOF

cat >$binary/src/main.rs <<'EOF'
//! `demo-binary`: A demo binary that trellis generates for its tests.

mod cli;
mod greeting;
mod run;

use tokio::runtime::Builder;

use crate::cli::Cli;

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(run::run(cli))
}
EOF

cat >$binary/src/run.rs <<'EOF'
//! What `demo-binary` does once it has parsed its command line.

use std::time::Duration;
use std::{io, pin};

use demo_binary_core::Settings;
use futures::future::{self, Either};
use tokio::task::{self, JoinSet};
use tokio::time;

use crate::cli::Cli;

/// How long reading the settings and counting may take.
const DEADLINE: Duration = Duration::from_secs(5);

/// Greets whoever the settings `cli` names hold.
pub(crate) async fn run(cli: Cli) -> anyhow::Result<()> {
    use std::io::Write as _;

    use tracing::Instrument as _;

    tracing::info!(port = cli.port, greeting = ?cli.greeting, "parsed the command line");
    let load = pin::pin!(
        task::spawn_blocking(move || Settings::load(&cli.settings))
            .instrument(tracing::info_span!("load_settings"))
    );
    let settings = time::timeout(DEADLINE, load).await???;
    anyhow::ensure!(
        settings.retries > 0,
        "the settings must allow at least one try"
    );
    tracing::info!(retries = settings.retries, "starting");
    let words: Vec<String> = settings
        .name
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    let (letters, first) = future::join(letters(&settings.name), first_greeted(words)).await;
    tracing::info!(letters = letters?, ?first, "counted the letters");
    let mut stdout = io::stdout().lock();
    writeln!(
        stdout,
        "{}: hello, {}",
        demo_binary_core::version_line(),
        settings.name
    )?;
    Ok(())
}

/// How many letters `name` has outside its spaces, counted on one task for the whole name and on
/// another word by word, which must agree.
///
/// # Errors
///
/// Returns an error if a task fails, the counts differ, or counting takes longer than `DEADLINE`.
async fn letters(name: &str) -> anyhow::Result<usize> {
    let whole = name.to_owned();
    let words: Vec<String> = name.split_whitespace().map(str::to_owned).collect();
    let total = task::spawn(async move {
        whole
            .chars()
            .filter(|letter| !letter.is_whitespace())
            .count()
    });
    let per_word = task::spawn(future::join_all(
        words
            .into_iter()
            .map(|word| async move { word.chars().count() }),
    ));
    let (total, per_word) = time::timeout(DEADLINE, future::try_join(total, per_word)).await??;
    anyhow::ensure!(
        total == per_word.iter().sum::<usize>(),
        "the counts of the letters differ"
    );
    Ok(total)
}

/// The first of `names` that a task of its own hands back, or `None` if none does before
/// `DEADLINE`.
async fn first_greeted(names: Vec<String>) -> Option<String> {
    let mut tasks = JoinSet::new();
    for name in names {
        tasks.spawn(async move { name });
    }
    let first = pin::pin!(tasks.join_next());
    let deadline = pin::pin!(time::sleep(DEADLINE));
    match future::select(first, deadline).await {
        Either::Left((Some(Ok(name)), _)) => Some(name),
        Either::Left(_) | Either::Right(_) => None,
    }
}
EOF

cat >$binary/src/cli.rs <<'EOF'
//! The command line `demo-binary` takes.

use std::path::PathBuf;

use clap::{Arg, ArgMatches, Command};

use crate::greeting::Greeting;

/// The command line `demo-binary` takes.
#[derive(Debug)]
pub(crate) struct Cli {
    /// The JSON settings file to read.
    pub(crate) settings: PathBuf,
    /// The port to report.
    pub(crate) port: u16,
    /// How to greet.
    pub(crate) greeting: Greeting,
}

impl Cli {
    /// Parses the process's command line, or exits with clap's message if it isn't valid.
    pub(crate) fn parse() -> Self {
        Self::from_matches(&command().get_matches())
    }

    /// The command line `matches` holds, which `command` parsed.
    fn from_matches(matches: &ArgMatches) -> Self {
        Self {
            settings: matches
                .get_one::<PathBuf>("settings")
                .cloned()
                .expect("clap requires `--settings`"),
            port: matches
                .get_one::<u16>("port")
                .copied()
                .expect("`--port` has a default"),
            greeting: matches
                .get_one::<Greeting>("greeting")
                .copied()
                .expect("`--greeting` has a default"),
        }
    }
}

/// The command line's definition.
fn command() -> Command {
    Command::new(env!("CARGO_BIN_NAME"))
        .version(env!("CARGO_PKG_VERSION"))
        .about(env!("CARGO_PKG_DESCRIPTION"))
        .arg(
            Arg::new("settings")
                .long("settings")
                .help("The JSON settings file to read")
                .required(true)
                .value_parser(clap::value_parser!(PathBuf)),
        )
        .arg(
            Arg::new("port")
                .long("port")
                .help("The port to report")
                .default_value("8080")
                .value_parser(clap::value_parser!(u16).range(1..)),
        )
        .arg(
            Arg::new("greeting")
                .long("greeting")
                .help("How to greet")
                .default_value("plain")
                .value_parser(clap::value_parser!(Greeting)),
        )
}

#[cfg(test)]
mod tests {
    #[test]
    fn command_is_well_formed() {
        super::command().debug_assert();
    }
}
EOF

cat >$binary/src/greeting.rs <<'EOF'
//! How `demo-binary` greets.

use clap::ValueEnum;
use clap::builder::PossibleValue;

/// How `demo-binary` greets.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Greeting {
    /// A plain hello.
    Plain,
    /// A hello with a flourish.
    Fancy,
}

impl ValueEnum for Greeting {
    fn value_variants<'variants>() -> &'variants [Self] {
        &[Self::Plain, Self::Fancy]
    }

    fn to_possible_value(&self) -> Option<PossibleValue> {
        Some(match self {
            Self::Plain => PossibleValue::new("plain").help("A plain hello"),
            Self::Fancy => PossibleValue::new("fancy").help("A hello with a flourish"),
        })
    }
}
EOF
