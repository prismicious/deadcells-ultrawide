"""Dead Cells ultrawide launcher: start the game and widen its view so it fills the screen.

Dead Cells (HashLink bytecode inside deadcells.exe) sizes its play area in
Main.onResize as

    pixelScale   = floor4(max(1, 3 * min(W / 1920, H / 1080)))   # 0.25 steps
    stage width  = min(pixelScale * Viewport.NATIVE_WIDTH,  W)
    stage height = min(pixelScale * Viewport.NATIVE_HEIGHT, H)

and paints everything outside the stage black. NATIVE_WIDTH is 683, about 16:9,
so a 5120x1440 screen gets a 2732 px wide stage and 1194 px of black on each side.
The HUD, menus and parallax backgrounds all lay out against the stage size and
NATIVE_WIDTH. Raising that single static therefore widens the world view (Hor+)
with everything following it, and nothing is stretched.

The exe is protected by SteamStub DRM, so editing it on disk fails with Steam's
"Application load error 3:0000065432". This launcher leaves every game file
untouched. Instead it:

  1. starts the game through Steam, or runs the command Steam hands it when used as
     a Steam launch-option wrapper (`"...\\pythonw.exe" "...\\deadcells_ultrawide.py" %command%`);
  2. waits for the game to initialise the $Viewport statics, which show up in memory
     as the aligned int32 pair 683, 400 (NATIVE_WIDTH, NATIVE_HEIGHT);
  3. writes the new NATIVE_WIDTH into that one location, reads it back, and resizes
     the game window by a pixel and back so Main.onResize runs again.

Usage:
    python deadcells_ultrawide.py                    # launch via Steam, fit the primary monitor
    python deadcells_ultrawide.py --width 1280       # explicit logical width
    python deadcells_ultrawide.py --attach           # patch an already running game
    <launcher> %command%                             # as Steam launch options
"""

import argparse
import ctypes
import ctypes.wintypes as wt
import math
import os
import struct
import subprocess
import sys
import time

STEAM_APP_ID = 588650
GAME_EXES = ("deadcells.exe", "deadcells_gl.exe")
STOCK_NATIVE = struct.pack("<ii", 683, 400)
WINDOW_TITLE = "Dead Cells"
STARTUP_TIMEOUT_S = 120

PROCESS_ACCESS = 0x0438  # QUERY_INFORMATION | VM_OPERATION | VM_READ | VM_WRITE
MEM_COMMIT, PAGE_READWRITE = 0x1000, 0x04
TH32CS_SNAPPROCESS = 0x2
SWP_NOZORDER_NOACTIVATE = 0x0004 | 0x0010

kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
user32 = ctypes.WinDLL("user32", use_last_error=True)


class MemoryBasicInformation(ctypes.Structure):
    _fields_ = [("BaseAddress", ctypes.c_void_p), ("AllocationBase", ctypes.c_void_p),
                ("AllocationProtect", wt.DWORD), ("PartitionId", wt.WORD),
                ("RegionSize", ctypes.c_size_t), ("State", wt.DWORD),
                ("Protect", wt.DWORD), ("Type", wt.DWORD)]


class ProcessEntry32(ctypes.Structure):
    _fields_ = [("dwSize", wt.DWORD), ("cntUsage", wt.DWORD), ("th32ProcessID", wt.DWORD),
                ("th32DefaultHeapID", ctypes.c_size_t), ("th32ModuleID", wt.DWORD),
                ("cntThreads", wt.DWORD), ("th32ParentProcessID", wt.DWORD),
                ("pcPriClassBase", ctypes.c_long), ("dwFlags", wt.DWORD),
                ("szExeFile", ctypes.c_wchar * 260)]


kernel32.OpenProcess.restype = wt.HANDLE
kernel32.CreateToolhelp32Snapshot.restype = wt.HANDLE
kernel32.VirtualQueryEx.argtypes = [wt.HANDLE, ctypes.c_void_p,
                                    ctypes.POINTER(MemoryBasicInformation), ctypes.c_size_t]
kernel32.ReadProcessMemory.argtypes = [wt.HANDLE, ctypes.c_void_p, ctypes.c_void_p,
                                       ctypes.c_size_t, ctypes.POINTER(ctypes.c_size_t)]
kernel32.WriteProcessMemory.argtypes = kernel32.ReadProcessMemory.argtypes
WNDENUMPROC = ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)


def set_dpi_aware():
    try:
        user32.SetProcessDpiAwarenessContext(ctypes.c_void_p(-4))
    except (AttributeError, OSError):
        user32.SetProcessDPIAware()


def logical_width_for_primary_monitor():
    """Logical width that fills the primary monitor at the game's own pixelScale."""
    w, h = user32.GetSystemMetrics(0), user32.GetSystemMetrics(1)
    scale = math.floor(max(1.0, 3 * min(w / 1920, h / 1080)) * 4) / 4
    return w, h, math.ceil(w / scale)


def find_game_pid():
    snap = kernel32.CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)
    entry = ProcessEntry32(dwSize=ctypes.sizeof(ProcessEntry32))
    try:
        ok = kernel32.Process32FirstW(snap, ctypes.byref(entry))
        while ok:
            if entry.szExeFile.lower() in GAME_EXES:
                return entry.th32ProcessID
            ok = kernel32.Process32NextW(snap, ctypes.byref(entry))
    finally:
        kernel32.CloseHandle(snap)
    return None


