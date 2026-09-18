#!/usr/bin/env python3
"""Lychee Dev automation CLI: task delivery, notices, reload and receipt.

Subcommand groups (design section 9):

- ``task``      update the installed auto.lua registry (upsert/remove/list)
- ``profile``   bind one WoW client installation, addon dir and SV path
- ``sv``        find account-level SavedVariables candidates; read a Ticket
- ``capture``   poll the game window and decode the completion notice QR
- ``send``      deliver one slash command through focus/clipboard/Enter
- ``run``       task flow: run command, notice, output reload, SavedVariables
                read and artifact save (the input-side reload that loads the
                new task block is a separate, operator-visible step)
- ``bugs``      bug snapshot flow: run command, notice, output reload, read
- ``status``    summarize the host session log
- ``recover``   print recent host log events and the suggested next step

Exit codes: 0 success; 1 invalid usage or a missing binding/profile; 2 task
registry failure; 3 SavedVariables read or verification failure; 4 window
identity, command input, or missing optional capture dependency failure; 5 no
notice decoded before the timeout. Expected failures print one explicit
``error: ...`` line instead of a traceback.

Live-window features (capture/send/run/bugs) require optional packages
(windows-capture, numpy, zxing-cpp) and real-game verification; they fail with
explicit messages instead of guessing. Host data lives under the session data
directory, never inside the skill sources or the addon ZIP.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import secrets
import sys
import time
import traceback

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from automation import registry, saved_variables as svlib  # noqa: E402
from automation import session as session_mod  # noqa: E402
from automation.registry import RegistryError  # noqa: E402
from automation.saved_variables import SavedVariablesError  # noqa: E402

NOTICE_MAX_BYTES = 512
BUG_TASK_ID = "bug"
# Client -> candidate install folders, in the order to try. A folder name is a
# location rather than an identity: the launcher reuses a test folder for
# whatever is on the test track, so WoW: Forever currently lives in
# `_classic_beta_` while a dedicated `_forever_` folder is also accepted. The
# right folder is decided by what exists under the WoW root, not by the name.
CLIENT_FOLDERS = {
    "retail": ("_retail_",),
    "classic": ("_classic_",),
    "titan": ("_classic_titan_",),
    "forever": ("_classic_beta_", "_forever_"),
}
DEFAULT_TIMEOUT = 120.0
DEFAULT_INTERVAL = 0.1
SV_READ_RETRIES = 3
SV_READ_RETRY_DELAY = 0.5
EXIT_USAGE = 1
EXIT_REGISTRY = 2
EXIT_SV_READ = 3
EXIT_INPUT = 4
EXIT_NOTICE_TIMEOUT = 5


class CliError(Exception):
    """An expected CLI failure: message on stderr, explicit non-zero exit."""

    def __init__(self, message: str, code: int = EXIT_USAGE):
        super().__init__(message)
        self.code = code


def resolve_install_dir(args, *, required: bool = True) -> str | None:
    if getattr(args, "install_dir", None):
        return args.install_dir
    if getattr(args, "profile", None):
        session = session_mod.Session(data_dir=args.data_dir,
                                      installation=args.installation)
        return load_profile(session, args.profile)["addon_dir"]
    if required:
        raise CliError("either --install-dir or --profile is required")
    return None


# --- task ---------------------------------------------------------------------

def new_request_id() -> str:
    stamp = time.strftime("%Y%m%d-%H%M%S", time.gmtime())
    return f"req-{stamp}-{secrets.token_hex(3)}"


def cmd_task(args) -> int:
    install_dir = resolve_install_dir(args)
    task_path = os.path.join(install_dir, "Modules", "Automation", "auto", "auto.lua")
    reg = registry.TaskRegistry(task_path)

    if args.task_command == "list":
        definitions = reg.read()
        for task_id, definition in definitions.items():
            print(f"{task_id}\t{definition['requestId']}\t{definition['revision']}"
                  f"\t{definition['sourceBytes']}B")
        return 0

    if args.task_command == "remove":
        if not args.id:
            raise CliError("task remove requires --id")
        removed = reg.remove(args.id, expect_revision=args.expect_revision)
        print(f"removed {args.id}" if removed else f"{args.id} not present")
        return 0

    if not args.id:
        raise CliError("task upsert requires --id")
    if not args.source_file:
        raise CliError("task upsert requires --source-file")
    try:
        with open(args.source_file, "rb") as handle:
            raw = handle.read()
    except FileNotFoundError:
        raise CliError(f"source file not found: {args.source_file}")
    except OSError as error:
        raise CliError(f"could not read source file: {error}")
    try:
        # The generated Lua shell stays ASCII; the writer escapes non-ASCII
        # source bytes as \ddd, so the source itself only has to be UTF-8.
        source = raw.decode("utf-8")
    except UnicodeDecodeError as error:
        raise CliError(f"source file is not valid UTF-8: {error}") from error

    request_id = args.request_id or new_request_id()
    definition = reg.upsert(
        args.id,
        request_id=request_id,
        revision=args.revision or new_request_id(),
        source=source,
        created_at=args.created_at or int(time.time()),
        expires_at=args.expires_at or 0,
        expected_interface=args.expected_interface,
        output_limit=args.output_limit,
        expect_revision=args.expect_revision,
    )
    print(json.dumps({
        "taskId": definition["taskId"],
        "requestId": definition["requestId"],
        "revision": definition["revision"],
        "sourceBytes": definition["sourceBytes"],
        "fileHash": definition["fileHash"],
    }, sort_keys=True))
    return 0


# --- profile ------------------------------------------------------------------

def load_profile(session: session_mod.Session, name: str) -> dict:
    profile = session.load_json("profiles.json").get(name)
    if not profile:
        raise CliError(f"profile {name!r} is not configured; use 'profile set' first")
    return profile


def save_profile(session: session_mod.Session, name: str, profile: dict) -> None:
    profiles = session.load_json("profiles.json")
    profiles[name] = profile
    session.save_json("profiles.json", profiles)


def cmd_profile(args) -> int:
    session = session_mod.Session(data_dir=args.data_dir,
                                  installation=args.installation)
    if args.profile_command == "show":
        print(json.dumps(load_profile(session, args.name), sort_keys=True, indent=2))
        return 0
    for flag in ("client", "wow_root", "addon_dir"):
        if not getattr(args, flag):
            raise CliError(f"profile set requires --{flag.replace('_', '-')}")
    if not os.path.isdir(args.addon_dir):
        raise CliError(f"addon directory not found: {args.addon_dir}")
    if not os.path.isdir(args.wow_root):
        raise CliError(f"WoW root directory not found: {args.wow_root}")
    folders = CLIENT_FOLDERS[args.client]
    profile = {
        "client": args.client,
        # The first entry is the default; clientFolders keeps every accepted
        # location so a client that moved folders still resolves.
        "clientFolder": folders[0],
        "clientFolders": list(folders),
        "wow_root": os.path.abspath(args.wow_root),
        "addon_dir": os.path.abspath(args.addon_dir),
        "sv_path": os.path.abspath(args.sv_path) if args.sv_path else None,
    }
    save_profile(session, args.name, profile)
    session.log_event("profile_set", name=args.name, client=args.client)
    print(f"profile {args.name} saved")
    return 0


# --- saved variables ----------------------------------------------------------

def resolve_client_root(profile: dict) -> str:
    """Resolve the client folder (`_retail_`, ...) under the configured WoW root.

    A profile may point either at the WoW root that contains ``_retail_`` or at
    the client folder itself; the account tree decides which one is right. Every
    accepted folder for the client is tried, so a build that moved folders still
    resolves without rewriting the profile.
    """

    root = os.path.abspath(profile.get("wow_root") or "")
    recorded = profile.get("clientFolders")
    if not recorded:
        recorded = [profile.get("clientFolder")] if profile.get("clientFolder") else []
    folders = list(recorded) + list(CLIENT_FOLDERS.get(profile.get("client"), ()))
    candidates = []
    for folder in folders:
        if not folder:
            continue
        path = os.path.join(root, folder)
        if path not in candidates:
            candidates.append(path)
    if root not in candidates:
        candidates.append(root)
    for candidate in candidates:
        if os.path.isdir(os.path.join(candidate, "WTF", "Account")):
            return candidate
    raise CliError(
        f"account root not found for client {profile.get('client')!r}; checked "
        + ", ".join(candidates))


def retry_read(loader, *, attempts: int, delay: float, on_retry=None,
               sleep=time.sleep):
    """Bounded retry for a torn, replaced or temporarily unreadable SV file.

    Returns ``(value, error, attempts_used)``. The loader is called at most
    ``attempts`` times; when every attempt fails the last error is returned.
    """

    last_error: Exception | None = None
    used = 0
    for attempt in range(1, max(1, attempts) + 1):
        used = attempt
        try:
            return loader(), None, used
        except (SavedVariablesError, UnicodeDecodeError, OSError, ValueError) as error:
            last_error = error
            if on_retry is not None:
                on_retry(attempt, error)
            if attempt < max(1, attempts):
                sleep(delay)
    return None, last_error, used


def _load_ticket(sv_path: str, args) -> tuple[dict, str, dict]:
    database = svlib.load_database(sv_path)
    record = svlib.get_record(database, args.ticket)
    failures = svlib.verify_evidence(
        record,
        ticket=args.ticket,
        created_at=args.created_at,
        task_id=args.task,
        execution_id=args.request_id,
        revision=args.revision,
    )
    if failures:
        raise SavedVariablesError("; ".join(failures))
    content = record["payload"]["content"]
    report = svlib.parse_inner_report(
        content,
        task_id=args.task,
        execution_id=args.request_id,
        revision=args.revision,
        request_type=getattr(args, "request_type", None),
        metadata=record.get("metadata"),
    )
    return record, content, report


def cmd_sv(args) -> int:
    session = session_mod.Session(data_dir=args.data_dir,
                                  installation=args.installation)

    if args.sv_command == "find":
        if not args.profile:
            raise CliError("sv find requires --profile")
        profile = load_profile(session, args.profile)
        account_root = os.path.join(resolve_client_root(profile), "WTF", "Account")
        found = []
        for entry in sorted(os.listdir(account_root)):
            candidate = os.path.join(account_root, entry, "SavedVariables",
                                     "Lychee Dev.lua")
            if entry != "SavedVariables" and os.path.isfile(candidate):
                found.append(candidate)
        for candidate in found:
            print(candidate)
        if not found:
            print(f"no account-level 'Lychee Dev.lua' found under {account_root}",
                  file=sys.stderr)
            return EXIT_USAGE
        return 0

    if not args.ticket:
        raise CliError("sv read requires --ticket")
    sv_path = args.sv
    if not sv_path and args.profile:
        sv_path = (load_profile(session, args.profile) or {}).get("sv_path")
    if not sv_path:
        raise CliError("saved variables file not found; bind it via "
                       "'profile set --sv-path'")
    if os.path.basename(sv_path) != "Lychee Dev.lua" or sv_path.endswith(".bak"):
        raise CliError("refusing to read anything but the live 'Lychee Dev.lua' file")
    if not os.path.isfile(sv_path):
        raise CliError(f"saved variables file not found: {sv_path}")

    session.log_event("sv_read_started", ticket=args.ticket, path=sv_path)
    before = session.file_snapshot(sv_path)
    attempts = max(1, int(args.retries) + 1)
    result, error, used = retry_read(
        lambda: _load_ticket(sv_path, args),
        attempts=attempts,
        delay=args.retry_delay,
        on_retry=lambda attempt, failure: session.log_event(
            "sv_read_retry", ticket=args.ticket, attempt=attempt,
            reason=str(failure)),
    )
    if result is None:
        session.log_event("sv_read_failed", ticket=args.ticket, reason=str(error))
        print(f"saved variables read failed after {used} attempt(s): {error}",
              file=sys.stderr)
        return EXIT_SV_READ
    record, content, report = result
    after = session.file_snapshot(sv_path)

    content_bytes = content.encode("utf-8")
    sha256 = hashlib.sha256(content_bytes).hexdigest()
    paths = {
        "evidence": session.save_artifact(
            args.ticket, "evidence.json",
            json.dumps(record, ensure_ascii=False, indent=2, sort_keys=True)),
        "report": session.save_artifact(
            args.ticket, "report.json",
            json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True)),
        "content": session.save_artifact(args.ticket, "content.json", content),
    }
    session.log_event(
        "received",
        ticket=args.ticket,
        requestId=args.request_id,
        taskId=args.task,
        revision=args.revision,
        status=report.get("status"),
        complete=report.get("complete"),
        byteCount=len(content_bytes),
        sha256=sha256,
        attempts=used,
        fileChanged=(before != after),
        artifacts=paths,
    )
    print(json.dumps({
        "ticket": args.ticket,
        "status": report.get("status"),
        "complete": report.get("complete"),
        "byteCount": len(content_bytes),
        "sha256": sha256,
        "attempts": used,
        "artifacts": paths,
    }, sort_keys=True, indent=2))
    return 0


# --- window input and capture --------------------------------------------------

def _window_module():
    try:
        from automation import windows
        return windows
    except Exception as error:  # pragma: no cover - non-Windows hosts
        raise CliError(f"window control requires Windows: {error}")


def _deliver(session: session_mod.Session, windows, args, text: str) -> None:
    """Log the intent, then send exactly one command (design section 6.2)."""

    mode = getattr(args, "mode", "foreground")
    session.log_event("command_sent", hwnd=args.hwnd, text=text, mode=mode)
    try:
        windows.send_command(args.hwnd, text, pid=getattr(args, "pid", None),
                             exe_path=getattr(args, "exe_path", None),
                             mode=mode)
    except (windows.WindowIdentityError, windows.InputError) as error:
        session.log_event("command_failed", hwnd=args.hwnd, text=text,
                          mode=mode, reason=str(error))
        raise CliError(f"command delivery failed: {error}", code=EXIT_INPUT)


def cmd_send(args) -> int:
    """Deliver one command through the host log and the reload dedup."""

    windows = _window_module()
    session = session_mod.Session(data_dir=args.data_dir,
                                  installation=args.installation)
    if args.text == "/reload" and args.request_id and args.ticket:
        if not session.mark_reload_requested(args.request_id, args.ticket):
            raise CliError(
                f"reload for {args.ticket} was already requested; check the saved "
                "variables file before repeating it")
    _deliver(session, windows, args, args.text)
    print("sent")
    return 0


def ack_command(ticket: str, outcome: str) -> str:
    """Build the in-game command that reports a ticket's read outcome.

    The plugin cannot see whether the host read a result, so the host says so
    through the same chat-command channel it already uses for run/bug.
    """

    if outcome not in ("received", "failed"):
        raise CliError("ack status must be 'received' or 'failed'")
    return f"/dev auto ack {ticket} {outcome}"


def cmd_ack(args) -> int:
    """Tell the running game that a ticket was read (or failed to be read)."""

    windows = _window_module()
    session = session_mod.Session(data_dir=args.data_dir,
                                  installation=args.installation)
    text = ack_command(args.ticket, args.status)
    _deliver(session, windows, args, text)
    session.log_event("ticket_ack_sent", ticket=args.ticket, status=args.status)
    print(f"ack sent: {args.ticket} {args.status}")
    return 0


def validate_notice(json_text: str) -> dict:
    if not isinstance(json_text, str) or not json_text:
        raise SavedVariablesError("notice payload is empty")
    if len(json_text.encode("utf-8")) > NOTICE_MAX_BYTES:
        raise SavedVariablesError("notice payload exceeds 512 bytes")
    try:
        notice = json.loads(json_text)
    except ValueError as error:
        raise SavedVariablesError(f"notice is not valid JSON: {error}") from error
    if not isinstance(notice, dict) or notice.get("v") != 1:
        raise SavedVariablesError("unsupported notice protocol version")
    limits = {"ticket": 64, "task": 64, "run": 128}
    for key, limit in limits.items():
        value = notice.get(key)
        if not isinstance(value, str) or not value:
            raise SavedVariablesError(f"notice field {key} is missing")
        if len(value) > limit:
            raise SavedVariablesError(f"notice field {key} exceeds {limit} chars")
    if not svlib.TICKET_PATTERN.match(notice["ticket"]):
        raise SavedVariablesError("notice ticket has an invalid format")
    if not registry.TASK_ID_PATTERN.match(notice["task"]):
        raise SavedVariablesError("notice task id has an invalid format")
    if not registry.REQUEST_ID_PATTERN.match(notice["run"]):
        raise SavedVariablesError("notice run id has an invalid format")
    if not isinstance(notice.get("ts"), int) or isinstance(notice["ts"], bool) \
            or notice["ts"] <= 0:
        raise SavedVariablesError("notice ts is missing or invalid")
    return notice


def notice_identity_problems(notice: dict, *, expected_task: str | None,
                             expected_run: str | None) -> list[str]:
    """Compare a decoded notice against the request this run actually delivered.

    A static QR left on screen from an older run must never be accepted, so the
    task and request identity are cross-checked before any reload (design
    section 6.2).
    """

    problems = []
    if expected_task is not None and notice["task"] != expected_task:
        problems.append(f"notice task {notice['task']!r} != expected {expected_task!r}")
    if expected_run is not None and notice["run"] != expected_run:
        problems.append(f"notice run {notice['run']!r} != expected {expected_run!r}")
    return problems


def poll_notice(windows, session: session_mod.Session, args, *,
                expected_task: str | None = None,
                expected_run: str | None = None) -> dict | None:
    """Keep one WGC session open and poll until a matching notice or the deadline.

    The session outlives a single frame because a task can run far longer than
    one capture interval. Stale notices (older task or request identity) are
    logged and ignored; only the exact expected notice is returned.
    """

    interval = getattr(args, "interval", None) or DEFAULT_INTERVAL
    deadline = time.monotonic() + args.timeout
    with windows.open_notice_capture(args.hwnd, pid=getattr(args, "pid", None),
                                     exe_path=getattr(args, "exe_path", None),
                                     interval=interval) as capture:
        while time.monotonic() < deadline:
            if capture.window_closed:
                session.log_event("notice_capture_closed", hwnd=args.hwnd)
                return None
            roi = capture.latest_roi()
            if roi is not None:
                for payload in windows.decode_qr(roi):
                    try:
                        notice = validate_notice(payload)
                    except (SavedVariablesError, ValueError) as error:
                        session.log_event("notice_rejected", reason=str(error))
                        continue
                    problems = notice_identity_problems(
                        notice, expected_task=expected_task,
                        expected_run=expected_run)
                    if problems:
                        session.log_event("notice_identity_mismatch",
                                          ticket=notice["ticket"],
                                          task=notice["task"],
                                          requestId=notice["run"],
                                          reason="; ".join(problems))
                        continue
                    session.log_event(
                        "notice_seen", ticket=notice["ticket"], task=notice["task"],
                        requestId=notice["run"], ts=notice["ts"])
                    return notice
            time.sleep(interval)
    return None


def cmd_capture(args) -> int:
    windows = _window_module()
    session = session_mod.Session(data_dir=args.data_dir,
                                  installation=args.installation)
    notice = poll_notice(windows, session, args)
    if notice is None:
        print("no completion notice decoded before the timeout (or the capture "
              "session closed)", file=sys.stderr)
        return EXIT_NOTICE_TIMEOUT
    print(json.dumps(notice, sort_keys=True))
    return 0


def cmd_identify(args) -> int:
    """Read the in-game identity marker from one or more game windows.

    Nothing observable from outside the game says which character sits behind
    which window, so with several windows open a slash command cannot be aimed
    without guessing. The addon can display a marker on request
    (``/dev auto identify``); this reads it out of each window so the caller can
    offer the user a character-based choice instead of an arbitrary index.

    Capture does not need the foreground, so this is safe before any input is
    sent. A window with no visible marker is reported with ``identity: null``
    rather than treated as an error: that is the normal state until the user
    asks the game to show one.
    """

    windows = _window_module()
    targets = _identify_targets(args)
    if not targets:
        raise CliError("identify needs at least one window: pass --hwnd, or "
                       "--windows-json with the instances list")

    # Only forward an explicit timeout: passing None would override the helper's
    # own measured default rather than selecting it.
    options = {"interval": args.interval}
    if args.timeout is not None:
        options["timeout"] = args.timeout
    results = windows.read_identities(targets, **options)
    print(json.dumps(results, ensure_ascii=False, sort_keys=True))
    # Every window unreadable is a real failure; a partly readable set is not.
    if results and all(entry["identity"] is None and entry["error"] for entry in results):
        return EXIT_NOTICE_TIMEOUT
    return 0


def _identify_targets(args) -> list[dict]:
    """Build the probe list from repeated --hwnd flags or a windows JSON file."""

    targets: list[dict] = []
    if args.windows_json:
        try:
            with open(args.windows_json, encoding="utf-8") as handle:
                loaded = json.load(handle)
        except OSError as error:
            raise CliError(f"could not read --windows-json: {error}") from error
        except ValueError as error:
            raise CliError(f"--windows-json is not valid JSON: {error}") from error
        if not isinstance(loaded, list):
            raise CliError("--windows-json must contain a JSON array of windows")
        for entry in loaded:
            if not isinstance(entry, dict) or entry.get("hwnd") is None:
                raise CliError("every --windows-json entry needs an 'hwnd'")
            targets.append({
                "hwnd": entry.get("hwnd"),
                "pid": entry.get("pid"),
                "exePath": entry.get("exePath"),
                "flavorFolder": entry.get("flavorFolder"),
                "flavorId": entry.get("flavorId"),
                "flavorLabel": entry.get("flavorLabel"),
            })
        return targets

    for hwnd in args.hwnd or ():
        targets.append({"hwnd": hwnd, "pid": args.pid, "exePath": args.exe_path,
                        "flavorFolder": None, "flavorId": None, "flavorLabel": None})
    return targets


# --- orchestrated flows ---------------------------------------------------------

def _notice_to_sv(session: session_mod.Session, windows, notice: dict, args,
                  *, revision: str | None, request_type: str) -> int:
    """Validate a notice, request one reload, wait, then read the ticket."""

    task = notice["task"]
    request_id = notice["run"]
    ticket = notice["ticket"]
    if not session.mark_reload_requested(request_id, ticket, task=task):
        raise CliError(
            f"reload for {ticket} was already requested; check the saved "
            "variables file before repeating the reload")
    baseline = session.file_snapshot(args.sv)
    _deliver(session, windows, args, "/reload")
    if not session.wait_for_saved_variables(args.sv, timeout=args.timeout,
                                            baseline=baseline):
        session.log_event("sv_write_timeout", ticket=ticket)
        raise CliError("the game did not rewrite its saved variables in time",
                       code=EXIT_NOTICE_TIMEOUT)
    read_args = argparse.Namespace(
        sv_command="read", data_dir=args.data_dir, installation=args.installation,
        profile=None, sv=args.sv, ticket=ticket, task=task, request_id=request_id,
        revision=revision, created_at=notice["ts"], request_type=request_type,
        retries=SV_READ_RETRIES, retry_delay=SV_READ_RETRY_DELAY,
        all_installations=False,
    )
    code = cmd_sv(read_args)
    if code != 0:
        raise CliError(f"saved variables read failed for {ticket}", code=code)
    return 0


def _installed_definition(install_dir: str, task_id: str) -> dict:
    task_path = os.path.join(install_dir, "Modules", "Automation", "auto", "auto.lua")
    definition = registry.TaskRegistry(task_path).read().get(task_id)
    if definition is None:
        raise CliError(f"task {task_id!r} is not present in {task_path}")
    return definition


def cmd_run(args) -> int:
    windows = _window_module()
    session = session_mod.Session(data_dir=args.data_dir,
                                  installation=args.installation)
    install_dir = resolve_install_dir(args, required=False)
    expected_request_id = args.request_id
    revision = args.revision
    if install_dir:
        definition = _installed_definition(install_dir, args.task)
        if args.request_id is not None and args.request_id != definition["requestId"]:
            raise CliError(
                f"--request-id {args.request_id!r} does not match the loaded block "
                f"requestId {definition['requestId']!r}")
        expected_request_id = args.request_id or definition["requestId"]
        revision = args.revision or definition["revision"]

    session.log_event("run_started", task=args.task,
                      requestId=expected_request_id, revision=revision)
    _deliver(session, windows, args, f"/dev auto run {args.task}")
    notice = poll_notice(windows, session, args, expected_task=args.task,
                         expected_run=expected_request_id)
    if notice is None:
        session.log_event("run_unresolved", task=args.task,
                          requestId=expected_request_id)
        raise CliError("no completion notice decoded before the timeout (or the "
                       "capture session closed); the run stays unresolved in the "
                       "host log", code=EXIT_NOTICE_TIMEOUT)
    return _notice_to_sv(session, windows, notice, args, revision=revision,
                         request_type="task")


def cmd_bugs(args) -> int:
    windows = _window_module()
    if not 1 <= args.count <= 100:
        raise CliError("--count must be an integer between 1 and 100")
    session = session_mod.Session(data_dir=args.data_dir,
                                  installation=args.installation)
    session.log_event("bugs_started", count=args.count)
    _deliver(session, windows, args, f"/dev auto bug {args.count}")
    notice = poll_notice(windows, session, args, expected_task=BUG_TASK_ID,
                         expected_run=args.request_id)
    if notice is None:
        session.log_event("bugs_unresolved", count=args.count)
        raise CliError("no completion notice decoded before the timeout (or the "
                       "capture session closed); the run stays unresolved in the "
                       "host log", code=EXIT_NOTICE_TIMEOUT)
    return _notice_to_sv(session, windows, notice, args, revision=args.revision,
                         request_type="bug")


# --- session --------------------------------------------------------------------

def _tail(records: list[dict], limit: int) -> list[dict]:
    if limit <= 0:
        return records
    return records[-limit:]


def _log_scope(args) -> str | None:
    if getattr(args, "all_installations", False):
        return session_mod.ALL_INSTALLATIONS
    return None


def cmd_status(args) -> int:
    session = session_mod.Session(data_dir=args.data_dir,
                                  installation=args.installation)
    events = _tail(session.read_events(args.event, installation=_log_scope(args)),
                   args.limit)
    for record in events:
        print(json.dumps(record, ensure_ascii=False, sort_keys=True))
    if not events:
        print(f"no matching host log events in installation "
              f"{session.installation!r}")
    return 0


def cmd_recover(args) -> int:
    session = session_mod.Session(data_dir=args.data_dir,
                                  installation=args.installation)
    scope = _log_scope(args)
    events = _tail(session.read_events(installation=scope), args.limit)
    for record in events:
        print(json.dumps(record, ensure_ascii=False, sort_keys=True))
    reloads = {r.get("ticket") for r in
               session.read_events("reload_requested", installation=scope)}
    received = {r.get("ticket") for r in
                session.read_events("received", installation=scope)}
    pending = sorted(t for t in reloads if t not in received)
    for ticket in pending:
        print(f"UNRESOLVED: reload requested for {ticket} but not received yet; "
              "inspect the saved variables file manually before any retry")
    if not pending:
        print(f"no unresolved reload requests in installation "
              f"{session.installation!r}")
    return 0


# --- parser ----------------------------------------------------------------------

def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="automation.py",
                                     description=__doc__)
    parser.add_argument("--data-dir", default=session_mod.default_data_dir(),
                        help="host data directory (log, artifacts, profiles)")
    parser.add_argument("--installation", default="default",
                        help="installation name used as the dedup/log scope "
                             "(default: default)")
    sub = parser.add_subparsers(dest="command", required=True)

    task = sub.add_parser("task", help="update the installed auto.lua registry")
    task.add_argument("task_command", choices=["upsert", "remove", "list"])
    task.add_argument("--install-dir", help="installed addon directory (contains "
                                            "Modules/Automation/auto/auto.lua)")
    task.add_argument("--profile", help="profile name to take --install-dir from")
    task.add_argument("--id", help="task id ([A-Za-z0-9_-]{1,64})")
    task.add_argument("--source-file", help="UTF-8 Lua source file for the task")
    task.add_argument("--request-id", help="request id for this execution "
                                           "(default: generated)")
    task.add_argument("--revision", help="source revision (default: generated)")
    task.add_argument("--expect-revision",
                      help="replace only when the stored revision matches")
    task.add_argument("--created-at", type=int, help="creation timestamp (epoch)")
    task.add_argument("--expires-at", type=int, default=0,
                      help="expiry timestamp (epoch, 0 = never)")
    task.add_argument("--expected-interface", type=int,
                      help="expected client Interface version")
    task.add_argument("--output-limit", type=int,
                      help="captured stdout limit in bytes")
    task.set_defaults(func=cmd_task)

    profile = sub.add_parser("profile", help="bind one client installation")
    profile.add_argument("profile_command", choices=["set", "show"])
    profile.add_argument("--name", required=True, help="profile name")
    profile.add_argument("--client", choices=sorted(CLIENT_FOLDERS),
                         help="client type: retail, classic, titan or forever; the "
                              "client folder is resolved from the WoW root")
    profile.add_argument("--wow-root", help="WoW root that contains the client folder")
    profile.add_argument("--addon-dir", help="installed addon directory")
    profile.add_argument("--sv-path", help="exact 'Lychee Dev.lua' path to read")
    profile.set_defaults(func=cmd_profile)

    sv = sub.add_parser("sv", help="find or read SavedVariables")
    sv.add_argument("sv_command", choices=["find", "read"])
    sv.add_argument("--profile", help="profile name (install dir and SV path)")
    sv.add_argument("--sv", help="exact 'Lychee Dev.lua' path")
    sv.add_argument("--ticket", help="exact Ticket to read (required for read)")
    sv.add_argument("--task", help="expected task id")
    sv.add_argument("--request-id", help="expected execution/request id")
    sv.add_argument("--revision", help="expected source revision")
    sv.add_argument("--created-at", type=int, help="expected record createdAt")
    sv.add_argument("--request-type", choices=["task", "bug"],
                    help="expected inner report request type")
    sv.add_argument("--retries", type=int, default=SV_READ_RETRIES,
                    help="additional read attempts for a torn or replaced file "
                         f"(default: {SV_READ_RETRIES})")
    sv.add_argument("--retry-delay", type=float, default=SV_READ_RETRY_DELAY,
                    help="seconds between read attempts "
                         f"(default: {SV_READ_RETRY_DELAY})")
    sv.set_defaults(func=cmd_sv)

    window = argparse.ArgumentParser(add_help=False)
    window.add_argument("--hwnd", type=lambda value: int(value, 0), required=True,
                        help="target window handle (decimal or 0x...)")
    window.add_argument("--pid", type=int, help="expected process id")
    window.add_argument("--exe-path", help="expected executable path")
    window.add_argument(
        "--mode", choices=("foreground", "messages"), default="foreground",
        help="input strategy: 'foreground' focuses the window and uses "
             "SendInput; 'messages' posts keys straight to the window and needs "
             "no foreground but requires the chat box to be open already "
             "(default: foreground)")

    polling = argparse.ArgumentParser(add_help=False)
    polling.add_argument("--timeout", type=float, default=DEFAULT_TIMEOUT,
                         help="seconds to keep polling for the notice "
                              f"(default: {DEFAULT_TIMEOUT})")
    polling.add_argument("--interval", type=float, default=DEFAULT_INTERVAL,
                         help="seconds between notice polls "
                              f"(default: {DEFAULT_INTERVAL})")

    send = sub.add_parser("send", parents=[window],
                          help="deliver one slash command")
    send.add_argument("--text", required=True, help="exact slash command to send")
    send.add_argument("--request-id",
                      help="request id used to dedup a '/reload' command")
    send.add_argument("--ticket", help="ticket used to dedup a '/reload' command")
    send.set_defaults(func=cmd_send)

    ack = sub.add_parser("ack", parents=[window],
                         help="report a ticket's read outcome to the running game")
    ack.add_argument("--ticket", required=True, help="exact ticket that was read")
    ack.add_argument("--status", required=True, choices=("received", "failed"),
                     help="'received' when the report was read, 'failed' otherwise")
    ack.set_defaults(func=cmd_ack)

    capture = sub.add_parser("capture", parents=[window, polling],
                             help="poll the window and decode the notice QR")
    capture.set_defaults(func=cmd_capture)

    identify = sub.add_parser(
        "identify",
        help="read the character/build identity marker from game windows")
    identify.add_argument("--hwnd", type=lambda value: int(value, 0), action="append",
                          help="a window to probe; repeat for several windows")
    identify.add_argument("--windows-json",
                          help="JSON file with the instance list to probe instead "
                               "of --hwnd (the shape 'lycheedev instances' prints)")
    identify.add_argument("--pid", type=int, help="expected process id")
    identify.add_argument("--exe-path", help="expected executable path")
    identify.add_argument("--timeout", type=float, default=None,
                          help="seconds to wait per window for its marker "
                               "(default: the helper's measured budget)")
    identify.add_argument("--interval", type=float, default=DEFAULT_INTERVAL,
                          help=f"seconds between polls per window "
                               f"(default: {DEFAULT_INTERVAL})")
    identify.set_defaults(func=cmd_identify)

    run = sub.add_parser("run", parents=[window, polling],
                         help="deliver and resolve one task run")
    run.add_argument("--task", required=True, help="task id to run")
    run.add_argument("--sv", required=True, help="'Lychee Dev.lua' path to read")
    run.add_argument("--install-dir",
                     help="installed addon directory used to resolve the expected "
                          "request id and revision")
    run.add_argument("--profile", help="profile name for --install-dir")
    run.add_argument("--request-id", help="expected request id (default: the "
                                          "loaded block's requestId)")
    run.add_argument("--revision", help="expected source revision (default: the "
                                        "loaded block's revision)")
    run.set_defaults(func=cmd_run)

    bugs = sub.add_parser("bugs", parents=[window, polling],
                          help="snapshot recent errors and resolve the run")
    bugs.add_argument("--count", type=int, required=True,
                      help="error records to snapshot (1-100)")
    bugs.add_argument("--sv", required=True, help="'Lychee Dev.lua' path to read")
    bugs.add_argument("--request-id", help="expected request id, when known")
    bugs.add_argument("--revision", help="expected source revision, when known")
    bugs.set_defaults(func=cmd_bugs)

    status = sub.add_parser("status", help="print host log events")
    status.add_argument("--event", help="only this event name")
    status.add_argument("--limit", type=int, default=20,
                        help="most recent records to print (0 = all, default: 20)")
    status.add_argument("--all-installations", action="store_true",
                        help="include every installation scope, not just this one")
    status.set_defaults(func=cmd_status)

    recover = sub.add_parser("recover", help="show unresolved reload intents")
    recover.add_argument("--limit", type=int, default=50,
                         help="most recent records to print (0 = all, default: 50)")
    recover.add_argument("--all-installations", action="store_true",
                         help="include every installation scope, not just this one")
    recover.set_defaults(func=cmd_recover)

    return parser


def _expected_error_types() -> tuple:
    errors = [RegistryError, SavedVariablesError, UnicodeDecodeError,
              json.JSONDecodeError, OSError]
    try:
        from automation import windows
    except Exception:  # pragma: no cover - non-Windows hosts
        return tuple(errors)
    errors.extend([windows.WindowIdentityError, windows.InputError,
                   windows.CaptureDependencyError])
    return tuple(errors)


def _window_error_types() -> tuple:
    """Errors that mean the game window, its identity, or the capture
    dependency failed. They map to the documented window/input exit code.

    The result must never be an empty tuple: ``except`` with no exception
    types falls back to catching BaseException, and an exception group is
    rejected outright by ``except``. Non-Windows hosts return a harmless
    unused placeholder so the handler simply never fires.
    """

    try:
        from automation import windows
    except Exception:  # pragma: no cover - non-Windows hosts
        return (RuntimeError,)
    return (windows.WindowIdentityError, windows.InputError,
            windows.CaptureDependencyError)


def main(argv=None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    try:
        return args.func(args)
    except CliError as error:
        print(f"error: {error}", file=sys.stderr)
        return error.code
    except RegistryError as error:
        print(f"error: task registry failed: {error}", file=sys.stderr)
        return EXIT_REGISTRY
    except SavedVariablesError as error:
        print(f"error: saved variables failed: {error}", file=sys.stderr)
        return EXIT_SV_READ
    except _window_error_types() as error:
        print(f"error: {error}", file=sys.stderr)
        return EXIT_INPUT
    except _expected_error_types() as error:
        print(f"error: {error}", file=sys.stderr)
        return EXIT_USAGE
    except KeyboardInterrupt:
        print("error: interrupted", file=sys.stderr)
        return EXIT_USAGE
    except Exception as error:  # unexpected: keep the traceback, stay non-zero
        print(f"unexpected failure: {type(error).__name__}: {error}",
              file=sys.stderr)
        traceback.print_exc()
        return EXIT_USAGE


if __name__ == "__main__":
    sys.exit(main())
