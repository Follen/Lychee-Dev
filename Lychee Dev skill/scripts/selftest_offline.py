# -*- coding: utf-8 -*-
"""Offline self test for the Lychee Dev automation host tooling.

Covers the registry writer, the restricted SavedVariables reader, the session
log/dedup, artifact byte fidelity, notice validation/identity, the bounded read
retry and the CLI exit behaviour. The real packages (windows-capture,
zxing-cpp) are checked only when they are importable; everything else runs with
no third-party dependency.
"""

import argparse
import contextlib
import ctypes
import hashlib
import importlib.util
import io
import json
import os
import shutil
import sys
import tempfile
import time
import zlib

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from automation import registry as reg  # noqa: E402
from automation import saved_variables as sv  # noqa: E402
from automation import session as ses  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
SKILL_ROOT = os.path.dirname(HERE)
WORKTREE = os.path.dirname(SKILL_ROOT)

BS = chr(92)


def _load_cli():
    """Load automation.py under its own name (a package shares the name)."""

    path = os.path.join(HERE, "automation.py")
    spec = importlib.util.spec_from_file_location("lychee_automation_cli", path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


cli = _load_cli()


def expect_failure(label, callback, errors):
    try:
        callback()
    except errors:
        return
    raise SystemExit("expected failure was not raised: " + label)


report = {
    "schema": "lychee.automation.result.v1",
    "taskId": "inspect-example",
    "executionId": "req-20260912-000001-aaaaaa",
    "revision": "rev-1",
    "kind": "lua",
    "status": "succeeded",
    "startedAt": 1789207190,
    "finishedAt": 1789207200,
    "requestType": "task",
    "stdout": "line1\nline2\ttab \"quoted\" back" + BS + "slash unicode caf\u00e9 control \u0001",
    "returns": [{"ok": True, "nested": [1, 2.5, None]}],
    "error": None,
    "lifecycleLog": [],
    "lifecycleLogLimited": False,
    "bugSnapshot": None,
    "environment": {"clientId": "retail", "character": "Follen", "realm": "TestRealm"},
    "complete": True,
    "incompleteReasons": [],
}
content = json.dumps(report, ensure_ascii=False, separators=(",", ":"))
checksum = format(zlib.adler32(content.encode("utf-8")) & 0xFFFFFFFF, "08x")


def lua_q(s):
    out = ['"']
    for ch in s:
        if ch == '"':
            out.append(BS + '"')
        elif ch == BS:
            out.append(BS + BS)
        elif ch == "\n":
            out.append(BS + "n")
        elif ch == "\r":
            out.append(BS + "r")
        elif ch == "\t":
            out.append(BS + "t")
        elif ord(ch) < 32:
            out.append(BS + "%03d" % ord(ch))
        else:
            out.append(ch)
    out.append('"')
    return "".join(out)


ticket = "LYCHEE-20260912-180000-0001"
content_bytes = content.encode("utf-8")
sv_text = (
    "LycheeDevDB = {\n"
    '\t["schemaVersion"] = 8,\n'
    '\t["history"] = {\n\t},\n'
    '\t["exports"] = {\n'
    '\t\t["version"] = 2,\n'
    '\t\t["nextId"] = 1,\n'
    "\t\t[\"totalBytes\"] = %d,\n"
    '\t\t["order"] = {\n\t\t\t"%s",\n\t\t},\n'
    '\t\t["records"] = {\n'
    '\t\t\t["%s"] = {\n'
    '\t\t\t\t["schema"] = "lychee.evidence.v1",\n'
    '\t\t\t\t["ticket"] = "%s",\n'
    "\t\t\t\t[\"createdAt\"] = 1789207200,\n"
    '\t\t\t\t["source"] = {\n\t\t\t\t\t["kind"] = "automation_result",\n\t\t\t\t\t["title"] = "inspect-example",\n\t\t\t\t},\n'
    '\t\t\t\t["payload"] = {\n'
    '\t\t\t\t\t["mediaType"] = "application/json",\n'
    '\t\t\t\t\t["encoding"] = "utf-8",\n'
    '\t\t\t\t\t["content"] = %s,\n'
    "\t\t\t\t\t[\"byteCount\"] = %d,\n"
    "\t\t\t\t},\n"
    '\t\t\t\t["metadata"] = {\n'
    '\t\t\t\t\t["taskId"] = "inspect-example",\n'
    '\t\t\t\t\t["executionId"] = "req-20260912-000001-aaaaaa",\n'
    '\t\t\t\t\t["revision"] = "rev-1",\n'
    '\t\t\t\t\t["resultSchema"] = "lychee.automation.result.v1",\n'
    '\t\t\t\t\t["status"] = "succeeded",\n'
    '\t\t\t\t\t["complete"] = true,\n'
    '\t\t\t\t\t["checksumAlgorithm"] = "adler32",\n'
    '\t\t\t\t\t["contentChecksum"] = "%s",\n'
    "\t\t\t\t},\n"
    "\t\t\t},\n"
    "\t\t},\n"
    "\t},\n"
    '\t["automation"] = {\n\t\t["version"] = 1,\n\t},\n'
    "}\n"
) % (len(content_bytes), ticket, ticket, ticket, lua_q(content),
     len(content_bytes), checksum)

tmp = tempfile.mkdtemp()
# The harness reuses one scratch directory per case, so drop stale state first.
shutil.rmtree(tmp, ignore_errors=True)
os.makedirs(tmp, exist_ok=True)
path = os.path.join(tmp, "Lychee Dev.lua")
with open(path, "wb") as f:
    f.write(sv_text.encode("utf-8"))

db = sv.load_database(path)
assert db["schemaVersion"] == 8
record = sv.get_record(db, ticket)
failures = sv.verify_evidence(
    record, ticket=ticket, created_at=1789207200, task_id="inspect-example",
    execution_id="req-20260912-000001-aaaaaa", revision="rev-1")
assert failures == [], failures
inner = sv.parse_inner_report(record["payload"]["content"])
assert inner["status"] == "succeeded" and inner["complete"] is True
assert inner["stdout"].startswith("line1\nline2\t")
assert inner["returns"][0]["nested"] == [1, 2.5, None]

for kwargs in ({"task_id": "other"}, {"execution_id": "wrong"},
               {"revision": "wrong"}, {"created_at": 1}):
    assert sv.verify_evidence(record, ticket=ticket, **kwargs), kwargs

corrupt = dict(record)
corrupt["payload"] = dict(record["payload"])
corrupt["payload"]["content"] = record["payload"]["content"][:-1]
assert sv.verify_evidence(corrupt, ticket=ticket), "checksum mismatch not detected"

try:
    sv.get_record(db, "LYCHEE-DOES-NOT-EXIST")
    raise SystemExit("missing ticket not rejected")
except sv.SavedVariablesError:
    pass
try:
    sv.parse_lua_assignments("x = (function() end)")
    raise SystemExit("expression accepted")
except sv.SavedVariablesError:
    pass
try:
    sv.parse_lua_assignments('x = os.execute("evil")')
    raise SystemExit("call accepted")
except sv.SavedVariablesError:
    pass
try:
    sv.parse_lua_assignments('x = "unterminated')
    raise SystemExit("unterminated string accepted")
except sv.SavedVariablesError:
    pass
try:
    sv.parse_lua_assignments('x = "bad ' + BS + 'q escape"')
    raise SystemExit("bad escape accepted")
except sv.SavedVariablesError:
    pass

print("saved_variables base reader OK")

# --- real client output: explicit nil for an absent global --------------------
#
# The client writes `DumperDB = nil` for a declared SavedVariables global that
# does not exist, and a Lua table can carry `["key"] = nil`. Both mean absent.

nil_doc = sv.parse_lua_assignments(
    'LycheeDevDB = { ["a"] = 1, ["b"] = nil }\nDumperDB = nil\n')
assert nil_doc == {"LycheeDevDB": {"a": 1}}, nil_doc

# The exact trailer shape a real client file ends with.
real_tail = 'LycheeDevDB = { ["schemaVersion"] = 8 }\nDumperDB = nil\n'
assert sv.parse_lua_assignments(real_tail) == {"LycheeDevDB": {"schemaVersion": 8}}

# nil must not weaken the data-only rule.
try:
    sv.parse_lua_assignments('x = os.execute("evil")')
    raise SystemExit("call accepted after nil support")
except sv.SavedVariablesError:
    pass
print("client nil assignments (global and table key) OK")

# --- Lua \ddd escapes take one to three digits --------------------------------

assert sv.parse_lua_assignments('x = "' + BS + '12x"')["x"] == chr(12) + "x"
assert sv.parse_lua_assignments('x = "' + BS + '65bc"')["x"] == "Abc"
assert sv.parse_lua_assignments('x = "' + BS + '065"')["x"] == "A"
assert sv.parse_lua_assignments('x = "' + BS + '0"')["x"] == chr(0)
assert sv.parse_lua_assignments('x = "' + BS + '9"')["x"] == chr(9)
assert sv.parse_lua_assignments('x = "' + BS + 'z \n\t y"')["x"] == "y"
utf8_text = "\u00e9\u4e2d"
utf8_escaped = "".join(BS + "%03d" % byte for byte in utf8_text.encode("utf-8"))
assert sv.parse_lua_assignments('x = "' + utf8_escaped + '"')["x"] == utf8_text
for bad_escape in (BS + "256", BS + "999", BS + "300"):
    expect_failure("decimal escape " + bad_escape,
                   lambda bad=bad_escape: sv.parse_lua_assignments('x = "' + bad + '"'),
                   sv.SavedVariablesError)
assert reg.unescape_lua_string('"' + BS + '12x"') == chr(12) + "x"
assert reg.unescape_lua_string('"' + BS + '65bc"') == "Abc"
print("lua decimal escapes (1-3 digits, <=255) OK")

# --- payload / metadata validation is no longer optional ----------------------

def _mutated(mutate):
    copy = json.loads(json.dumps(record))
    mutate(copy)
    return copy


def _fails(mutate, needle):
    reasons = sv.verify_evidence(_mutated(mutate), ticket=ticket)
    assert reasons, "mutation was accepted: " + needle
    assert any(needle in reason for reason in reasons), (needle, reasons)


def _drop(field):
    def mutate(copy):
        copy["payload"].pop(field, None)
    return mutate


def _drop_meta(field):
    def mutate(copy):
        copy["metadata"].pop(field, None)
    return mutate


_fails(_drop("mediaType"), "mediaType")
_fails(lambda copy: copy["payload"].__setitem__("mediaType", "text/plain"), "mediaType")
_fails(_drop("encoding"), "encoding")
_fails(lambda copy: copy["payload"].__setitem__("encoding", "utf-16"), "encoding")
_fails(_drop("byteCount"), "byteCount")
_fails(lambda copy: copy["payload"].__setitem__("byteCount", 1), "byteCount")
_fails(_drop_meta("checksumAlgorithm"), "checksumAlgorithm")
_fails(lambda copy: copy["metadata"].__setitem__("checksumAlgorithm", "sha256"),
       "checksumAlgorithm")
_fails(_drop_meta("contentChecksum"), "contentChecksum")
_fails(lambda copy: copy["metadata"].__setitem__("contentChecksum", "zzzz"), "contentChecksum")
_fails(_drop_meta("resultSchema"), "resultSchema")
_fails(_drop_meta("status"), "status")
_fails(lambda copy: copy["metadata"].__setitem__("status", "running"), "status")
_fails(_drop_meta("complete"), "complete")
_fails(lambda copy: copy["metadata"].__setitem__("complete", "yes"), "complete")


def _oversize(copy):
    big = "a" * (sv.MAX_REPORT_BYTES + 1)
    copy["payload"]["content"] = big
    copy["payload"]["byteCount"] = len(big.encode("utf-8"))
    copy["metadata"]["contentChecksum"] = format(
        zlib.adler32(big.encode("utf-8")) & 0xFFFFFFFF, "08x")


_fails(_oversize, "report limit")
print("payload mediaType/encoding/byteCount/checksum/limit checks OK")
print("metadata resultSchema/status/complete/checksumAlgorithm checks OK")

# --- inner report checks -------------------------------------------------------

def _inner_fails(mutate, needle, base=None, **kwargs):
    body = json.loads(json.dumps(report if base is None else base))
    mutate(body)
    try:
        sv.parse_inner_report(json.dumps(body), **kwargs)
    except sv.SavedVariablesError as error:
        assert needle in str(error), (needle, str(error))
        return
    raise SystemExit("inner report mutation was accepted: " + needle)


_inner_fails(lambda body: body.__setitem__("schema", "other"), "schema")
_inner_fails(lambda body: body.__setitem__("status", "running"), "status")
_inner_fails(lambda body: body.__setitem__("complete", "yes"), "complete")
_inner_fails(lambda body: body.__setitem__("environment", None), "environment")
_inner_fails(lambda body: body.pop("requestType"), "requestType")
_inner_fails(lambda body: body.__setitem__("requestType", "other"), "requestType")


def _mark_incomplete(body):
    body["complete"] = False
    body["incompleteReasons"] = None


_inner_fails(_mark_incomplete, "incompleteReasons")
_inner_fails(lambda body: body.__setitem__("taskId", "other"), "taskId",
             task_id="inspect-example")

bug_body = json.loads(json.dumps(report))
bug_body["requestType"] = "bug"
bug_body["params"] = {"count": 3, "scope": "provider_storage"}
sv.parse_inner_report(json.dumps(bug_body), request_type="bug")
_inner_fails(lambda body: body.__setitem__("params", None), "params",
             base=bug_body, request_type="bug")
_inner_fails(lambda body: body.__setitem__("params", {"scope": "x"}), "count",
             base=bug_body, request_type="bug")
_inner_fails(lambda body: body.__setitem__("params", {"count": 3}), "scope",
             base=bug_body, request_type="bug")
bug_meta = json.loads(json.dumps(record["metadata"]))
bug_meta["status"] = "succeeded"
assert sv.parse_inner_report(json.dumps(bug_body), request_type="bug",
                             metadata=bug_meta)["requestType"] == "bug"
tampered = dict(bug_meta, status="cancelled")
expect_failure(
    "inner/metadata status disagreement",
    lambda: sv.parse_inner_report(json.dumps(bug_body), request_type="bug",
                                  metadata=tampered),
    sv.SavedVariablesError)
print("inner report version/status/complete/environment/request checks OK")

# --- session artifact byte fidelity and log scope -----------------------------

data = os.path.join(tmp, "data")
os.makedirs(data, exist_ok=True)
s = ses.Session(data_dir=data, installation="retail-main")

artifacts_payload = 'line1\nline2\ncaf\u00e9\r\nend\n'
one = "LYCHEE-20260912-180000-0002"
artifact = s.save_artifact(one, "content.json", artifacts_payload)
raw = open(artifact, "rb").read()
expected = artifacts_payload.encode("utf-8")
assert raw == expected, (len(raw), len(expected))
assert b"\r\n" in raw, "the CRLF that was in the content must survive"
assert raw.count(b"\n") - raw.count(b"\r\n") == 3, "LF endings must not be rewritten"
sha256 = hashlib.sha256(raw).hexdigest()
s.log_event("received", ticket=one, byteCount=len(raw), sha256=sha256)
received = s.read_events("received")[-1]
assert received["byteCount"] == os.path.getsize(artifact) == len(expected)
assert received["sha256"] == hashlib.sha256(open(artifact, "rb").read()).hexdigest()
print("artifact byte fidelity (recorded byteCount/sha256 == on-disk bytes) OK")

other = ses.Session(data_dir=data, installation="second-client")
other.log_event("received", ticket="LYCHEE-OTHER")
assert [r["ticket"] for r in s.read_events("received")] == [one], "scope leak"
assert len(s.read_events("received", installation=ses.ALL_INSTALLATIONS)) == 2
assert [r["ticket"] for r in other.read_events("received")] == ["LYCHEE-OTHER"]
print("host log installation scope OK")

assert not s.reload_already_requested("req-1", ticket)
assert s.mark_reload_requested("req-1", ticket)
assert not s.mark_reload_requested("req-1", ticket), "dedup failed"
assert s.reload_already_requested("req-1", ticket)
assert s.mark_reload_requested("req-1", ticket + "-other")
assert not s.reload_already_requested("req-2", ticket)
assert not s.wait_for_saved_variables(path, timeout=0.1)
baseline = s.file_snapshot(path)
os.utime(path, (time.time() + 5, time.time() + 5))
assert s.wait_for_saved_variables(path, timeout=2, baseline=baseline)
print("reload dedup + bounded SV wait OK")

# --- bounded SV read retry -----------------------------------------------------

def _succeed_after(failures_to_raise):
    state = {"calls": 0}

    def loader():
        state["calls"] += 1
        if state["calls"] <= failures_to_raise:
            raise sv.SavedVariablesError("torn file")
        return "value"
    return loader, state


loader, state = _succeed_after(2)
slept = []
value, error, used = cli.retry_read(loader, attempts=4, delay=0.01,
                                    sleep=slept.append)
assert (value, error, used) == ("value", None, 3), (value, error, used)
assert state["calls"] == 3 and slept == [0.01, 0.01], (state, slept)

loader, state = _succeed_after(99)
value, error, used = cli.retry_read(loader, attempts=3, delay=0.01,
                                    sleep=slept.append)
assert value is None and isinstance(error, sv.SavedVariablesError)
assert used == 3 and state["calls"] == 3, (used, state)
assert len(slept) == 4, slept
print("bounded SV read retry OK")

# --- CLI exit behaviour and defaults ------------------------------------------

parser = cli.build_parser()
capture_args = parser.parse_args(["capture", "--hwnd", "0x10"])
assert capture_args.timeout == 120.0 and capture_args.interval == 0.1
sv_defaults = parser.parse_args(["sv", "read", "--sv", path, "--ticket", ticket])
assert sv_defaults.retries == 3 and sv_defaults.retry_delay == 0.5
subparsers = next(action for action in parser._actions
                  if isinstance(action, argparse._SubParsersAction))
capture_help = subparsers.choices["capture"].format_help()
assert "--timeout" in capture_help and "seconds" in capture_help
assert "--interval" in capture_help
run_help = subparsers.choices["run"].format_help()
assert "--timeout" in run_help and "--interval" in run_help and "--sv" in run_help
send_help = subparsers.choices["send"].format_help()
assert "--request-id" in send_help and "--ticket" in send_help
assert cli.main(["task", "upsert", "--install-dir", tmp, "--id", "x"]) == 1
assert cli.main(["sv", "read", "--sv", os.path.join(tmp, "missing.lua"),
                 "--ticket", ticket]) == 1
assert cli.main(["sv", "find"]) == 1
assert cli.main(["profile", "set", "--name", "p", "--wow-root", tmp,
                 "--addon-dir", tmp]) == 1
assert cli.main(["sv", "read", "--sv", path]) == 1
print("CLI explicit errors (exit 1, no traceback) OK")

# --- exit-code contract: window/input failures must map to 4, not 1 ------------
#
# Regression: a dead window used to exit 1 from capture/run/bugs while `send`
# exited 4, and a missing optional dependency was reported as invalid usage.

# automation.windows is imported lazily by the CLI, so import it explicitly for
# the exception classes; the package module is the same one the CLI resolves.
try:
    from automation import windows as windows_mod
except Exception:  # pragma: no cover - non-Windows hosts
    windows_mod = None

if windows_mod is None:  # pragma: no cover - non-Windows hosts
    print("CLI window/input exit codes (skipped: no windows module)")
else:
    class _DeadWindowWindows:
        def __init__(self, error):
            self.error = error

        def open_notice_capture(self, *_args, **_kwargs):
            raise self.error

    real_window_module = cli._window_module
    try:
        for label, error, expected in (
            ("window identity", windows_mod.WindowIdentityError("dead window"), 4),
            ("missing dependency",
             windows_mod.CaptureDependencyError("no zxing-cpp"), 4),
            ("plain usage error", ValueError("bad argument"), 1),
        ):
            cli._window_module = lambda _error=error: _DeadWindowWindows(_error)
            code = cli.main(["--data-dir", tmp, "capture", "--hwnd", "0x10",
                             "--timeout", "1"])
            assert code == expected, f"{label} exited {code}, expected {expected}"
    finally:
        cli._window_module = real_window_module
    print("CLI window/input exit codes (4 for window/dependency, 1 otherwise) OK")

# --- command delivery order: open chat, paste, submit ------------------------
#
# Regression: the sequence used to paste BEFORE opening the chat box, so the
# client opened an empty chat and no command ran. Reference behaviour is
# Return -> Ctrl+V -> Return.

try:
    from automation import windows as winmod
except Exception:  # pragma: no cover - non-Windows hosts
    winmod = None

if winmod is None:  # pragma: no cover - non-Windows hosts
    print("command delivery order (skipped: no windows module)")
else:
    class _RecordingClipboard:
        def __init__(self):
            self.value = None
            self.writes = []

        def read_text(self):
            return self.value

        def copy_text(self, text):
            self.value = text
            self.writes.append(text)

        def clear(self):
            self.value = None

    pressed = []
    real_send_key = winmod._send_key
    real_set_fg = winmod.user32.SetForegroundWindow
    real_is_foreground = winmod.is_foreground
    real_confirm = winmod.confirm_window
    real_sleep = winmod.time.sleep
    try:
        def fake_send_key(vk, *, ctrl=False):
            pressed.append(("ctrl+v" if ctrl else "key") + ":%#x" % vk)

        winmod._send_key = fake_send_key
        winmod.user32.SetForegroundWindow = lambda _hwnd: 1
        winmod.is_foreground = lambda _hwnd: True
        winmod.confirm_window = lambda *a, **k: None
        winmod.time.sleep = lambda _seconds: None

        delivery_clipboard = _RecordingClipboard()
        winmod.send_command(1, "/reload", clipboard=delivery_clipboard)
        expected = ["key:0xd", "ctrl+v:0x56", "key:0xd"]
        assert pressed == expected, f"delivery order was {pressed}, expected {expected}"
        assert delivery_clipboard.writes[0] == "/reload", delivery_clipboard.writes
    finally:
        winmod._send_key = real_send_key
        winmod.user32.SetForegroundWindow = real_set_fg
        winmod.is_foreground = real_is_foreground
        winmod.confirm_window = real_confirm
        winmod.time.sleep = real_sleep
    print("command delivery order (Return, Ctrl+V, Return) OK")

    posted = []
    real_post = winmod.user32.PostMessageW
    real_sleep_messages = winmod.time.sleep
    try:
        winmod.user32.PostMessageW = (
            lambda hwnd, msg, wparam, lparam: posted.append((msg, wparam)) or 1)
        winmod.time.sleep = lambda _seconds: None
        winmod.send_command_messages(1, "/reload")
    finally:
        winmod.user32.PostMessageW = real_post
        winmod.time.sleep = real_sleep_messages

    chars = "".join(chr(w) for m, w in posted if m == winmod.WM_CHAR)
    assert chars == "/reload", chars
    down = [w for m, w in posted if m == winmod.WM_KEYDOWN]
    assert down == [winmod.VK_RETURN], down
    assert all(m in (winmod.WM_CHAR, winmod.WM_KEYDOWN, winmod.WM_KEYUP)
               for m, _w in posted), posted
    print("command delivery via posted messages (no foreground) OK")

    # The notice is drawn at the top-left corner of the game window, so the scan
    # area must cover that corner. A ROI that drifts back to another corner makes
    # the whole receipt path fail silently (capture finds no code, times out).
    top, left, bottom, right = winmod.NOTICE_ROI_FRACTIONS
    assert top == 0.0 and left == 0.0, (top, left)
    assert 0.0 < bottom <= 0.5 and 0.0 < right <= 0.5, (bottom, right)
    print("notice ROI covers the top-left corner OK")

    # Keep the Lua anchor and the Python ROI in sync: both corners must match.
    overlay = os.path.join(WORKTREE, "add-on", "UI", "AutomationOverlay.lua")
    if os.path.isfile(overlay):
        overlay_text = open(overlay, encoding="utf-8").read()
        assert 'noticeFrame:SetPoint("TOPLEFT", UIParent, "TOPLEFT"' in overlay_text, \
            "the Lua notice anchor is not TOPLEFT"
        assert "SetPoint(\"TOPRIGHT\", UIParent, \"TOPRIGHT\"" not in overlay_text, \
            "the Lua notice still anchors to TOPRIGHT while the ROI scans TOPLEFT"
        print("Lua notice anchor and Python ROI agree on the top-left corner OK")

# --- host read acknowledgement ------------------------------------------------
#
# The plugin cannot see whether the host read a result, so the host says so with
# an in-game chat command through the already working delivery channel.

assert cli.ack_command("LYCHEE-1", "received") == "/dev auto ack LYCHEE-1 received"
assert cli.ack_command("LYCHEE-1", "failed") == "/dev auto ack LYCHEE-1 failed"
for bad in ("ok", "RECEIVED", ""):
    try:
        cli.ack_command("LYCHEE-1", bad)
        raise SystemExit(f"ack accepted an invalid status: {bad!r}")
    except cli.CliError:
        pass
ack_help = cli.build_parser()._subparsers._group_actions[0].choices["ack"].format_help()
assert "--ticket" in ack_help and "--status" in ack_help
assert "received" in ack_help
print("ack command shape and status validation OK")

# --- notice validation and identity -------------------------------------------

def notice_json(**overrides):
    payload = {"v": 1, "ticket": ticket, "task": "inspect-example",
               "run": "req-20260912-000001-aaaaaa", "ts": 1789207200}
    payload.update(overrides)
    return json.dumps(payload)


notice = cli.validate_notice(notice_json())
assert cli.notice_identity_problems(notice, expected_task="inspect-example",
                                    expected_run=notice["run"]) == []
assert cli.notice_identity_problems(notice, expected_task="other",
                                    expected_run=notice["run"])
assert cli.notice_identity_problems(notice, expected_task=None,
                                    expected_run="stale-run")
for bad in (json.dumps({"v": 2, "ticket": ticket, "task": "t", "run": "r", "ts": 1}),
            json.dumps({"ticket": ticket, "task": "t", "run": "r", "ts": 1}),
            notice_json(ts=0),
            notice_json(ts=True),
            notice_json(ticket="nope"),
            notice_json(task="bad/id"),
            notice_json(run="bad run"),
            notice_json(ticket="L" * 65),
            "",
            "not json"):
    expect_failure("bad notice " + str(bad)[:24],
                   lambda payload=bad: cli.validate_notice(payload),
                   sv.SavedVariablesError)
print("notice validation + task/run identity checks OK")


class _FakeCapture:
    def __init__(self):
        self.window_closed = False
        self.polls = 0

    def __enter__(self):
        return self

    def __exit__(self, *_exc):
        return False

    def latest_roi(self):
        self.polls += 1
        return self if self.polls <= 2 else None


class _FakeWindows:
    def __init__(self, batches):
        self.batches = list(batches)
        self.opened = 0

    def open_notice_capture(self, hwnd, **kwargs):
        self.opened += 1
        return _FakeCapture()

    def decode_qr(self, roi):
        return self.batches.pop(0) if self.batches else []


poll_args = argparse.Namespace(hwnd=1, timeout=1.0, interval=0.001, pid=None,
                               exe_path=None)
stale = notice_json(task="older-task", run="older-run",
                    ticket="LYCHEE-20260912-170000-0009")
fresh = notice_json()
fake_session = ses.Session(data_dir=os.path.join(tmp, "poll"), installation="retry")
fake_session_dirs = fake_session.data_dir
os.makedirs(fake_session_dirs, exist_ok=True)
fake_windows = _FakeWindows([[stale], [fresh]])
resolved = cli.poll_notice(fake_windows, fake_session, poll_args,
                           expected_task="inspect-example",
                           expected_run="req-20260912-000001-aaaaaa")
assert resolved is not None and resolved["ticket"] == ticket, resolved
assert fake_windows.opened == 1, "one capture session must serve the whole poll"
events = [r["event"] for r in fake_session.read_events()]
assert "notice_identity_mismatch" in events and "notice_seen" in events, events

timeout_session = ses.Session(data_dir=os.path.join(tmp, "poll-timeout"),
                              installation="retry")
os.makedirs(timeout_session.data_dir, exist_ok=True)
timeout_args = argparse.Namespace(hwnd=1, timeout=0.2, interval=0.001, pid=None,
                                  exe_path=None)
started = time.monotonic()
assert cli.poll_notice(_FakeWindows([]), timeout_session, timeout_args,
                       expected_task="x") is None
assert 0.15 < time.monotonic() - started < 5.0
timeout_events = [r["event"] for r in timeout_session.read_events()]
assert "notice_seen" not in timeout_events
print("poll_notice: stale notice rejected, fresh accepted, timeout bounded OK")

# --- registry: schema/kind rejection, hash, region preservation ---------------

def registry_text(*blocks):
    return reg.REGISTRY_HEADER + "".join(blocks)


block = "\n".join(reg.build_block(
    "inspect-example", request_id="req-1", revision="rev-1",
    source='print("hi")', created_at=1)) + "\n"

expect_failure("schema 9 rejected",
               lambda: reg.parse_blocks(reg.REGISTRY_HEADER + block.replace(
                   "schema = 1,", "schema = 9,")),
               reg.RegistryError)
expect_failure("kind rejected",
               lambda: reg.parse_blocks(reg.REGISTRY_HEADER + block.replace(
                   'kind = "lua",', 'kind = "nope",')),
               reg.RegistryError)
expect_failure("unterminated block rejected",
               lambda: reg.parse_blocks(reg.REGISTRY_HEADER + block.replace(
                   reg.END_MARKER + "inspect-example\n", "")),
               reg.RegistryError)
expect_failure("unexpected block footer rejected",
               lambda: reg.parse_blocks(reg.REGISTRY_HEADER + block.replace(
                   "\n}\n" + reg.END_MARKER, "\n" + reg.END_MARKER)),
               reg.RegistryError)
assert reg.split_regions("head\nbody\n") == ("head\nbody\n", "")
assert reg.split_regions("pre\n" + block + "post\n") == ("pre\n", "post\n")

regdir = os.path.join(tmp, "registry")
os.makedirs(regdir, exist_ok=True)
task_path = os.path.join(regdir, "auto.lua")
prefix_note = "-- hand written note before the blocks\n"
suffix_note = "-- hand written trailer kept verbatim\n"
with open(task_path, "wb") as handle:
    handle.write((reg.REGISTRY_HEADER + prefix_note).encode("utf-8"))

task_registry = reg.TaskRegistry(task_path)
assert task_registry.path == reg.resolve_registry_path(task_path)
definition = task_registry.upsert(
    "inspect-example", request_id="req-1", revision="rev-1",
    source='print("caf' + "\u00e9" + '")' + "\n", created_at=1)
assert prefix_note in open(task_path, "r", encoding="ascii").read()
disk = open(task_path, "rb").read()
assert definition["sourceBytes"] == len(
    ('print("caf' + "\u00e9" + '")' + "\n").encode("utf-8"))
assert definition["fileHash"] == hashlib.sha256(disk).hexdigest()
assert all(byte < 128 for byte in disk), "registry must stay ASCII"
assert b"\r" not in disk
text = disk.decode("ascii")
assert prefix_note in text, "content before the blocks lost"
assert task_registry.read()["inspect-example"]["source"] == \
    'print("caf' + "\u00e9" + '")' + "\n"
assert "\\195\\169" in text, "non-ASCII source must be escaped as \\ddd"

# A human trailer after the blocks must survive every later write.
with open(task_path, "a", encoding="utf-8") as handle:
    handle.write(suffix_note)
second = task_registry.upsert("second-task", request_id="req-2", revision="rev-2",
                              source="return 1", created_at=2)
assert second["fileHash"] == hashlib.sha256(open(task_path, "rb").read()).hexdigest()
text = open(task_path, "r", encoding="ascii").read()
assert prefix_note in text and suffix_note in text, "content outside blocks lost"
assert text.startswith(reg.REGISTRY_HEADER + prefix_note)
assert text.endswith(suffix_note), "trailer after the blocks was lost"
assert list(task_registry.read()) == ["inspect-example", "second-task"]
task_registry.upsert("second-task", request_id="req-3", revision="rev-3",
                     source="return 2", created_at=3)
definitions = task_registry.read()
assert definitions["second-task"]["revision"] == "rev-3"
assert definitions["inspect-example"]["revision"] == "rev-1"
assert prefix_note in open(task_path, "r", encoding="ascii").read()
expect_failure(
    "revision conflict",
    lambda: task_registry.upsert("second-task", request_id="req-4",
                                 revision="rev-4", source="return 3",
                                 created_at=4, expect_revision="rev-1"),
    reg.RegistryError)
assert task_registry.remove("second-task")
text_after_remove = open(task_path, "r", encoding="ascii").read()
assert prefix_note in text_after_remove and suffix_note in text_after_remove
assert list(task_registry.read()) == ["inspect-example"]
assert not task_registry.remove("second-task")
print("registry schema/kind rejection, fileHash, region preservation OK")

link_path = os.path.join(tmp, "registry-link")
linked = False
try:
    os.symlink(regdir, link_path, target_is_directory=True)
    linked = True
except (OSError, NotImplementedError, AttributeError):
    pass
if not linked:
    # Junctions need no elevated privilege; used as the Windows fallback.
    linked = (os.system('cmd /c mklink /J "' + link_path + '" "' + regdir
                        + '" >nul 2>nul') == 0 and os.path.isdir(link_path))
if linked:
    resolved = reg.TaskRegistry(os.path.join(link_path, "auto.lua"))
    assert resolved.path == task_registry.path, (resolved.path, task_registry.path)
    assert resolved.read() == task_registry.read()
    print("symlink/junction path resolution OK")
else:
    print("symlink/junction resolution check skipped (no link support)")

# --- task upsert through the CLI (non-ASCII source, byte-exact sourceBytes) ---

cli_dir = os.path.join(tmp, "cli-install")
auto_dir = os.path.join(cli_dir, "Modules", "Automation", "auto")
os.makedirs(auto_dir, exist_ok=True)
source_file = os.path.join(tmp, "probe.lua")
source_text = 'print("caf\u00e9")\r\nreturn { ok = true }\n'
with open(source_file, "wb") as handle:
    handle.write(source_text.encode("utf-8"))
auto_path = os.path.join(auto_dir, "auto.lua")
code = cli.main(["task", "upsert", "--install-dir", cli_dir, "--id", "probe-one",
                 "--source-file", source_file, "--request-id", "req-cli-1",
                 "--revision", "rev-cli-1"])
assert code == 0, code
cli_registry = reg.TaskRegistry(auto_path)
cli_definition = cli_registry.read()["probe-one"]
assert cli_definition["source"] == source_text, "CRLF source bytes changed"
assert cli_definition["sourceBytes"] == len(source_text.encode("utf-8"))
assert all(byte < 128 for byte in open(auto_path, "rb").read())
assert cli.main(["task", "list", "--install-dir", cli_dir]) == 0
assert cli.main(["task", "remove", "--install-dir", cli_dir, "--id", "probe-one"]) == 0
assert cli_registry.read() == {}
print("CLI task upsert keeps non-ASCII/CRLF source byte-exact OK")

# --- profile set + sv find resolve the client folder --------------------------

wow_root = os.path.join(tmp, "wow")
account_dir = os.path.join(wow_root, "_retail_", "WTF", "Account", "TESTACCOUNT",
                           "SavedVariables")
os.makedirs(account_dir, exist_ok=True)
expected_sv = os.path.join(account_dir, "Lychee Dev.lua")
with open(expected_sv, "wb") as handle:
    handle.write(b"LycheeDevDB = {}\n")
addon_install = os.path.join(tmp, "addon-install")
os.makedirs(addon_install, exist_ok=True)
profile_data = os.path.join(tmp, "profile-data")
assert cli.main(["--data-dir", profile_data, "profile", "set", "--name", "retail-main",
                 "--client", "retail", "--wow-root", wow_root,
                 "--addon-dir", addon_install, "--sv-path", expected_sv]) == 0
out = io.StringIO()
with contextlib.redirect_stdout(out):
    assert cli.main(["--data-dir", profile_data, "sv", "find",
                     "--profile", "retail-main"]) == 0
assert out.getvalue().strip() == expected_sv, out.getvalue()

# A profile may point straight at the client folder instead of the WoW root.
with contextlib.redirect_stdout(out):
    assert cli.main(["--data-dir", profile_data, "profile", "set", "--name", "direct",
                     "--client", "retail",
                     "--wow-root", os.path.join(wow_root, "_retail_"),
                     "--addon-dir", addon_install]) == 0
with contextlib.redirect_stdout(out):
    assert cli.main(["--data-dir", profile_data, "sv", "find", "--profile", "direct"]) == 0
assert expected_sv in out.getvalue()
assert cli.main(["--data-dir", profile_data, "sv", "find", "--profile", "missing"]) == 1
print("profile set + sv find resolve the client folder OK")

# --- send: host log first, reload dedup, explicit input failure ---------------

send_data = os.path.join(tmp, "send-data")


class _FakeSendWindows:
    class WindowIdentityError(Exception):
        pass

    class InputError(Exception):
        pass

    sent = []
    modes = []

    @classmethod
    def send_command(cls, hwnd, text, pid=None, exe_path=None, mode="foreground"):
        cls.sent.append((hwnd, text))
        cls.modes.append(mode)


class _FailingSendWindows:
    class WindowIdentityError(Exception):
        pass

    class InputError(Exception):
        pass

    @staticmethod
    def send_command(hwnd, text, pid=None, exe_path=None, mode="foreground"):
        raise _FailingSendWindows.WindowIdentityError("window identity changed")


original_window_module = cli._window_module
try:
    cli._window_module = lambda: _FakeSendWindows
    reload_call = ["--data-dir", send_data, "--installation", "send-scope", "send",
                   "--hwnd", "0x1", "--text", "/reload",
                   "--request-id", "req-send-1", "--ticket", "LYCHEE-SEND-1"]
    assert cli.main(reload_call) == 0
    assert cli.main(reload_call) == 1, "second reload for one notice must be refused"
    assert cli.main(["--data-dir", send_data, "send", "--hwnd", "0x1",
                     "--text", "/dev auto status x"]) == 0
    scoped = [r["event"] for r in
              ses.Session(data_dir=send_data, installation="send-scope").read_events()]
    assert scoped.count("reload_requested") == 1, scoped
    assert scoped.count("command_sent") == 1, scoped
    every = ses.Session(data_dir=send_data).read_events(
        installation=ses.ALL_INSTALLATIONS)
    assert [r["event"] for r in every].count("command_sent") == 2
    assert _FakeSendWindows.sent == [(1, "/reload"), (1, "/dev auto status x")]
    assert _FakeSendWindows.modes == ["foreground", "foreground"], _FakeSendWindows.modes
    assert cli.main(["--data-dir", send_data, "send", "--hwnd", "0x1",
                     "--mode", "messages", "--text", "hi"]) == 0
    assert _FakeSendWindows.modes[-1] == "messages", _FakeSendWindows.modes

    cli._window_module = lambda: _FailingSendWindows
    assert cli.main(["--data-dir", send_data, "send", "--hwnd", "0x1",
                     "--text", "/dev"]) == 4
    failed = [r["event"] for r in ses.Session(data_dir=send_data).read_events(
        installation=ses.ALL_INSTALLATIONS)]
    assert "command_failed" in failed, failed
finally:
    cli._window_module = original_window_module
print("send host log + reload dedup + input failure exit OK")

# --- packaging metadata -------------------------------------------------------

requirements = os.path.join(HERE, "requirements.txt")
assert os.path.isfile(requirements), requirements
requirements_text = open(requirements, "r", encoding="utf-8").read()
for pinned in ("zxing-cpp==3.1.1", "windows-capture==2.0.1", "numpy==2.5.3"):
    assert pinned in requirements_text, pinned
gitignore = open(os.path.join(WORKTREE, ".gitignore"), "r", encoding="utf-8").read()
assert "__pycache__/" in gitignore and "*.pyc" in gitignore
print("requirements.txt pins + .gitignore pycache entries OK")

# --- real optional dependencies (only when importable) ------------------------

try:
    import inspect
    import numpy as np
    import windows_capture
    import zxingcpp
    from automation import windows as win
except ImportError as error:
    print("optional live-window dependency checks skipped: " + str(error))
else:
    assert callable(getattr(zxingcpp, "read_barcodes", None))
    assert not hasattr(zxingcpp, "read_bars")
    assert callable(getattr(windows_capture.WindowsCapture, "on_frame_arrived", None))
    assert callable(getattr(windows_capture.WindowsCapture, "on_closed", None))
    assert callable(getattr(windows_capture.WindowsCapture, "start_free_threaded", None))
    assert callable(getattr(windows_capture.Frame, "convert_to_bgr", None))
    assert callable(getattr(windows_capture.Frame, "crop", None))
    capture_parameters = inspect.signature(
        windows_capture.WindowsCapture.__init__).parameters
    assert "window_hwnd" in capture_parameters
    assert "minimum_update_interval" in capture_parameters
    assert win.INPUT_SIZE == (40 if ctypes.sizeof(ctypes.c_void_p) == 8 else 28)
    assert win.user32.GetClipboardData.restype is ctypes.c_void_p
    assert win.kernel32.GlobalLock.restype is ctypes.c_void_p
    assert win.kernel32.OpenProcess.restype is ctypes.c_void_p
    assert win.user32.SendInput.argtypes[1] == ctypes.POINTER(win.INPUT)
    barcode = zxingcpp.create_barcode(notice_json(), zxingcpp.BarcodeFormat.QRCode)
    qr_image = np.asarray(zxingcpp.write_barcode_to_image(barcode))
    assert win.decode_qr(qr_image, zxingcpp) == [notice_json()]
    assert win.decode_qr(qr_image, None) == [notice_json()]

    class _FakeCaptureObject:
        frame_handler = None
        closed_handler = None

    # The real Frame class drives the exact ROI path used on live frames.
    real_frame = windows_capture.Frame(np.zeros((20, 20, 4), dtype=np.uint8), 20, 20, 0)
    capture = win.NoticeCapture(_FakeCaptureObject(), np, hwnd=0)
    capture._on_frame(real_frame)
    roi = capture.latest_roi()
    assert capture.last_frame_error is None, capture.last_frame_error
    # Derive the expected ROI from the configured fractions instead of freezing
    # one shape: the corner the notice uses has changed once already.
    expect_top, expect_left, expect_bottom, expect_right = win.NOTICE_ROI_FRACTIONS
    expect_h = int(20 * expect_bottom) - int(20 * expect_top)
    expect_w = int(20 * expect_right) - int(20 * expect_left)
    assert roi is not None and roi.shape == (expect_h, expect_w, 3), (
        roi.shape, (expect_h, expect_w, 3))
    assert capture.window_closed is False
    capture._on_closed()
    assert capture.window_closed is True
    assert "read_bars(" not in open(win.__file__, "r", encoding="utf-8").read()
    print("real zxing-cpp/windows-capture API checks OK")

print("saved_variables + session OK")


