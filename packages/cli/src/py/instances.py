"""List running World of Warcraft instances as JSON.

The executable name cannot identify a build: Blizzard ships every non-retail
client as ``WowClassic.exe``. The install path can, because each build lives in
a ``_flavor_`` directory. Visible top-level window handles are attached so the
caller can tell same-build instances apart.

Read-only: this opens no process, only enumerates windows and process metadata.
"""

from __future__ import annotations

import ctypes
import json
import os
import sys
from ctypes import wintypes

user32 = ctypes.windll.user32
kernel32 = ctypes.windll.kernel32

PROCESS_QUERY_LIMITED_INFORMATION = 0x1000
ERROR_INSUFFICIENT_BUFFER = 122

user32.EnumWindows.restype = wintypes.BOOL
user32.EnumWindows.argtypes = [ctypes.c_void_p, wintypes.LPARAM]
user32.IsWindowVisible.restype = wintypes.BOOL
user32.GetWindowTextLengthW.restype = ctypes.c_int
user32.GetWindowTextW.restype = ctypes.c_int
user32.GetWindowThreadProcessId.restype = wintypes.DWORD
user32.GetWindowThreadProcessId.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.DWORD)]

kernel32.OpenProcess.restype = wintypes.HANDLE
kernel32.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
kernel32.QueryFullProcessImageNameW.restype = wintypes.BOOL
kernel32.QueryFullProcessImageNameW.argtypes = [
    wintypes.HANDLE, wintypes.DWORD, wintypes.LPWSTR, ctypes.POINTER(wintypes.DWORD)]
kernel32.CloseHandle.restype = wintypes.BOOL
kernel32.CloseHandle.argtypes = [wintypes.HANDLE]

EnumWindowsProc = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)

FLAVOR_LABELS = {
    "_retail_": ("retail", "Retail"),
    "_classic_": ("classic", "Classic"),
    "_classic_titan_": ("titan", "Classic Titan"),
    "_classic_era_": ("classicEra", "Classic Era"),
    "_anniversary_": ("anniversary", "Anniversary"),
    "_beta_": ("beta", "Beta"),
}

SUPPORTED_EXE = {"wow.exe", "wowclassic.exe", "wowb.exe"}


def process_image(pid: int) -> str | None:
    handle = kernel32.OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, False, pid)
    if not handle:
        return None
    try:
        size = wintypes.DWORD(32768)
        buffer = ctypes.create_unicode_buffer(size.value)
        if not kernel32.QueryFullProcessImageNameW(handle, 0, buffer, ctypes.byref(size)):
            return None
        return buffer.value or None
    finally:
        kernel32.CloseHandle(handle)


def visible_windows() -> dict[int, dict]:
    found: dict[int, dict] = {}

    def callback(hwnd, _lparam):
        if not user32.IsWindowVisible(hwnd):
            return True
        pid = wintypes.DWORD(0)
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
        length = user32.GetWindowTextLengthW(hwnd)
        if length <= 0:
            return True
        buffer = ctypes.create_unicode_buffer(length + 1)
        user32.GetWindowTextW(hwnd, buffer, length + 1)
        found.setdefault(pid.value, {"hwnd": int(hwnd), "title": buffer.value})
        return True

    user32.EnumWindows(EnumWindowsProc(callback), 0)
    return found


def main() -> int:
    windows = visible_windows()
    instances = []

    for pid in windows:
        image = process_image(pid)
        if not image:
            continue
        base = os.path.basename(image).lower()
        if base not in SUPPORTED_EXE:
            continue
        parts = image.replace("/", os.sep).split(os.sep)
        folder = next((part for part in parts if part.startswith("_") and part.endswith("_")), None)
        flavor_id, label = FLAVOR_LABELS.get(folder, (None, None))
        instances.append({
            "pid": pid,
            "exe": os.path.basename(image),
            "exePath": image,
            "flavorFolder": folder,
            "flavorId": flavor_id,
            "flavorLabel": label,
            "hwnd": windows[pid]["hwnd"],
            "title": windows[pid]["title"],
        })

    instances.sort(key=lambda item: (item.get("flavorId") or "", item["pid"]))
    print(json.dumps(instances, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    sys.exit(main())