def game_window(pid):
    found = []

    def visit(hwnd, _):
        owner = wt.DWORD()
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
        if owner.value == pid and user32.IsWindowVisible(hwnd):
            title = ctypes.create_unicode_buffer(256)
            user32.GetWindowTextW(hwnd, title, 256)
            if title.value == WINDOW_TITLE:
                found.append(hwnd)
        return True

    user32.EnumWindows(WNDENUMPROC(visit), 0)
    return found[0] if found else None


def find_native_size_statics(process):
    """Addresses of 8-byte-aligned (683, 400) int pairs in the game's writable memory."""
    hits, address, info = [], 0, MemoryBasicInformation()
    while kernel32.VirtualQueryEx(process, ctypes.c_void_p(address), ctypes.byref(info),
                                  ctypes.sizeof(info)):
        base, size = info.BaseAddress or 0, info.RegionSize
        if info.State == MEM_COMMIT and info.Protect == PAGE_READWRITE and size < 1 << 31:
            buf, got = ctypes.create_string_buffer(size), ctypes.c_size_t()
            if kernel32.ReadProcessMemory(process, ctypes.c_void_p(base), buf, size, ctypes.byref(got)):
                data, i = buf.raw[:got.value], -1
                while (i := data.find(STOCK_NATIVE, i + 1)) != -1:
                    if (base + i) % 8 == 0:
                        hits.append(base + i)
        address = base + size
    return hits


def read_int(process, address):
    buf, got = ctypes.create_string_buffer(4), ctypes.c_size_t()
    kernel32.ReadProcessMemory(process, ctypes.c_void_p(address), buf, 4, ctypes.byref(got))
    return struct.unpack("<i", buf.raw)[0]


def write_int(process, address, value):
    wrote = ctypes.c_size_t()
    ok = kernel32.WriteProcessMemory(process, ctypes.c_void_p(address), struct.pack("<i", value),
                                     4, ctypes.byref(wrote))
    return bool(ok) and wrote.value == 4


def nudge_resize(hwnd):
    """Resize the window by one pixel and back, so the game re-runs Main.onResize."""
    r = wt.RECT()
    user32.GetWindowRect(hwnd, ctypes.byref(r))
    w, h = r.right - r.left, r.bottom - r.top
    user32.SetWindowPos(hwnd, None, r.left, r.top, w - 1, h, SWP_NOZORDER_NOACTIVATE)
    time.sleep(0.5)
    user32.SetWindowPos(hwnd, None, r.left, r.top, w, h, SWP_NOZORDER_NOACTIVATE)


def wait_for(probe, what, timeout=STARTUP_TIMEOUT_S):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        value = probe()
        if value:
            return value
        time.sleep(1)
    raise SystemExit(f"timed out after {timeout}s waiting for {what}")


def patch_running_game(width):
    pid = wait_for(find_game_pid, "the Dead Cells process")
    process = kernel32.OpenProcess(PROCESS_ACCESS, False, pid)
    if not process:
        raise SystemExit(f"cannot open process {pid} (error {ctypes.get_last_error()})")
    try:
        hwnd = wait_for(lambda: game_window(pid), "the Dead Cells window")
        hits = wait_for(lambda: find_native_size_statics(process), "the game to initialise")
        if len(hits) != 1:
            raise SystemExit(f"expected one NATIVE_WIDTH/NATIVE_HEIGHT pair, found {len(hits)}; "
                             "not patching (unsupported game version?)")
        address = hits[0]
        if not write_int(process, address, width) or read_int(process, address) != width:
            raise SystemExit(f"writing NATIVE_WIDTH at {address:#x} failed")
        nudge_resize(hwnd)
        print(f"pid {pid}: NATIVE_WIDTH 683 -> {width} at {address:#x}")
    finally:
        kernel32.CloseHandle(process)
    return pid


def launch(command):
    """Start the game: the wrapped Steam command if given, else through Steam itself."""
    if command:
        return subprocess.Popen(command)
    os.startfile(f"steam://rungameid/{STEAM_APP_ID}")
    return None


def log_to_file_without_console():
    """Under pythonw there is no console; keep messages in a log next to this script."""
    if sys.stdout is None:
        log = open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "deadcells_ultrawide.log"),
                   "a", encoding="utf-8", buffering=1)
        sys.stdout = sys.stderr = log
        print(time.strftime("--- %Y-%m-%d %H:%M:%S"))


def main():
    log_to_file_without_console()
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--width", type=int, help="logical NATIVE_WIDTH (default: fit primary monitor)")
    ap.add_argument("--attach", action="store_true", help="patch an already running game")
    ap.add_argument("command", nargs=argparse.REMAINDER, help="game command from Steam's %%command%%")
    args = ap.parse_args()

    set_dpi_aware()
    width = args.width
    if width is None:
        w, h, width = logical_width_for_primary_monitor()
        print(f"primary monitor {w}x{h} -> logical width {width}")
    if width <= 683:
        print("screen is 16:9 or narrower; the stock game already fills it")
        width = None

    if not args.attach and find_game_pid():
        raise SystemExit("Dead Cells is already running; close it or use --attach")
    child = None if args.attach else launch(args.command)
    if width:
        patch_running_game(width)
    if child:
        child.wait()  # Steam tracks the wrapper, so stay alive while the game runs


if __name__ == "__main__":
    sys.exit(main())
