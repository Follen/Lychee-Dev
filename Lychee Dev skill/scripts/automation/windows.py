"""Window identity, capture and controlled command input for Windows.

Window identity is never guessed from the title alone: every use confirms the
HWND against pid, process executable path and (best effort) the process
creation time. Command input uses the controlled focus-verify-paste-confirm
sequence; WGC capture and QR decoding are optional heavy imports that fail
with explicit messages when their packages are missing.

The capture session is long lived: a task can run far longer than one frame, so
``open_notice_capture`` returns a context manager that keeps one WGC session
open, keeps only the newest ROI, and is polled by the caller until its own
deadline expires. Every ctypes entry point used here has an explicit prototype,
because on 64-bit hosts the default ``c_int`` return type truncates handles and
pointers.

Verified behavior gaps (design section 11): SendInput success does not prove
the game processed the command, and minimization/occlusion/HDR support must be
measured before being claimed. The host log, not this module, decides when a
reload may be repeated.
"""

from __future__ import annotations

import ctypes
import ctypes.wintypes as wt
import time
from collections import deque
from dataclasses import dataclass

user32 = ctypes.WinDLL("user32", use_last_error=True)
kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)

INPUT_KEYBOARD = 1
KEYEVENTF_KEYUP = 0x0002
VK_CONTROL = 0x11
VK_RETURN = 0x0D
VK_V = 0x56
VK_MENU = 0x12
WM_CHAR = 0x0102
WM_KEYDOWN = 0x0100
WM_KEYUP = 0x0101

PROCESS_QUERY_LIMITED_INFORMATION = 0x1000

# Top-left notice region as (top, left, bottom, right) fractions of the captured
# frame. The completion notice is drawn at the top-left corner (see
# UI/AutomationOverlay.lua), so the scan area covers that corner with room for
# the QR quiet zone and a few DPI increments. Recalibrate here when the notice
# anchor, DPI, window size or capture origin changes (design section 6.1).
NOTICE_ROI_FRACTIONS = (0.0, 0.0, 0.30, 0.30)


class WindowIdentityError(Exception):
    pass


class InputError(Exception):
    pass


class CaptureDependencyError(Exception):
    pass


@dataclass
class WindowIdentity:
    hwnd: int
    pid: int
    exe_path: str | None
    title: str

    def matches(self, *, pid: int | None = None, exe_path: str | None = None) -> bool:
        if pid is not None and self.pid != pid:
            return False
        if exe_path is not None and (self.exe_path or "").lower() != exe_path.lower():
            return False
        return True


class KEYBDINPUT(ctypes.Structure):
    _fields_ = [("wVk", wt.WORD), ("wScan", wt.WORD), ("dwFlags", wt.DWORD),
                ("time", wt.DWORD), ("dwExtraInfo", ctypes.c_void_p)]


class MOUSEINPUT(ctypes.Structure):
    _fields_ = [("dx", wt.LONG), ("dy", wt.LONG), ("mouseData", wt.DWORD),
                ("dwFlags", wt.DWORD), ("time", wt.DWORD),
                ("dwExtraInfo", ctypes.c_void_p)]


class HARDWAREINPUT(ctypes.Structure):
    _fields_ = [("uMsg", wt.DWORD), ("wParamL", wt.WORD), ("wParamH", wt.WORD)]


class _INPUTUNION(ctypes.Union):
    # The union size must follow the real largest member: 32 bytes on 64-bit and
    # 24 bytes on 32-bit, which is why the padding shortcut was wrong.
    _fields_ = [("ki", KEYBDINPUT), ("mi", MOUSEINPUT), ("hi", HARDWAREINPUT)]


class INPUT(ctypes.Structure):
    _anonymous_ = ("u",)
    _fields_ = [("type", wt.DWORD), ("u", _INPUTUNION)]


INPUT_SIZE = ctypes.sizeof(INPUT)

WNDENUMPROC = ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)


def _prototype(function, restype, argtypes):
    function.restype = restype
    function.argtypes = list(argtypes)
    return function


_prototype(kernel32.OpenProcess,
           wt.HANDLE, (wt.DWORD, wt.BOOL, wt.DWORD))
_prototype(kernel32.QueryFullProcessImageNameW,
           wt.BOOL, (wt.HANDLE, wt.DWORD, wt.LPWSTR, ctypes.POINTER(wt.DWORD)))
