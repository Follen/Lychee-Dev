"""Nonce-bound reload readiness; images remain on the host, not in model input."""
from __future__ import annotations

import json
import os
import re
import struct
import time
import zlib

NONCE = re.compile(r"[A-Za-z0-9.-]{1,64}\Z")


class ReloadError(Exception):
    pass


def marker(payload, nonce):
    if not isinstance(payload, str) or len(payload.encode("utf-8")) > 512:
        return None
    try:
        value = json.loads(payload)
    except (ValueError, TypeError):
        return None
    if not isinstance(value, dict) or value.get("v") != 1:
        return None
    if value.get("kind") != "reload" or value.get("run") != nonce or value.get("status") != "ready":
        return None
    if type(value.get("ts")) is not int or value["ts"] <= 0:
        return None
    if not all(isinstance(value.get(k), str) and 0 < len(value[k]) <= 64 for k in ("client", "build")):
        return None
    return value


def save_timeout_image(session, nonce, roi):
    if roi is None:
        return None
    # WGC's bounded ROI is BGR(A); write RGB PNG with the stdlib, no new dependency.
    height, width = roi.shape[:2]
    raw = b"".join(b"\0" + roi[y, :, :3][:, ::-1].tobytes() for y in range(height))
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xffffffff)
    png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b"")
    directory = session.state_path(os.path.join("reload", nonce))
    os.makedirs(directory, exist_ok=True)
    path = os.path.join(directory, "timeout.png")
    with open(path, "wb") as handle:
        handle.write(png)
    return path


def wait_ready(windows, session, args, nonce):
    deadline = time.monotonic() + args.timeout
    interval = getattr(args, "interval", None) or .1
    last_roi = None
    with windows.open_notice_capture(args.hwnd, pid=args.pid, exe_path=args.exe_path, interval=interval) as capture:
        while time.monotonic() < deadline:
            if capture.window_closed:
                break
            roi = capture.latest_roi()
            if roi is not None:
                last_roi = roi
                for payload in windows.decode_qr(roi):
                    ready = marker(payload, nonce)
                    if ready:
                        return ready
            time.sleep(interval)
    path = save_timeout_image(session, nonce, last_roi)
    session.log_event("reload_ready_timeout", nonce=nonce, screenshot=path)
    raise ReloadError(f"reload readiness unresolved for {nonce}; resume this nonce without reloading; screenshot={path}")


def wait_cleared(windows, args, nonce):
    deadline = time.monotonic() + min(args.timeout, 5)
    interval = getattr(args, "interval", None) or .1
    absent = 0
    with windows.open_notice_capture(args.hwnd, pid=args.pid, exe_path=args.exe_path, interval=interval) as capture:
        while time.monotonic() < deadline:
            if capture.window_closed:
                return False
            roi = capture.latest_roi()
            if roi is not None:
                visible = any(marker(payload, nonce) for payload in windows.decode_qr(roi))
                absent = 0 if visible else absent + 1
                if absent >= 2:
                    return True
            time.sleep(interval)
    return False


def execute(windows, session, args, deliver, nonce, *, ticket=None, resume=False):
    if not NONCE.fullmatch(nonce):
        raise ReloadError("invalid reload nonce")
    history = [e for e in session.read_events() if e.get("nonce") == nonce]
    previous = next((e for e in history if e["event"] == "reload_handshake_requested"), None)
    if resume:
        if not previous or any(previous.get(k) != getattr(args, attr, None)
                               for k, attr in (("hwnd", "hwnd"), ("pid", "pid"), ("exePath", "exe_path"))):
            raise ReloadError("reload resume requires the original nonce and window binding")
        if any(e["event"] == "reload_ready_cleared" for e in history):
            return {"status": "already_completed", "nonce": nonce}
    else:
        if previous:
            raise ReloadError("reload nonce was already submitted; use --resume, do not reload again")
        session.log_event("reload_handshake_requested", nonce=nonce, ticket=ticket,
                          hwnd=args.hwnd, pid=args.pid, exePath=args.exe_path)
        deliver(session, windows, args, f"/dev auto reload {nonce}" + (f" {ticket}" if ticket else ""))
    started = time.monotonic()
    confirmed = next((e for e in history if e["event"] == "reload_ready_confirmed"), None)
    ready = confirmed.get("ready") if confirmed else wait_ready(windows, session, args, nonce)
    if not confirmed:
        session.log_event("reload_ready_confirmed", nonce=nonce, ready=ready)
    deliver(session, windows, args, f"/dev auto unidentify {nonce}")
    if not wait_cleared(windows, args, nonce):
        session.log_event("reload_ready_cleanup_unresolved", nonce=nonce)
        raise ReloadError(f"reload ready, but marker cleanup unresolved; resume {nonce}")
    session.log_event("reload_ready_cleared", nonce=nonce)
    return {"status": "reload_ready", "nonce": nonce, "elapsedSeconds": round(time.monotonic() - started, 2),
            "client": ready["client"], "build": ready["build"], "markerCleared": True}
