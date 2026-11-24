"""Native Windows desktop automation helpers for installed binding scenarios."""

from __future__ import annotations

import ctypes
import json
import os
import struct
import subprocess
import time
from pathlib import Path


def _capture_windows_accessibility(hwnd: int, title: str) -> dict:
    if os.name != "nt":
        raise RuntimeError("Windows UI Automation is available only on Windows")
    script = r"""
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
$condition = [System.Windows.Automation.PropertyCondition]::new(
    [System.Windows.Automation.AutomationElement]::NameProperty,
    $env:GRAFIKR_UIA_TITLE
)
$window = [System.Windows.Automation.AutomationElement]::RootElement.FindFirst(
    [System.Windows.Automation.TreeScope]::Children,
    $condition
)
if ($null -eq $window) { throw 'Grafikr window was not found in UI Automation tree' }
$items = $window.FindAll(
    [System.Windows.Automation.TreeScope]::Descendants,
    [System.Windows.Automation.Condition]::TrueCondition
)
$controls = @(
    foreach ($item in $items) {
        $current = $item.Current
        $value = $null
        try {
            $candidate = $item.GetCurrentPropertyValue([System.Windows.Automation.ValuePattern]::ValueProperty)
            if ($candidate -is [string]) { $value = $candidate }
        } catch { }
        [pscustomobject]@{
            name = $current.Name
            control_type = $current.ControlType.ProgrammaticName
            keyboard_focusable = $current.IsKeyboardFocusable
            value = $value
        }
    }
)
[pscustomobject]@{
    window_name = $window.Current.Name
    native_window_handle = $window.Current.NativeWindowHandle
    controls = $controls
} | ConvertTo-Json -Depth 6 -Compress
"""
    environment = os.environ.copy()
    environment["GRAFIKR_UIA_TITLE"] = title
    environment["GRAFIKR_UIA_HWND"] = str(hwnd)
    completed = subprocess.run(
        ["powershell.exe", "-NoProfile", "-Sta", "-NonInteractive", "-Command", script],
        env=environment,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        check=False,
        timeout=15,
        creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0x08000000),
    )
    if completed.returncode != 0:
        raise RuntimeError(f"Windows UI Automation query failed: {completed.stderr[-2000:]}")
    try:
        report = json.loads(completed.stdout)
    except json.JSONDecodeError as error:
        raise RuntimeError("Windows UI Automation returned invalid JSON") from error
    if not isinstance(report, dict) or not isinstance(report.get("controls"), list):
        raise RuntimeError("Windows UI Automation returned an invalid control tree")
    if report.get("native_window_handle") != hwnd:
        raise RuntimeError("Windows UI Automation returned a different window handle")
    return report


def _wait_for_windows_window(process: subprocess.Popen, title: str, timeout: float) -> int:
    if os.name != "nt":
        raise RuntimeError("Windows window automation is available only on Windows")
    user32 = ctypes.WinDLL("user32", use_last_error=True)
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        found: list[int] = []

        @ctypes.WINFUNCTYPE(ctypes.c_bool, ctypes.c_void_p, ctypes.c_void_p)
        def visit(hwnd: int, _context: int) -> bool:
            native_handle = ctypes.c_void_p(hwnd)
            if not user32.IsWindowVisible(native_handle):
                return True
            length = user32.GetWindowTextLengthW(native_handle)
            if length <= 0:
                return True
            buffer = ctypes.create_unicode_buffer(length + 1)
            user32.GetWindowTextW(native_handle, buffer, len(buffer))
            if title in buffer.value:
                found.append(hwnd)
                return False
            return True

        user32.EnumWindows(visit, 0)
        if found:
            hwnd = found[0]
            native_handle = ctypes.c_void_p(hwnd)
            user32.ShowWindow(ctypes.c_void_p(hwnd), 5)  # SW_SHOW
            process_id = ctypes.c_ulong()
            kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
            user32.GetWindowThreadProcessId.argtypes = [ctypes.c_void_p, ctypes.POINTER(ctypes.c_ulong)]
            user32.GetWindowThreadProcessId.restype = ctypes.c_ulong
            kernel32.GetCurrentThreadId.restype = ctypes.c_ulong
            user32.AttachThreadInput.argtypes = [ctypes.c_ulong, ctypes.c_ulong, ctypes.c_bool]
            user32.AttachThreadInput.restype = ctypes.c_bool
            user32.GetForegroundWindow.restype = ctypes.c_void_p
            user32.SetWindowPos.argtypes = [
                ctypes.c_void_p, ctypes.c_void_p, ctypes.c_int, ctypes.c_int,
                ctypes.c_int, ctypes.c_int, ctypes.c_uint,
            ]
            user32.SetWindowPos.restype = ctypes.c_bool
            target_thread = user32.GetWindowThreadProcessId(native_handle, ctypes.byref(process_id))
            current_thread = kernel32.GetCurrentThreadId()
            attached = user32.AttachThreadInput(current_thread, target_thread, True)
            try:
                # The scenario window is test-owned. Keep it above the test console so
                # native mouse input is delivered to the visible app even when Windows
                # denies this background process a foreground activation.
                if not user32.SetWindowPos(native_handle, ctypes.c_void_p(-1), 0, 0, 0, 0, 0x0043):
                    raise ctypes.WinError(ctypes.get_last_error())
                user32.BringWindowToTop(native_handle)
                user32.SetActiveWindow(native_handle)
                user32.SetForegroundWindow(native_handle)
            finally:
                if attached:
                    user32.AttachThreadInput(current_thread, target_thread, False)
            time.sleep(0.2)
            return hwnd
        if process.poll() is not None:
            break
        time.sleep(0.1)
    raise RuntimeError(f"could not find native Windows window titled {title!r}")