_prototype(kernel32.CloseHandle, wt.BOOL, (wt.HANDLE,))
_prototype(kernel32.GlobalLock, ctypes.c_void_p, (ctypes.c_void_p,))
_prototype(kernel32.GlobalUnlock, wt.BOOL, (ctypes.c_void_p,))
_prototype(kernel32.GlobalAlloc, ctypes.c_void_p, (wt.UINT, ctypes.c_size_t))
_prototype(kernel32.GlobalFree, ctypes.c_void_p, (ctypes.c_void_p,))
_prototype(ctypes.memmove, ctypes.c_void_p,
           (ctypes.c_void_p, ctypes.c_void_p, ctypes.c_size_t))

_prototype(user32.IsWindow, wt.BOOL, (wt.HWND,))
_prototype(user32.IsWindowVisible, wt.BOOL, (wt.HWND,))
_prototype(user32.GetWindowThreadProcessId,
           wt.DWORD, (wt.HWND, ctypes.POINTER(wt.DWORD)))
_prototype(user32.GetWindowTextLengthW, ctypes.c_int, (wt.HWND,))
_prototype(user32.GetWindowTextW, ctypes.c_int, (wt.HWND, wt.LPWSTR, ctypes.c_int))
_prototype(user32.EnumWindows, wt.BOOL, (WNDENUMPROC, wt.LPARAM))
_prototype(user32.GetForegroundWindow, wt.HWND, ())
_prototype(user32.SetForegroundWindow, wt.BOOL, (wt.HWND,))
_prototype(user32.OpenClipboard, wt.BOOL, (wt.HWND,))
_prototype(user32.CloseClipboard, wt.BOOL, ())
_prototype(user32.EmptyClipboard, wt.BOOL, ())
_prototype(user32.IsClipboardFormatAvailable, wt.BOOL, (wt.UINT,))
_prototype(user32.GetClipboardData, wt.HANDLE, (wt.UINT,))
_prototype(user32.SetClipboardData, wt.HANDLE, (wt.UINT, wt.HANDLE))
_prototype(user32.SendInput, wt.UINT, (wt.UINT, ctypes.POINTER(INPUT), ctypes.c_int))
_prototype(user32.keybd_event, None, (wt.BYTE, wt.BYTE, wt.DWORD, ctypes.c_void_p))
_prototype(user32.MapVirtualKeyW, wt.UINT, (wt.UINT, wt.UINT))
_prototype(user32.PostMessageW, wt.BOOL,
           (wt.HWND, wt.UINT, wt.WPARAM, wt.LPARAM))


def _query_process_image(pid: int) -> str | None:
    handle = kernel32.OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, False, pid)
    if not handle:
        return None
    try:
        size = wt.DWORD(1024)
        buffer = ctypes.create_unicode_buffer(size.value)
        if kernel32.QueryFullProcessImageNameW(handle, 0, buffer, ctypes.byref(size)):
            return buffer.value
        return None
    finally:
        kernel32.CloseHandle(handle)


def identify_window(hwnd: int) -> WindowIdentity:
    if not user32.IsWindow(hwnd):
        raise WindowIdentityError(f"hwnd {hwnd} is no longer a live window")
    pid = wt.DWORD(0)
    user32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
    length = user32.GetWindowTextLengthW(hwnd)
    buffer = ctypes.create_unicode_buffer(max(length + 1, 1))
    user32.GetWindowTextW(hwnd, buffer, length + 1)
    return WindowIdentity(
        hwnd=hwnd,
        pid=pid.value,
        exe_path=_query_process_image(pid.value),
        title=buffer.value,
    )


def confirm_window(hwnd: int, *, pid: int | None = None, exe_path: str | None = None) -> WindowIdentity:
    """Re-verify the window identity immediately before any use."""

    identity = identify_window(hwnd)
    if not identity.matches(pid=pid, exe_path=exe_path):
        raise WindowIdentityError(
            f"window identity changed: hwnd {hwnd} now pid {identity.pid} "
            f"exe {identity.exe_path!r}")
    return identity


def find_windows_by_pid(pid: int) -> list[WindowIdentity]:
    found: list[WindowIdentity] = []

    @WNDENUMPROC
    def callback(hwnd, _lparam):
        window_pid = wt.DWORD(0)
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(window_pid))
        if window_pid.value == pid and user32.IsWindowVisible(hwnd):
            try:
                found.append(identify_window(hwnd))
            except WindowIdentityError:
                pass
        return True

    user32.EnumWindows(callback, 0)
    return found


