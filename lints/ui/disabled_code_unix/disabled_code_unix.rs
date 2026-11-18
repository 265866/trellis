// The test's `dylint.toml` sets `ci_targets = ["linux", "macos"]`.

// Not flagged: both built systems are Unix.
#[cfg(unix)]
fn on_unix() {}
#[cfg(target_os = "macos")]
fn on_macos() {}
#[cfg(not(target_os = "macos"))]
fn off_macos() {}

// Flagged: holds on no built system.
#[cfg(not(unix))]
fn off_unix() {}

// Flagged: Windows isn't built.
#[cfg(target_os = "windows")]
fn on_windows() {}

fn main() {}
