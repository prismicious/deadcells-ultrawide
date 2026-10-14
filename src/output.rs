//! Where messages go. The exe is a GUI-subsystem program, so starting it from a
//! shortcut or from Steam opens no console window. Run from a terminal, it
//! attaches to that console; otherwise it appends to a log next to the exe.

use crate::win32;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::{Mutex, OnceLock};

const LOG_NAME: &str = "deadcells-ultrawide.log";

static SINK: OnceLock<Mutex<Option<File>>> = OnceLock::new();

/// Pick the console or the log file. Call once, before the first `say`.
pub fn init() {
    let attached = unsafe { win32::AttachConsole(win32::ATTACH_PARENT_PROCESS) } != 0;
    let file = if attached {
        OpenOptions::new()
            .write(true)
            .open("CONOUT$")
            .ok()
            .map(|mut console| {
                let _ = console.write_all(b"\r\n");
                console
            })
    } else {
        open_log()
    };
    let _ = SINK.set(Mutex::new(file));
    if !attached {
        say(&format!("--- {}", timestamp()));
    }
}

/// Print one line to the chosen sink. Messages are best-effort: a missing
/// console or an unwritable log never stops the launcher.
pub fn say(line: &str) {
    if let Some(Ok(mut sink)) = SINK.get().map(|m| m.lock()) {
        if let Some(out) = sink.as_mut() {
            let _ = out.write_all(format!("{line}\r\n").as_bytes());
        }
    }
}

fn open_log() -> Option<File> {
    let path = std::env::current_exe().ok()?.with_file_name(LOG_NAME);
    OpenOptions::new().create(true).append(true).open(path).ok()
}

fn timestamp() -> String {
    let mut t = win32::SystemTime::default();
    unsafe { win32::GetLocalTime(&mut t) };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        t.year, t.month, t.day, t.hour, t.minute, t.second
    )
}