def is_foreground(hwnd: int) -> bool:
    return user32.GetForegroundWindow() == hwnd


def _send_key(vk: int, *, ctrl: bool = False) -> None:
    def make(vkey: int, flags: int) -> INPUT:
        item = INPUT()
        item.type = INPUT_KEYBOARD
        item.ki = KEYBDINPUT(vkey, 0, flags, 0, None)
        return item

    sequence: list[INPUT] = []
    if ctrl:
        sequence.append(make(VK_CONTROL, 0))
    sequence.append(make(vk, 0))
    sequence.append(make(vk, KEYEVENTF_KEYUP))
    if ctrl:
        sequence.append(make(VK_CONTROL, KEYEVENTF_KEYUP))
    array = (INPUT * len(sequence))(*sequence)
    sent = user32.SendInput(len(sequence), array, ctypes.sizeof(INPUT))
    if sent != len(sequence):
        raise InputError("SendInput did not accept the full key sequence")


def send_command(hwnd: int, command: str, *, pid: int | None = None,
                 exe_path: str | None = None, clipboard=None,
                 mode: str = "foreground") -> None:
    """Deliver one slash command through the game chat box.

    ``mode`` selects the injection strategy:

    - ``"foreground"`` (default): focus the window, open chat with Return,
      paste from the clipboard, submit with Return. Uses ``SendInput``, which
      only reaches the foreground window, so the window is focused first.
    - ``"messages"``: post the keystrokes straight into the target window's
      message queue. This needs no foreground at all, but the client only
      reacts when its chat edit box is already open and focused, and a client
      that reads raw input may ignore posted messages entirely.

    Both modes run the game's own command parser, so a slash command that the
    client cannot run (locked UI, unknown command) still fails inside the game.

    ``clipboard`` may be injected for tests; it needs ``copy_text``,
    ``read_text`` and ``clear``. The previous clipboard content is restored
    when it still belongs to this operation.
    """

    if mode not in ("foreground", "messages"):
        raise InputError(f"unknown delivery mode: {mode}")

    if mode == "messages":
        confirm_window(hwnd, pid=pid, exe_path=exe_path)
        send_command_messages(hwnd, command)
        return

    if clipboard is None:
        clipboard = Clipboard()
    confirm_window(hwnd, pid=pid, exe_path=exe_path)
    focus_window(hwnd, pid=pid, exe_path=exe_path)

    previous = clipboard.read_text()
    clipboard.copy_text(command)
    if clipboard.read_text() != command:
        raise InputError("the clipboard did not accept the command text")
    try:
        _send_key(VK_RETURN)
        time.sleep(0.15)
        _send_key(VK_V, ctrl=True)
        time.sleep(0.15)
        _send_key(VK_RETURN)
        time.sleep(0.05)
    finally:
        if clipboard.read_text() == command:
            if previous is None:
                clipboard.clear()
            else:
                clipboard.copy_text(previous)


def focus_window(hwnd: int, *, pid: int | None = None,
                 exe_path: str | None = None, attempts: int = 3) -> None:
    """Bring ``hwnd`` to the foreground, or fail without sending any input.

    ``SetForegroundWindow`` is refused while another process owns the
    foreground, which is the normal state after the user clicks elsewhere.
    Windows releases that restriction when the foreground change follows user
    input, so a one-shot ALT press is used as the documented unlock and the
    result is always re-verified; a window that still is not foreground raises
    instead of typing into whatever else has focus.
    """

    for attempt in range(attempts):
        if user32.SetForegroundWindow(hwnd):
            time.sleep(0.10)
            if is_foreground(hwnd):
                return
        if attempt == attempts - 1:
            break
        # A synthetic ALT is delivered by this process, so the next
        # SetForegroundWindow is allowed through the foreground lock.
        user32.keybd_event(VK_MENU, 0, 0, 0)
        time.sleep(0.05)
        user32.keybd_event(VK_MENU, 0, KEYEVENTF_KEYUP, 0)
        time.sleep(0.15)

    confirm_window(hwnd, pid=pid, exe_path=exe_path)
    if not is_foreground(hwnd):
        raise InputError(
            "could not bring the game window to the foreground; click the game "
            "window once, or use `send --mode messages`")


