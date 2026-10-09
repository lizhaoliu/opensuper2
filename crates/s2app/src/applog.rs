//! Diagnostics for a windowed program: messages go to stderr (when there is
//! one) and to `opensuper2.log` in the game directory; fatal errors are also
//! shown in a message box, since release builds on Windows have no console.

use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;

static LOG: Mutex<Option<File>> = Mutex::new(None);

/// Start a fresh log file for this run (best effort).
pub fn init(game_dir: &Path) {
    if let Ok(f) = File::create(game_dir.join("opensuper2.log")) {
        *LOG.lock().unwrap() = Some(f);
    }
    std::panic::set_hook(Box::new(|info| {
        let msg = format!("{info}");
        line(&format!("panic: {msg}"));
        fatal_box(&format!("OpenSuper2 crashed:\n\n{msg}"));
    }));
}

pub fn line(msg: &str) {
    eprintln!("{msg}");
    if let Some(f) = LOG.lock().unwrap().as_mut() {
        let _ = writeln!(f, "{msg}");
        let _ = f.flush();
    }
}

#[macro_export]
macro_rules! log {
    ($($arg:tt)*) => { $crate::applog::line(&format!($($arg)*)) };
}

#[cfg(windows)]
pub fn fatal_box(msg: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
    let wide = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let (text, title) = (wide(msg), wide("OpenSuper2"));
    // SAFETY: both buffers are NUL-terminated UTF-16 and outlive the call.
    unsafe { MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), MB_OK | MB_ICONERROR) };
}

#[cfg(not(windows))]
pub fn fatal_box(_msg: &str) {}
