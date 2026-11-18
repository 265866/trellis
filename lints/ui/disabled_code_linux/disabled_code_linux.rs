// The test's `dylint.toml` sets `ci_targets = ["linux"]`.

// Not flagged: Linux is built, and `unix` holds on it.
#[cfg(target_os = "linux")]
fn on_linux() {}
#[cfg(unix)]
fn on_unix() {}
#[cfg(target_family = "unix")]
fn on_unix_family() {}

// Flagged: Windows and macOS aren't built, so no check compiles this code.
#[cfg(windows)]
fn on_windows() {}
#[cfg(target_os = "macos")]
fn on_macos() {}
#[cfg(target_family = "windows")]
fn on_windows_family() {}
#[cfg_attr(target_os = "macos", must_use)]
fn marked_on_macos() -> u8 {
    1
}

// Flagged: naming an unbuilt system, even to leave it out.
#[cfg(not(windows))]
fn not_on_windows() {}

// Flagged: holds on no built system.
#[cfg(not(target_os = "linux"))]
fn off_linux() {}
#[cfg(all(unix, not(target_os = "linux")))]
fn other_unix() {}

// Flagged: an arm on Windows, which isn't built, and a `_` arm after `unix`, which no built system
// selects.
cfg_select! {
    windows => {
        fn shell() -> &'static str { "cmd" }
    }
    _ => {
        fn shell() -> &'static str { "sh" }
    }
}

cfg_select! {
    unix => {
        fn home() -> &'static str { "HOME" }
    }
    _ => {
        fn home() -> &'static str { "USERPROFILE" }
    }
}

// Not flagged: one arm for the built system, which is every build.
cfg_select! {
    target_os = "linux" => {
        fn config_dir() -> &'static str { ".config" }
    }
}

fn main() {
    let _ = (shell(), home(), config_dir());
    if cfg!(target_os = "macos") {
        on_linux();
    }
}