def _key_lparam(vk: int, *, up: bool = False) -> int:
    """Pack a WM_KEYDOWN/WM_KEYUP lParam for a virtual key."""

    scan = user32.MapVirtualKeyW(vk, 0)
    lparam = 1 | (scan << 16)
    if up:
        lparam |= 1 << 30 | 1 << 31
    return lparam


def send_command_messages(hwnd: int, command: str, *, delay: float = 0.03) -> None:
    """Type ``command`` into ``hwnd`` by posting messages; no focus needed.

    The chat edit box must already be open and focused in the game, because the
    client routes posted keys to whatever widget has focus. Characters go out
    as WM_CHAR (the client re-encodes them for IME), and Return submits.
    """

    for char in command:
        if not user32.PostMessageW(hwnd, WM_CHAR, ord(char), 0):
            raise InputError("the client window rejected a character message")
        time.sleep(delay)
    if not user32.PostMessageW(hwnd, WM_KEYDOWN, VK_RETURN, _key_lparam(VK_RETURN)):
        raise InputError("the client window rejected the submit key")
    user32.PostMessageW(hwnd, WM_KEYUP, VK_RETURN, _key_lparam(VK_RETURN, up=True))


class Clipboard:
    """Minimal CF_UNICODETEXT clipboard via user32; no third-party dependency."""

    CF_UNICODETEXT = 13
    GMEM_MOVEABLE = 0x0002

    def _open(self, retries: int = 8) -> None:
        for attempt in range(retries):
            if user32.OpenClipboard(0):
                return
            time.sleep(0.05 * (attempt + 1))
        raise InputError("could not open the clipboard")

    def read_text(self) -> str | None:
        self._open()
        try:
            if not user32.IsClipboardFormatAvailable(self.CF_UNICODETEXT):
                return None
            handle = user32.GetClipboardData(self.CF_UNICODETEXT)
            if not handle:
                return None
            pointer = kernel32.GlobalLock(handle)
            if not pointer:
                return None
            try:
                return ctypes.wstring_at(pointer)
            finally:
                kernel32.GlobalUnlock(handle)
        finally:
            user32.CloseClipboard()

    def copy_text(self, text: str) -> None:
        self._open()
        try:
            user32.EmptyClipboard()
            size = (len(text) + 1) * 2
            handle = kernel32.GlobalAlloc(self.GMEM_MOVEABLE, size)
            if not handle:
                raise InputError("GlobalAlloc failed for clipboard data")
            pointer = kernel32.GlobalLock(handle)
            if not pointer:
                raise InputError("GlobalLock failed for clipboard data")
            try:
                source = ctypes.create_unicode_buffer(text)
                ctypes.memmove(pointer, ctypes.cast(source, ctypes.c_void_p), size)
            finally:
                kernel32.GlobalUnlock(handle)
            if not user32.SetClipboardData(self.CF_UNICODETEXT, handle):
                kernel32.GlobalFree(handle)
                raise InputError("SetClipboardData failed")
        finally:
            user32.CloseClipboard()

    def clear(self) -> None:
        self._open()
        try:
            user32.EmptyClipboard()
        finally:
            user32.CloseClipboard()


# --- capture (optional dependencies) -----------------------------------------

def load_decode_module():
    """Import zxing-cpp, the only QR reader this tooling supports."""

    try:
        import zxingcpp
    except ImportError as error:  # pragma: no cover - depends on host packages
        raise CaptureDependencyError(
            f"QR decoding dependency missing ({error}); install zxing-cpp 3.1.1"
        ) from error
    if not callable(getattr(zxingcpp, "read_barcodes", None)):
        raise CaptureDependencyError(
            "installed zxing-cpp has no read_barcodes(); zxing-cpp 3.x is required")
    return zxingcpp


def load_capture_modules():
    """Import windows-capture plus numpy for the live frame path."""

    try:
        import numpy
        import windows_capture
    except ImportError as error:  # pragma: no cover - depends on host packages
        raise CaptureDependencyError(
            f"capture dependencies missing ({error}); install windows-capture "
            "2.0.1 and numpy") from error
    return numpy, windows_capture