def _windows_client_point(hwnd: int, x: int, y: int) -> tuple[int, int]:
    class Point(ctypes.Structure):
        _fields_ = [("x", ctypes.c_long), ("y", ctypes.c_long)]

    user32 = ctypes.WinDLL("user32", use_last_error=True)
    point = Point(x, y)
    if not user32.ClientToScreen(ctypes.c_void_p(hwnd), ctypes.byref(point)):
        raise ctypes.WinError(ctypes.get_last_error())
    return point.x, point.y


def _capture_windows_window(hwnd: int, destination: Path) -> None:
    if os.name != "nt":
        raise RuntimeError("Windows screenshots are available only on Windows")

    class Rect(ctypes.Structure):
        _fields_ = [("left", ctypes.c_long), ("top", ctypes.c_long), ("right", ctypes.c_long), ("bottom", ctypes.c_long)]

    class BitmapInfoHeader(ctypes.Structure):
        _fields_ = [
            ("size", ctypes.c_ulong), ("width", ctypes.c_long), ("height", ctypes.c_long),
            ("planes", ctypes.c_ushort), ("bit_count", ctypes.c_ushort), ("compression", ctypes.c_ulong),
            ("image_size", ctypes.c_ulong), ("x_pixels_per_meter", ctypes.c_long),
            ("y_pixels_per_meter", ctypes.c_long), ("colors_used", ctypes.c_ulong),
            ("colors_important", ctypes.c_ulong),
        ]

    class BitmapInfo(ctypes.Structure):
        _fields_ = [("header", BitmapInfoHeader), ("colors", ctypes.c_ulong * 3)]

    user32 = ctypes.WinDLL("user32", use_last_error=True)
    gdi32 = ctypes.WinDLL("gdi32", use_last_error=True)
    user32.GetWindowDC.restype = ctypes.c_void_p
    user32.GetWindowDC.argtypes = [ctypes.c_void_p]
    user32.GetWindowRect.argtypes = [ctypes.c_void_p, ctypes.POINTER(Rect)]
    user32.PrintWindow.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint]
    user32.PrintWindow.restype = ctypes.c_bool
    user32.ReleaseDC.argtypes = [ctypes.c_void_p, ctypes.c_void_p]
    gdi32.CreateCompatibleDC.restype = ctypes.c_void_p
    gdi32.CreateCompatibleDC.argtypes = [ctypes.c_void_p]
    gdi32.CreateCompatibleBitmap.restype = ctypes.c_void_p
    gdi32.CreateCompatibleBitmap.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_int]
    gdi32.SelectObject.restype = ctypes.c_void_p
    gdi32.SelectObject.argtypes = [ctypes.c_void_p, ctypes.c_void_p]
    gdi32.GetDIBits.argtypes = [
        ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint, ctypes.c_uint, ctypes.c_void_p,
        ctypes.POINTER(BitmapInfo), ctypes.c_uint,
    ]
    gdi32.DeleteObject.argtypes = [ctypes.c_void_p]
    gdi32.DeleteDC.argtypes = [ctypes.c_void_p]
    native_handle = ctypes.c_void_p(hwnd)
    rect = Rect()
    if not user32.GetWindowRect(native_handle, ctypes.byref(rect)):
        raise ctypes.WinError(ctypes.get_last_error())
    width, height = rect.right - rect.left, rect.bottom - rect.top
    if width <= 0 or height <= 0:
        raise RuntimeError("cannot capture an empty Windows window")
    window_dc = user32.GetWindowDC(native_handle)
    memory_dc = gdi32.CreateCompatibleDC(window_dc)
    bitmap = gdi32.CreateCompatibleBitmap(window_dc, width, height)
    if not window_dc or not memory_dc or not bitmap:
        raise ctypes.WinError(ctypes.get_last_error())
    old_bitmap = gdi32.SelectObject(memory_dc, bitmap)
    try:
        if not user32.PrintWindow(native_handle, memory_dc, 2):  # PW_RENDERFULLCONTENT
            raise ctypes.WinError(ctypes.get_last_error())
        gdi32.SelectObject(memory_dc, old_bitmap)
        info = BitmapInfo()
        stride = width * 4
        image_size = stride * height
        info.header = BitmapInfoHeader(40, width, height, 1, 32, 0, image_size, 0, 0, 0, 0)
        pixels = ctypes.create_string_buffer(image_size)
        if gdi32.GetDIBits(memory_dc, bitmap, 0, height, pixels, ctypes.byref(info), 0) != height:
            raise ctypes.WinError(ctypes.get_last_error())
        file_header = struct.pack("<2sIHHI", b"BM", 54 + image_size, 0, 0, 54)
        dib_header = struct.pack("<IiiHHIIiiII", 40, width, height, 1, 32, 0, image_size, 0, 0, 0, 0)
        destination.write_bytes(file_header + dib_header + pixels.raw)
    finally:
        gdi32.DeleteObject(bitmap)
        gdi32.DeleteDC(memory_dc)
        user32.ReleaseDC(native_handle, window_dc)


