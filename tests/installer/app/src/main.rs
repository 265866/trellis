//! Holds its own file open until something closes it: with `window`, a top-level window that ends
//! the program when it is closed; with `hidden-window`, a top-level window that is never shown and
//! ends the program on `WM_ENDSESSION`; and with `hidden`, no window at all. `HELPER_BUILD`, set at
//! build time, goes in the window's title, so two builds differ.
#![windows_subsystem = "windows"]

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::{ptr, thread, time};

/// Whether the window ends the program on `WM_ENDSESSION`, as only the hidden one does; the shown
/// one waits for `WM_CLOSE`, as a program that handles neither does.
static ENDS_WITH_SESSION: AtomicBool = AtomicBool::new(false);

/// A window handle.
type Hwnd = *mut c_void;

/// The text that tells two builds apart.
const BUILD: &str = match option_env!("HELPER_BUILD") {
    Some(build) => build,
    None => "dev",
};

/// `WNDCLASSW`.
#[repr(C)]
struct WndClass {
    /// The class style.
    style: u32,
    /// The window procedure.
    procedure: unsafe extern "system" fn(Hwnd, u32, usize, isize) -> isize,
    /// Extra class bytes.
    class_extra: i32,
    /// Extra window bytes.
    window_extra: i32,
    /// The module.
    instance: *mut c_void,
    /// The icon.
    icon: *mut c_void,
    /// The cursor.
    cursor: *mut c_void,
    /// The background brush.
    background: *mut c_void,
    /// The menu's resource name.
    menu_name: *const u16,
    /// The class name.
    class_name: *const u16,
}

/// `MSG`.
#[repr(C)]
struct Msg {
    /// The window.
    window: Hwnd,
    /// The message.
    message: u32,
    /// The first parameter.
    wparam: usize,
    /// The second parameter.
    lparam: isize,
    /// When it was posted.
    time: u32,
    /// The cursor's position then.
    point: [i32; 2],
    /// Room for the fields newer Windows adds.
    private: [u32; 4],
}

#[link(name = "user32")]
unsafe extern "system" {
    fn RegisterClassW(class: *const WndClass) -> u16;
    fn CreateWindowExW(
        ex_style: u32,
        class_name: *const u16,
        title: *const u16,
        style: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        parent: Hwnd,
        menu: *mut c_void,
        instance: *mut c_void,
        parameter: *mut c_void,
    ) -> Hwnd;
    fn GetMessageW(message: *mut Msg, window: Hwnd, first: u32, last: u32) -> i32;
    fn TranslateMessage(message: *const Msg) -> i32;
    fn DispatchMessageW(message: *const Msg) -> isize;
    fn DefWindowProcW(window: Hwnd, message: u32, wparam: usize, lparam: isize) -> isize;
    fn PostQuitMessage(code: i32);
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleW(name: *const u16) -> *mut c_void;
}

/// `WM_DESTROY`.
const WM_DESTROY: u32 = 0x0002;

/// `WM_ENDSESSION`.
const WM_ENDSESSION: u32 = 0x0016;

/// `WS_OVERLAPPEDWINDOW`.
const OVERLAPPED_WINDOW: u32 = 0x00cf_0000;

/// `WS_VISIBLE`.
const VISIBLE: u32 = 0x1000_0000;

/// `CW_USEDEFAULT`.
const DEFAULT_POSITION: i32 = i32::MIN;

/// `text` as a null-terminated UTF-16 string.
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

/// Ends the message loop when the window is destroyed, which closing it does, and, for the hidden
/// window, ends the program when Windows says its session ends, with a nonzero `wparam`, as
/// Restart Manager does.
unsafe extern "system" fn procedure(
    window: Hwnd,
    message: u32,
    wparam: usize,
    lparam: isize,
) -> isize {
    if message == WM_ENDSESSION && wparam != 0 && ENDS_WITH_SESSION.load(Ordering::Relaxed) {
        std::process::exit(0);
    }
    if message == WM_DESTROY {
        // SAFETY: PostQuitMessage takes any exit code.
        unsafe { PostQuitMessage(0) };
        return 0;
    }
    // SAFETY: the arguments are the ones Windows passed to the window procedure.
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

/// Creates a top-level window, shown when `visible`, and runs its message loop until the program
/// ends.
fn window(visible: bool) {
    ENDS_WITH_SESSION.store(!visible, Ordering::Relaxed);
    let class_name = wide("InstallerHelper");
    let title = wide(&format!("installer helper {BUILD}"));
    // SAFETY: a null name asks for the running program's module.
    let instance = unsafe { GetModuleHandleW(ptr::null()) };
    let class = WndClass {
        style: 0,
        procedure,
        class_extra: 0,
        window_extra: 0,
        instance,
        icon: ptr::null_mut(),
        cursor: ptr::null_mut(),
        background: ptr::null_mut(),
        menu_name: ptr::null(),
        class_name: class_name.as_ptr(),
    };
    // SAFETY: `class` and the strings it points to outlive the window.
    if unsafe { RegisterClassW(&class) } == 0 {
        std::process::exit(2);
    }
    // SAFETY: the class is registered and the strings are null-terminated.
    let handle = unsafe {
        CreateWindowExW(
            0,
            class_name.as_ptr(),
            title.as_ptr(),
            if visible {
                OVERLAPPED_WINDOW | VISIBLE
            } else {
                OVERLAPPED_WINDOW
            },
            DEFAULT_POSITION,
            DEFAULT_POSITION,
            400,
            200,
            ptr::null_mut(),
            ptr::null_mut(),
            instance,
            ptr::null_mut(),
        )
    };
    if handle.is_null() {
        std::process::exit(3);
    }
    let mut message = Msg {
        window: ptr::null_mut(),
        message: 0,
        wparam: 0,
        lparam: 0,
        time: 0,
        point: [0; 2],
        private: [0; 4],
    };
    // SAFETY: `message` is a valid MSG for the loop to fill.
    while unsafe { GetMessageW(&mut message, ptr::null_mut(), 0, 0) } > 0 {
        // SAFETY: `message` holds the message GetMessageW returned.
        unsafe {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("window") => window(true),
        Some("hidden-window") => window(false),
        Some("hidden") => loop {
            thread::sleep(time::Duration::from_secs(1));
        },
        _ => std::process::exit(1),
    }
}