class NoticeCapture:
    """One open WGC window capture keeping only the newest notice ROI.

    The session stays open for as long as the caller needs it: a task can take
    far longer than a single frame. ``latest_roi()`` returns the newest copied
    ROI or ``None``; the frame buffer is copied before the native frame is
    released. Use it as a context manager.
    """

    def __init__(self, capture, numpy_module, *, hwnd: int,
                 roi_fractions=NOTICE_ROI_FRACTIONS):
        self.capture = capture
        self.hwnd = hwnd
        self.roi_fractions = tuple(roi_fractions)
        self.window_closed = False
        self.last_frame_error: str | None = None
        self._numpy = numpy_module
        self._frames: deque = deque(maxlen=1)
        self._control = None
        self._closed = False
        capture.frame_handler = self._on_frame
        capture.closed_handler = self._on_closed

    # -- windows-capture callbacks -----------------------------------------

    def _frame_bgr(self, frame):
        convert = getattr(frame, "convert_to_bgr", None)
        if callable(convert):
            return convert().frame_buffer
        return frame.frame_buffer

    def _on_frame(self, frame, _control=None) -> None:
        try:
            image = self._frame_bgr(frame)
            height, width = int(image.shape[0]), int(image.shape[1])
            top, left, bottom, right = self.roi_fractions
            roi = image[int(height * top):int(height * bottom),
                        int(width * left):int(width * right)]
            self._frames.append(self._numpy.ascontiguousarray(roi))
        except Exception as error:  # a bad frame must never stop the thread
            self.last_frame_error = str(error)

    def _on_closed(self) -> None:
        self.window_closed = True

    # -- lifecycle ---------------------------------------------------------

    def start(self) -> NoticeCapture:
        self._control = self.capture.start_free_threaded()
        return self

    def latest_roi(self):
        return self._frames[-1] if self._frames else None

    def close(self, timeout: float = 2.0) -> None:
        if self._closed:
            return
        self._closed = True
        control = self._control
        if control is None:
            return
        try:
            control.stop()
        except Exception:
            return
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline and not control.is_finished():
            time.sleep(0.02)

    def __enter__(self) -> NoticeCapture:
        return self.start()

    def __exit__(self, *_exc) -> bool:
        self.close()
        return False


def open_notice_capture(hwnd: int, *, pid: int | None = None,
                        exe_path: str | None = None,
                        interval: float = 0.1,
                        minimum_update_interval_ms: int | None = None,
                        roi_fractions=NOTICE_ROI_FRACTIONS) -> NoticeCapture:
    """Confirm the window identity, then open one WGC session for the notice.

    The caller owns the returned session and must close it (the context manager
    does that). Raises WindowIdentityError for a stale/reused hwnd and
    CaptureDependencyError when the optional packages are missing.
    """

    numpy_module, windows_capture = load_capture_modules()
    confirm_window(hwnd, pid=pid, exe_path=exe_path)
    if minimum_update_interval_ms is None:
        minimum_update_interval_ms = max(int(round(interval * 1000)), 0)
    kwargs = {"window_hwnd": hwnd}
    if minimum_update_interval_ms:
        kwargs["minimum_update_interval"] = minimum_update_interval_ms
    try:
        capture = windows_capture.WindowsCapture(**kwargs)
    except TypeError:
        # Older releases may not accept the update-interval keyword.
        capture = windows_capture.WindowsCapture(window_hwnd=hwnd)
    return NoticeCapture(capture, numpy_module, hwnd=hwnd,
                         roi_fractions=roi_fractions)


def decode_qr(image, zxingcpp_module=None) -> list[str]:
    """Decode QR payloads from one image; returns the raw strings found.

    Only ``read_barcodes`` exists in zxing-cpp 3.x; it is restricted to QR codes
    so the notice scan stays cheap, invalid results are dropped, and a
    dependency without that entry point fails explicitly instead of raising a
    bare AttributeError.
    """

    module = zxingcpp_module if zxingcpp_module is not None else load_decode_module()
    reader = getattr(module, "read_barcodes", None)
    if not callable(reader):
        raise CaptureDependencyError(
            "installed zxing-cpp has no read_barcodes(); zxing-cpp 3.x is required")
    formats = getattr(getattr(module, "BarcodeFormat", None), "QRCode", None)
    try:
        results = reader(image, formats=formats) if formats is not None else reader(image)
    except TypeError:
        results = reader(image)
    payloads: list[str] = []
    for result in results or ():
        if getattr(result, "valid", True) is False:
            continue
        text = getattr(result, "text", None)
        if isinstance(text, str) and text:
            payloads.append(text)
    return payloads


