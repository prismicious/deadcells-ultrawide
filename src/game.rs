//! Finding the running game, its window, and the Viewport statics in its memory.

use crate::win32::{self, Handle, Hwnd};
use std::ffi::c_void;
use std::thread::sleep;
use std::time::{Duration, Instant};

const GAME_EXES: [&str; 2] = ["deadcells.exe", "deadcells_gl.exe"];
const WINDOW_TITLE: &str = "Dead Cells";
pub const STOCK_WIDTH: i32 = 683;
const STOCK_HEIGHT: i32 = 400;
const STARTUP_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_REGION: usize = 1 << 31;

/// An open handle to the game process, closed on drop.
pub struct Process {
    pub pid: u32,
    handle: Handle,
}

impl Process {
    pub fn open(pid: u32) -> Result<Self, String> {
        let handle = unsafe { win32::OpenProcess(win32::PROCESS_ACCESS, 0, pid) };
        if handle.is_null() {
            return Err(format!(
                "cannot open process {pid} (error {})",
                last_error()
            ));
        }
        Ok(Self { pid, handle })
    }

    /// Read exactly `buf.len()` bytes at `address`.
    fn read(&self, address: usize, buf: &mut [u8]) -> Result<(), String> {
        let mut got = 0usize;
        let ok = unsafe {
            win32::ReadProcessMemory(
                self.handle,
                address as *const c_void,
                buf.as_mut_ptr().cast(),
                buf.len(),
                &mut got,
            )
        };
        if ok == 0 || got != buf.len() {
            return Err(format!(
                "reading {} bytes at {address:#x} failed (got {got}, error {})",
                buf.len(),
                last_error()
            ));
        }
        Ok(())
    }

    pub fn read_i32(&self, address: usize) -> Result<i32, String> {
        let mut buf = [0u8; 4];
        self.read(address, &mut buf)?;
        Ok(i32::from_le_bytes(buf))
    }

    pub fn write_i32(&self, address: usize, value: i32) -> Result<(), String> {
        let bytes = value.to_le_bytes();
        let mut wrote = 0usize;
        let ok = unsafe {
            win32::WriteProcessMemory(
                self.handle,
                address as *mut c_void,
                bytes.as_ptr().cast(),
                bytes.len(),
                &mut wrote,
            )
        };
        if ok == 0 || wrote != bytes.len() {
            return Err(format!(
                "writing at {address:#x} failed (error {})",
                last_error()
            ));
        }
        Ok(())
    }

    /// Addresses of 8-byte-aligned (683, 400) int pairs in committed read-write memory.
    pub fn find_native_size_statics(&self) -> Vec<usize> {
        let mut pattern = [0u8; 8];
        pattern[..4].copy_from_slice(&STOCK_WIDTH.to_le_bytes());
        pattern[4..].copy_from_slice(&STOCK_HEIGHT.to_le_bytes());

        let mut hits = Vec::new();
        let mut address = 0usize;
        let mut info: win32::MemoryBasicInformation = unsafe { std::mem::zeroed() };
        let info_size = std::mem::size_of::<win32::MemoryBasicInformation>();
        while unsafe {
            win32::VirtualQueryEx(self.handle, address as *const c_void, &mut info, info_size)
        } != 0
        {
            let (base, size) = (info.base_address as usize, info.region_size);
            if info.state == win32::MEM_COMMIT
                && info.protect == win32::PAGE_READWRITE
                && size < MAX_REGION
            {
                let mut data = vec![0u8; size];
                // Regions can change or vanish between query and read; skip those.
                if self.read(base, &mut data).is_ok() {
                    // Regions are page-aligned, so 8-byte steps keep the address aligned.
                    hits.extend(
                        data.as_chunks::<8>()
                            .0
                            .iter()
                            .enumerate()
                            .filter(|(_, chunk)| **chunk == pattern)
                            .map(|(i, _)| base + i * 8),
                    );
                }
            }
            match base.checked_add(size) {
                Some(next) if next > address => address = next,
                _ => break,
            }
        }
        hits
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        unsafe { win32::CloseHandle(self.handle) };
    }
}

/// Process id of a running Dead Cells, if any.
pub fn find_pid() -> Option<u32> {
    let snapshot = unsafe { win32::CreateToolhelp32Snapshot(win32::TH32CS_SNAPPROCESS, 0) };
    if snapshot == win32::INVALID_HANDLE_VALUE {
        return None;
    }
    let mut entry: win32::ProcessEntry32W = unsafe { std::mem::zeroed() };
    entry.size = std::mem::size_of::<win32::ProcessEntry32W>() as u32;
    let mut found = None;
    let mut ok = unsafe { win32::Process32FirstW(snapshot, &mut entry) };
    while ok != 0 {
        let name = win32::from_wide(&entry.exe_file).to_lowercase();
        if GAME_EXES.contains(&name.as_str()) {
            found = Some(entry.process_id);
            break;
        }
        ok = unsafe { win32::Process32NextW(snapshot, &mut entry) };
    }
    unsafe { win32::CloseHandle(snapshot) };
    found
}