def _windows_click(hwnd: int, x: int, y: int) -> None:
    user32 = ctypes.WinDLL("user32", use_last_error=True)
    screen_x, screen_y = _windows_client_point(hwnd, x, y)
    if not user32.SetCursorPos(screen_x, screen_y):
        raise ctypes.WinError(ctypes.get_last_error())
    class MouseInput(ctypes.Structure):
        _fields_ = [
            ("dx", ctypes.c_long), ("dy", ctypes.c_long), ("mouseData", ctypes.c_ulong),
            ("dwFlags", ctypes.c_ulong), ("time", ctypes.c_ulong), ("dwExtraInfo", ctypes.c_size_t),
        ]

    class InputUnion(ctypes.Union):
        _fields_ = [("mi", MouseInput)]

    class Input(ctypes.Structure):
        _anonymous_ = ("data",)
        _fields_ = [("type", ctypes.c_ulong), ("data", InputUnion)]

    inputs = (Input * 2)(
        Input(type=0, mi=MouseInput(dwFlags=0x0002)),
        Input(type=0, mi=MouseInput(dwFlags=0x0004)),
    )
    if user32.SendInput(2, inputs, ctypes.sizeof(Input)) != 2:
        raise ctypes.WinError(ctypes.get_last_error())
def _windows_key(virtual_key: int) -> None:
    user32 = ctypes.WinDLL("user32", use_last_error=True)
    class KeyboardInput(ctypes.Structure):
        _fields_ = [
            ("wVk", ctypes.c_ushort), ("wScan", ctypes.c_ushort), ("dwFlags", ctypes.c_ulong),
            ("time", ctypes.c_ulong), ("dwExtraInfo", ctypes.c_size_t),
        ]

    class MouseInput(ctypes.Structure):
        _fields_ = [
            ("dx", ctypes.c_long), ("dy", ctypes.c_long), ("mouseData", ctypes.c_ulong),
            ("dwFlags", ctypes.c_ulong), ("time", ctypes.c_ulong), ("dwExtraInfo", ctypes.c_size_t),
        ]

    class InputUnion(ctypes.Union):
        _fields_ = [("ki", KeyboardInput), ("mi", MouseInput)]

    class Input(ctypes.Structure):
        _anonymous_ = ("data",)
        _fields_ = [("type", ctypes.c_ulong), ("data", InputUnion)]

    inputs = (Input * 2)(
        Input(type=1, ki=KeyboardInput(wVk=virtual_key)),
        Input(type=1, ki=KeyboardInput(wVk=virtual_key, dwFlags=0x0002)),
    )
    if user32.SendInput(2, inputs, ctypes.sizeof(Input)) != 2:
        raise ctypes.WinError(ctypes.get_last_error())


