"""Window identity, capture and controlled command input for Windows.

Window identity is never guessed from the title alone: every use confirms the
HWND against pid and process executable path. Command input uses a controlled
focus-verify-paste-confirm sequence; WGC capture and QR decoding are optional heavy imports that fail
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
import json
import ntpath
import uuid
import time
from collections import deque
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass

user32 = ctypes.WinDLL("user32", use_last_error=True)
kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)

INPUT_KEYBOARD = 1
KEYEVENTF_KEYUP = 0x0002
VK_CONTROL = 0x11
VK_RETURN = 0x0D
VK_V = 0x56
VK_A = 0x41
VK_C = 0x43
VK_ESCAPE = 0x1B
VK_OEM_2 = 0xBF
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
        if exe_path is not None and (
                ntpath.normcase(ntpath.normpath(self.exe_path or ""))
                != ntpath.normcase(ntpath.normpath(exe_path))):
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
                 mode: str = "messages") -> None:
    """Replace a chat draft and verify the copied edit text before submitting.

    A successful return means keyboard submission, not game execution. Callers
    must correlate the game's receipt. Messages mode is a separate HWND-bound
    background transport and does not use focus or the clipboard.
    """
    if mode not in ("foreground", "messages"):
        raise InputError(f"unknown delivery mode: {mode}")
    if (not isinstance(command, str) or not command.startswith("/")
            or len(command.encode("utf-8")) > 255
            or any(ord(char) < 32 or ord(char) == 127 for char in command)):
        raise InputError("expected one slash command of at most 255 UTF-8 bytes")
    if mode == "messages":
        send_command_messages(hwnd, command, pid=pid, exe_path=exe_path)
        return
    if clipboard is None:
        clipboard = Clipboard()
    identity = confirm_window(hwnd, pid=pid, exe_path=exe_path)
    # Freeze inferred identity too: callers need not supply both fields.
    pid, exe_path = identity.pid, identity.exe_path
    focus_window(hwnd, pid=pid, exe_path=exe_path)
    deadline = time.monotonic() + 5.0

    def guard():
        confirm_window(hwnd, pid=pid, exe_path=exe_path)
        if not is_foreground(hwnd):
            raise InputError("game lost foreground; input cancelled without retry")
        if time.monotonic() >= deadline:
            raise InputError("command input timed out; input cancelled without retry")

    def key(vk, *, ctrl=False):
        guard()
        _send_key(vk, ctrl=ctrl)
        time.sleep(0.10)
        guard()

    previous = clipboard.read_text()
    sentinel = "lycheedev-readback-" + uuid.uuid4().hex
    owned = {command, sentinel}
    try:
        # Open chat through the slash binding, never an initial Return: even
        # an ignored Escape must not submit an existing public-chat draft.
        key(VK_ESCAPE)
        key(VK_OEM_2)
        key(VK_A, ctrl=True)
        clipboard.copy_text(command)
        if clipboard.read_text() != command:
            raise InputError("the clipboard did not accept the command text")
        key(VK_V, ctrl=True)
        key(VK_A, ctrl=True)
        # Reading back the original clipboard would prove nothing. A unique
        # sentinel makes missing/ignored Ctrl+C distinguishable from success.
        clipboard.copy_text(sentinel)
        key(VK_C, ctrl=True)
        for _ in range(20):
            guard()
            copied = clipboard.read_text()
            if copied == command:
                break
            if copied != sentinel:
                raise InputError("chat edit text differs from the prepared command")
            time.sleep(0.025)
        else:
            raise InputError("could not verify chat edit text before submission")
        guard()
        _send_key(VK_RETURN)
    except BaseException:
        # Never send cleanup keys into another app or resume a deferred submit.
        try:
            confirm_window(hwnd, pid=pid, exe_path=exe_path)
            if is_foreground(hwnd):
                _send_key(VK_ESCAPE)
        except (WindowIdentityError, InputError):
            pass
        raise
    finally:
        if clipboard.read_text() in owned:
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
                confirm_window(hwnd, pid=pid, exe_path=exe_path)
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
            "window once and retry the command")


def _key_lparam(vk: int, *, up: bool = False) -> int:
    """Pack a WM_KEYDOWN/WM_KEYUP lParam for a virtual key."""

    scan = user32.MapVirtualKeyW(vk, 0)
    lparam = 1 | (scan << 16)
    if up:
        lparam |= 1 << 30 | 1 << 31
    return lparam


def send_command_messages(hwnd: int, command: str, *, delay: float = 0.05,
                          pid: int | None = None, exe_path: str | None = None) -> None:
    """Submit through the target HWND's queue without changing foreground.

    PostMessage acceptance only means queued input. It cannot prove chat focus
    or text consumption; callers must wait for a matching game receipt.
    """
    if (not isinstance(command, str) or not command.startswith("/")
            or len(command.encode("utf-8")) > 255
            or any(ord(char) < 32 or ord(char) == 127 for char in command)):
        raise InputError("expected one slash command of at most 255 UTF-8 bytes")
    identity = confirm_window(hwnd, pid=pid, exe_path=exe_path)
    pid, exe_path = identity.pid, identity.exe_path
    deadline = time.monotonic() + 20.0

    def post(message, value, lparam):
        confirm_window(hwnd, pid=pid, exe_path=exe_path)
        if time.monotonic() >= deadline:
            raise InputError("background input timed out; submission is unresolved")
        if not user32.PostMessageW(hwnd, message, value, lparam):
            raise InputError("target rejected background input; submission is unresolved")

    def key(vk):
        post(WM_KEYDOWN, vk, _key_lparam(vk))
        post(WM_KEYUP, vk, _key_lparam(vk, up=True))
        time.sleep(0.15)

    try:
        key(VK_ESCAPE)
        key(VK_RETURN)
        # WM_CHAR carries UTF-16 code units, not Python's Unicode code points.
        encoded = command.encode("utf-16-le")
        for offset in range(0, len(encoded), 2):
            post(WM_CHAR, int.from_bytes(encoded[offset:offset + 2], "little"), 1)
            time.sleep(delay)
        key(VK_RETURN)
    except BaseException:
        # Queue cleanup only to the still-bound HWND, never to the foreground.
        try:
            confirm_window(hwnd, pid=pid, exe_path=exe_path)
            user32.PostMessageW(hwnd, WM_KEYDOWN, VK_ESCAPE, _key_lparam(VK_ESCAPE))
            user32.PostMessageW(hwnd, WM_KEYUP, VK_ESCAPE, _key_lparam(VK_ESCAPE, up=True))
        except WindowIdentityError:
            pass
        raise


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


def _identity_payload(payloads) -> dict | None:
    """Pick the identity marker out of decoded payloads.

    A window may show the completion notice instead of an identity marker, and
    both are JSON with a ``v`` field, so the shape is checked rather than
    assumed: an identity marker carries a character id, a notice carries a
    ticket.
    """

    for text in payloads:
        try:
            value = json.loads(text)
        except (TypeError, ValueError):
            continue
        if not isinstance(value, dict) or "id" not in value:
            continue
        if "ticket" in value:
            continue
        return value
    return None


# The identity probe's waiting budget, sized from measurement rather than guess:
# on a live window Windows.Graphics.Capture delivers the first frame in 78-94 ms
# (234 ms cold) and decoding an empty ROI costs about 3 ms. The grace therefore
# only has to cover a slow first frame, and the timeout only a window that never
# delivers one at all.
IDENTITY_FIRST_FRAME_GRACE = 0.35
IDENTITY_TIMEOUT = 1.5


def read_identity(hwnd: int, *, pid: int | None = None, exe_path: str | None = None,
                  timeout: float = IDENTITY_TIMEOUT, interval: float = 0.03,
                  first_frame_grace: float = IDENTITY_FIRST_FRAME_GRACE,
                  minimum_update_interval_ms: int | None = None) -> dict | None:
    """Read the in-game identity marker from one window, or None.

    This exists because nothing observable from outside the game says which
    character is behind which window. With one window that does not matter; with
    several it decides where a slash command may be typed. The window does not
    need to be in the foreground, because Windows.Graphics.Capture reads the
    window surface directly, which is what makes this usable before any input is
    sent.

    "No marker" is the ordinary state, not a failure: the user shows one on
    request. The wait therefore ends as soon as one frame has been decoded
    without finding a marker and no newer frame follows, so an unmarked window
    costs ``first_frame_grace`` rather than the whole ``timeout``. The timeout
    remains the ceiling for a window that never delivers a frame at all.

    Returns the decoded marker, or None. Raises WindowIdentityError for a stale
    hwnd and CaptureDependencyError when the optional packages are missing.
    """

    deadline = time.monotonic() + max(0.0, timeout)
    grace = max(0.0, first_frame_grace)
    last_attempted = None
    first_frame_at = None
    with open_notice_capture(hwnd, pid=pid, exe_path=exe_path, interval=interval,
                             minimum_update_interval_ms=minimum_update_interval_ms) as session:
        while time.monotonic() < deadline:
            roi = session.latest_roi()
            # The session keeps the newest frame until a newer one arrives, so
            # decode each distinct frame once: re-decoding an unchanged image
            # every interval would burn the whole timeout on one picture.
            if roi is not None and roi is not last_attempted:
                if first_frame_at is None:
                    first_frame_at = time.monotonic()
                last_attempted = roi
                marker = _identity_payload(decode_qr(roi))
                if marker is not None:
                    return marker
            if session.window_closed:
                return None
            if first_frame_at is not None and time.monotonic() - first_frame_at >= grace:
                # Frames are arriving and none carried a marker, so waiting
                # longer cannot change the answer.
                return None
            time.sleep(interval)
    return None


def _probe_one(window, timeout: float, interval: float,
               first_frame_grace: float, reader=None) -> dict:
    """Probe one window and return its result entry, never raising."""

    entry = {
        "hwnd": window.get("hwnd"),
        "pid": window.get("pid"),
        "flavorFolder": window.get("flavorFolder"),
        "flavorId": window.get("flavorId"),
        "flavorLabel": window.get("flavorLabel"),
        "identity": None,
        "error": None,
    }
    probe = reader if reader is not None else read_identity
    try:
        entry["identity"] = probe(
            int(window["hwnd"]),
            pid=window.get("pid"),
            exe_path=window.get("exePath"),
            timeout=timeout,
            interval=interval,
            first_frame_grace=first_frame_grace,
        )
    except (WindowIdentityError, CaptureDependencyError) as error:
        # These say something about the request or the environment rather than
        # about one window, so report them and keep going; the caller decides
        # whether an unreadable set is fatal.
        entry["error"] = str(error)
    except (OSError, RuntimeError, ValueError) as error:
        # One window being uncooperative (closed, minimized, denied) must not
        # abort the probe of the remaining windows.
        entry["error"] = str(error)
    return entry


def read_identities(windows, *, timeout: float = IDENTITY_TIMEOUT, interval: float = 0.03,
                    first_frame_grace: float = IDENTITY_FIRST_FRAME_GRACE,
                    reader=None) -> list[dict]:
    """Probe several windows for their identity marker, in the given order.

    The windows are probed concurrently. Each call owns its own capture session,
    the frame callback only touches that session's queue, and the waiting is
    sleep-bound, so the probe costs one window's latency instead of the sum of
    all of them. With one window this is exactly the old behavior.

    A window without a visible marker yields an entry with ``identity`` set to
    None rather than an error, because "no marker" is the expected state until
    the user asks the game to show one. ``reader`` exists so the concurrency and
    error isolation can be tested without a game window.
    """

    items = list(windows)
    if not items:
        return []

    def probe(window: dict) -> dict:
        return _probe_one(window, timeout, interval, first_frame_grace, reader)

    # Threads rather than processes: the work is blocking waits and small
    # decodes, so the GIL is not the constraint, and every entry stays picklable
    # for callers that serialize the result.
    workers = min(len(items), 8)
    with ThreadPoolExecutor(max_workers=workers) as pool:
        return list(pool.map(probe, items))