struct WindowSearch {
    pid: u32,
    found: Option<Hwnd>,
}

unsafe extern "system" fn visit_window(hwnd: Hwnd, param: isize) -> i32 {
    let search = &mut *(param as *mut WindowSearch);
    let mut owner = 0u32;
    win32::GetWindowThreadProcessId(hwnd, &mut owner);
    if owner == search.pid && win32::IsWindowVisible(hwnd) != 0 {
        let mut title = [0u16; 256];
        win32::GetWindowTextW(hwnd, title.as_mut_ptr(), title.len() as i32);
        if win32::from_wide(&title) == WINDOW_TITLE {
            search.found = Some(hwnd);
            return 0;
        }
    }
    1
}

/// The game's visible main window; the process also owns invisible helper windows.
pub fn find_window(pid: u32) -> Option<Hwnd> {
    let mut search = WindowSearch { pid, found: None };
    unsafe { win32::EnumWindows(visit_window, &mut search as *mut WindowSearch as isize) };
    search.found
}

/// Resize the window by one pixel and back, so the game re-runs Main.onResize.
pub fn nudge_resize(hwnd: Hwnd) {
    let mut r = win32::Rect::default();
    unsafe { win32::GetWindowRect(hwnd, &mut r) };
    let (w, h) = (r.right - r.left, r.bottom - r.top);
    let flags = win32::SWP_NOZORDER_NOACTIVATE;
    unsafe { win32::SetWindowPos(hwnd, std::ptr::null_mut(), r.left, r.top, w - 1, h, flags) };
    sleep(Duration::from_millis(500));
    unsafe { win32::SetWindowPos(hwnd, std::ptr::null_mut(), r.left, r.top, w, h, flags) };
}

/// Poll `probe` once a second until it yields a value or the startup timeout passes.
pub fn wait_for<T>(what: &str, mut probe: impl FnMut() -> Option<T>) -> Result<T, String> {
    let deadline = Instant::now() + STARTUP_TIMEOUT;
    loop {
        if let Some(value) = probe() {
            return Ok(value);
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "timed out after {}s waiting for {what}",
                STARTUP_TIMEOUT.as_secs()
            ));
        }
        sleep(Duration::from_secs(1));
    }
}

/// Wait for the game, then set NATIVE_WIDTH to `width` and make it re-lay-out.
pub fn patch(width: i32) -> Result<(), String> {
    let pid = wait_for("the Dead Cells process", find_pid)?;
    let process = Process::open(pid)?;
    let hwnd = wait_for("the Dead Cells window", || find_window(pid))?;
    let hits = wait_for("the game to initialise", || {
        Some(process.find_native_size_statics()).filter(|h| !h.is_empty())
    })?;
    let [address] = hits[..] else {
        return Err(format!(
            "expected one NATIVE_WIDTH/NATIVE_HEIGHT pair, found {}; not patching (unsupported game version?)",
            hits.len()
        ));
    };
    process.write_i32(address, width)?;
    let now = process.read_i32(address)?;
    if now != width {
        return Err(format!(
            "NATIVE_WIDTH at {address:#x} reads {now} after writing {width}"
        ));
    }
    nudge_resize(hwnd);
    crate::output::say(&format!(
        "pid {}: NATIVE_WIDTH {STOCK_WIDTH} -> {width} at {address:#x}",
        process.pid
    ));
    Ok(())
}

fn last_error() -> u32 {
    unsafe { win32::GetLastError() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn own_process() -> Process {
        Process::open(std::process::id()).expect("open own process")
    }

    #[test]
    fn failed_read_is_an_error_not_zero() {
        assert!(own_process().read_i32(0).is_err());
    }

    #[test]
    fn finds_and_rewrites_an_aligned_native_size_pair() {
        let statics: Box<[i32; 2]> = Box::new([STOCK_WIDTH, STOCK_HEIGHT]);
        let address = statics.as_ptr() as usize;
        let process = own_process();
        assert!(process.find_native_size_statics().contains(&address));
        process.write_i32(address, 1280).unwrap();
        assert_eq!(process.read_i32(address).unwrap(), 1280);
        assert_eq!(std::hint::black_box(&statics)[0], 1280);
    }
}