def _windows_chord(modifier: int, key: int) -> None:
    user32 = ctypes.WinDLL("user32", use_last_error=True)
    class KeyboardInput(ctypes.Structure):
        _fields_ = [
            ("wVk", ctypes.c_ushort), ("wScan", ctypes.c_ushort), ("dwFlags", ctypes.c_ulong),
            ("time", ctypes.c_ulong), ("dwExtraInfo", ctypes.c_size_t),
        ]

    class MouseInput(ctypes.Structure):
        _fields_ = [
            ("dx", ctypes.c_long), ("dy", ctypes.c_long), ("mouseData", ctypes.c_ulong),
            ("dwFlags", ctypes.c_ulong), ("time", ctypes.c_ulong), ("dwExtraInfo", ctypes.c_size_t),
        ]

    class InputUnion(ctypes.Union):
        _fields_ = [("ki", KeyboardInput), ("mi", MouseInput)]

    class Input(ctypes.Structure):
        _anonymous_ = ("data",)
        _fields_ = [("type", ctypes.c_ulong), ("data", InputUnion)]

    inputs = (Input * 4)(
        Input(type=1, ki=KeyboardInput(wVk=modifier)),
        Input(type=1, ki=KeyboardInput(wVk=key)),
        Input(type=1, ki=KeyboardInput(wVk=key, dwFlags=0x0002)),
        Input(type=1, ki=KeyboardInput(wVk=modifier, dwFlags=0x0002)),
    )
    if user32.SendInput(4, inputs, ctypes.sizeof(Input)) != 4:
        raise ctypes.WinError(ctypes.get_last_error())


def _windows_paste_text(value: str) -> None:
    user32 = ctypes.WinDLL("user32", use_last_error=True)
    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel32.GlobalAlloc.restype = ctypes.c_void_p
    kernel32.GlobalAlloc.argtypes = [ctypes.c_uint, ctypes.c_size_t]
    kernel32.GlobalLock.restype = ctypes.c_void_p
    kernel32.GlobalLock.argtypes = [ctypes.c_void_p]
    kernel32.GlobalUnlock.argtypes = [ctypes.c_void_p]
    kernel32.GlobalFree.restype = ctypes.c_void_p
    kernel32.GlobalFree.argtypes = [ctypes.c_void_p]
    user32.SetClipboardData.restype = ctypes.c_void_p
    user32.SetClipboardData.argtypes = [ctypes.c_uint, ctypes.c_void_p]
    if not user32.OpenClipboard(None):
        raise ctypes.WinError(ctypes.get_last_error())
    handle = None
    try:
        if not user32.EmptyClipboard():
            raise ctypes.WinError(ctypes.get_last_error())
        encoded = (value + "\0").encode("utf-16-le")
        handle = kernel32.GlobalAlloc(0x0002, len(encoded))  # GMEM_MOVEABLE
        if not handle:
            raise ctypes.WinError(ctypes.get_last_error())
        address = kernel32.GlobalLock(handle)
        if not address:
            raise ctypes.WinError(ctypes.get_last_error())
        try:
            ctypes.memmove(address, encoded, len(encoded))
        finally:
            kernel32.GlobalUnlock(handle)
        clipboard_handle = user32.SetClipboardData(13, handle)  # CF_UNICODETEXT
        if not clipboard_handle:
            error_code = ctypes.get_last_error()
            raise RuntimeError(
                f"SetClipboardData(CF_UNICODETEXT) failed for {len(encoded)} UTF-16 bytes: "
                f"{ctypes.WinError(error_code)}"
            )
        handle = None  # the system owns the allocation after SetClipboardData
    finally:
        user32.CloseClipboard()
        if handle:
            kernel32.GlobalFree(handle)
    _windows_chord(0x11, 0x41)  # Ctrl+A
    _windows_chord(0x11, 0x56)  # Ctrl+V; clipboard text is UTF-16

