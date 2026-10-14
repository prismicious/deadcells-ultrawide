//! The handful of Win32 calls the launcher needs, declared by hand so the build
//! has no dependencies to download or audit.

use std::ffi::c_void;

pub type Handle = *mut c_void;
pub type Hwnd = *mut c_void;

pub const INVALID_HANDLE_VALUE: Handle = -1isize as Handle;
pub const PROCESS_ACCESS: u32 = 0x0438; // QUERY_INFORMATION | VM_OPERATION | VM_READ | VM_WRITE
pub const MEM_COMMIT: u32 = 0x1000;
pub const PAGE_READWRITE: u32 = 0x04;
pub const TH32CS_SNAPPROCESS: u32 = 0x2;
pub const SWP_NOZORDER_NOACTIVATE: u32 = 0x0004 | 0x0010;
pub const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
pub const DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2: isize = -4;
pub const SM_CXSCREEN: i32 = 0;
pub const SM_CYSCREEN: i32 = 1;
pub const SW_SHOWNORMAL: i32 = 1;

#[repr(C)]
pub struct MemoryBasicInformation {
    pub base_address: *mut c_void,
    pub allocation_base: *mut c_void,
    pub allocation_protect: u32,
    pub partition_id: u16,
    pub region_size: usize,
    pub state: u32,
    pub protect: u32,
    pub kind: u32,
}

#[repr(C)]
pub struct ProcessEntry32W {
    pub size: u32,
    pub usage: u32,
    pub process_id: u32,
    pub default_heap_id: usize,
    pub module_id: u32,
    pub threads: u32,
    pub parent_process_id: u32,
    pub priority_class_base: i32,
    pub flags: u32,
    pub exe_file: [u16; 260],
}

#[repr(C)]
#[derive(Default)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[repr(C)]
#[derive(Default)]
pub struct SystemTime {
    pub year: u16,
    pub month: u16,
    pub day_of_week: u16,
    pub day: u16,
    pub hour: u16,
    pub minute: u16,
    pub second: u16,
    pub milliseconds: u16,
}

pub type WndEnumProc = unsafe extern "system" fn(Hwnd, isize) -> i32;

#[link(name = "kernel32")]
extern "system" {
    pub fn OpenProcess(access: u32, inherit: i32, pid: u32) -> Handle;
    pub fn CloseHandle(handle: Handle) -> i32;
    pub fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> Handle;
    pub fn Process32FirstW(snapshot: Handle, entry: *mut ProcessEntry32W) -> i32;
    pub fn Process32NextW(snapshot: Handle, entry: *mut ProcessEntry32W) -> i32;
    pub fn VirtualQueryEx(
        process: Handle,
        address: *const c_void,
        info: *mut MemoryBasicInformation,
        length: usize,
    ) -> usize;
    pub fn ReadProcessMemory(
        process: Handle,
        address: *const c_void,
        buffer: *mut c_void,
        size: usize,
        read: *mut usize,
    ) -> i32;
    pub fn WriteProcessMemory(
        process: Handle,
        address: *mut c_void,
        buffer: *const c_void,
        size: usize,
        written: *mut usize,
    ) -> i32;
    pub fn GetLastError() -> u32;
    pub fn AttachConsole(pid: u32) -> i32;
    pub fn GetLocalTime(time: *mut SystemTime);
}

#[link(name = "user32")]
extern "system" {
    pub fn SetProcessDpiAwarenessContext(context: isize) -> i32;
    pub fn GetSystemMetrics(index: i32) -> i32;
    pub fn EnumWindows(callback: WndEnumProc, param: isize) -> i32;
    pub fn GetWindowThreadProcessId(hwnd: Hwnd, pid: *mut u32) -> u32;
    pub fn IsWindowVisible(hwnd: Hwnd) -> i32;
    pub fn GetWindowTextW(hwnd: Hwnd, text: *mut u16, max: i32) -> i32;
    pub fn GetWindowRect(hwnd: Hwnd, rect: *mut Rect) -> i32;
    pub fn SetWindowPos(
        hwnd: Hwnd,
        after: Hwnd,
        x: i32,
        y: i32,
        cx: i32,
        cy: i32,
        flags: u32,
    ) -> i32;
}

#[link(name = "shell32")]
extern "system" {
    pub fn ShellExecuteW(
        hwnd: Hwnd,
        operation: *const u16,
        file: *const u16,
        parameters: *const u16,
        directory: *const u16,
        show: i32,
    ) -> isize;
}

/// NUL-terminated UTF-16 copy of `s` for passing to a `W` function.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// The text of a NUL-terminated UTF-16 buffer.
pub fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}
