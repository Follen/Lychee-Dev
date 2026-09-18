"""List running World of Warcraft instances as JSON.

The executable name cannot identify a build: Blizzard ships every non-retail
client as ``WowClassic.exe``. The install path narrows it to a ``_flavor_``
folder, but a folder is a location rather than an identity - the game reuses a
test folder for whatever is on the test track, so ``_classic_beta_`` currently
carries WoW: Forever (1.60.x) on that track. The folder's own ``.flavor.info``
product code and ``version.txt`` build are reported alongside the window handle
so the caller can identify the client from the client itself.

Read-only: this opens no process, only enumerates windows and reads process
metadata plus two small per-folder files.
"""

from __future__ import annotations

import ctypes
import json
import os
import re
import sys
from ctypes import wintypes

user32 = ctypes.windll.user32
kernel32 = ctypes.windll.kernel32

PROCESS_QUERY_LIMITED_INFORMATION = 0x1000
ERROR_INSUFFICIENT_BUFFER = 122

_FLAVOR_PRODUCT = re.compile(rb"\b(wow[a-z0-9_]*)\b", re.IGNORECASE)

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
    # Folder default only: this folder also carries WoW: Forever on the test
    # track, which the caller resolves from the folder's own product code.
    "_classic_beta_": ("classicBeta", "Classic Beta"),
    "_forever_": ("forever", "Forever"),
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


def _read_small_text(path: str, limit: int = 512) -> str | None:
    try:
        with open(path, "rb") as handle:
            return handle.read(limit).decode("utf-8", "replace").strip() or None
    except OSError:
        return None


def folder_product(version_dir: str) -> str | None:
    """The folder's declared product code, e.g. ``wow_classic_titan``."""
    for name in (".flavor.info", "flavor.info"):
        text = _read_small_text(os.path.join(version_dir, name))
        if not text:
            continue
        match = _FLAVOR_PRODUCT.search(text.encode("utf-8", "replace"))
        if match:
            return match.group(1).decode("ascii", "replace").lower()
    return None


def folder_version(version_dir: str) -> str | None:
    """The folder's build, e.g. ``1.60.1.69893`` from ``version.txt``."""
    return _read_small_text(os.path.join(version_dir, "version.txt"))


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
        # Folder defaults only. The caller re-resolves the flavor from `product`
        # and `version`, because a folder name is a location rather than an
        # identity; keeping the product rule in one place avoids two mappings
        # that can disagree about a reused test folder.
        flavor_id, label = FLAVOR_LABELS.get(folder, (None, None))
        version_dir = os.path.dirname(image)
        product = folder_product(version_dir)
        version = folder_version(version_dir)
        instances.append({
            "pid": pid,
            "exe": os.path.basename(image),
            "exePath": image,
            "flavorFolder": folder,
            "flavorId": flavor_id,
            "flavorLabel": label,
            "product": product,
            "version": version,
            "hwnd": windows[pid]["hwnd"],
            "title": windows[pid]["title"],
        })

    instances.sort(key=lambda item: (item.get("flavorId") or "", item["pid"]))
    print(json.dumps(instances, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    sys.exit(main())
