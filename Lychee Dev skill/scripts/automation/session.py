"""Host-side session state for the Lychee Dev automation loop.

Everything the host must remember across script restarts lives here: the
append-only host log, the ``(installation, requestId, ticket)`` reload dedup
key, bounded waits for the game to write its SavedVariables file, and the
per-ticket artifact directory. The host log and artifacts live under a local
data directory and never inside the versioned skill sources or the addon ZIP.

Every record carries its installation scope, and reads default to that scope so
``status``/``recover`` cannot mix two WoW installations or multi-account runs.
Artifacts are written byte-for-byte: the recorded byte count and SHA-256 must
describe the exact bytes on disk (no newline translation).
"""

from __future__ import annotations

import json
import os
import tempfile
import time
from dataclasses import dataclass, field

ALL_INSTALLATIONS = "*"


def default_data_dir() -> str:
    base = os.environ.get("LOCALAPPDATA") or os.path.expanduser("~")
    return os.path.join(base, "LycheeDev", "automation")


@dataclass
class Session:
    data_dir: str = field(default_factory=default_data_dir)
    installation: str = "default"

    def __post_init__(self) -> None:
        os.makedirs(self.data_dir, exist_ok=True)
        os.makedirs(self.received_dir, exist_ok=True)

    @property
    def log_path(self) -> str:
        return os.path.join(self.data_dir, "session.log.jsonl")

    @property
    def received_dir(self) -> str:
        return os.path.join(self.data_dir, "received")

    # --- host log ---------------------------------------------------------

    def log_event(self, event: str, **fields) -> None:
        record = {
            "ts": int(time.time()),
            "installation": self.installation,
            "event": event,
        }
        record.update(fields)
        line = json.dumps(record, ensure_ascii=False, sort_keys=True) + "\n"
        with open(self.log_path, "a", encoding="utf-8", newline="\n") as handle:
            handle.write(line)

    def read_events(self, event: str | None = None, *,
                    installation: str | None = None) -> list[dict]:
        """Read host log records, scoped to one installation by default.

        ``installation`` defaults to this session's scope; pass
        ``ALL_INSTALLATIONS`` to read every scope, or another name to inspect a
        different one. A torn tail line is skipped, never fatal.
        """

        scope = self.installation if installation is None else installation
        if not os.path.exists(self.log_path):
            return []
        matches = []
        with open(self.log_path, "r", encoding="utf-8") as handle:
            for line in handle:
                line = line.strip()
                if not line:
                    continue
                try:
                    record = json.loads(line)
                except ValueError:
                    continue  # a torn tail line must not break recovery
                if not isinstance(record, dict):
                    continue
                if event is not None and record.get("event") != event:
                    continue
                if scope != ALL_INSTALLATIONS and record.get("installation") != scope:
                    continue
                matches.append(record)
        return matches

    # --- reload dedup -----------------------------------------------------

    def reload_already_requested(self, request_id: str, ticket: str) -> bool:
        key = (self.installation, request_id, ticket)
        for record in self.read_events("reload_requested"):
            candidate = (record.get("installation"), record.get("requestId"),
                         record.get("ticket"))
            if candidate == key:
                return True
        return False

    def mark_reload_requested(self, request_id: str, ticket: str, **fields) -> bool:
        """Persist the intent before sending /reload; returns False when the
        same static notice was already acted on (one notice, one reload)."""

        if self.reload_already_requested(request_id, ticket):
            return False
        self.log_event("reload_requested", requestId=request_id, ticket=ticket, **fields)
        return True

    # --- artifacts --------------------------------------------------------

    def artifact_dir(self, ticket: str) -> str:
        path = os.path.join(self.received_dir, ticket)
        os.makedirs(path, exist_ok=True)
        return path

    def save_artifact(self, ticket: str, name: str, content: str | bytes) -> str:
        """Write one artifact byte-for-byte, with no newline translation.

        The on-disk bytes are exactly ``content`` encoded as UTF-8 (or the
        bytes passed in), so a recorded byteCount/SHA-256 always matches.
        """

        data = content.encode("utf-8") if isinstance(content, str) else bytes(content)
        path = os.path.join(self.artifact_dir(ticket), name)
        with open(path, "wb") as handle:
            handle.write(data)
        return path

    # --- JSON state files -------------------------------------------------

    def state_path(self, name: str) -> str:
        return os.path.join(self.data_dir, name)

    def load_json(self, name: str):
        path = self.state_path(name)
        if not os.path.exists(path):
            return {}
        with open(path, "r", encoding="utf-8") as handle:
            return json.load(handle)

    def save_json(self, name: str, payload) -> str:
        """Atomically replace one JSON state file inside the data directory."""

        path = self.state_path(name)
        text = json.dumps(payload, ensure_ascii=False, indent=2, sort_keys=True) + "\n"
        directory = os.path.dirname(path) or "."
        os.makedirs(directory, exist_ok=True)
        handle = tempfile.NamedTemporaryFile(
            "wb", dir=directory, delete=False, prefix=".state-", suffix=".tmp")
        try:
            with handle:
                handle.write(text.encode("utf-8"))
                handle.flush()
                os.fsync(handle.fileno())
            os.replace(handle.name, path)
        except BaseException:
            try:
                os.unlink(handle.name)
            except OSError:
                pass
            raise
        return path

    # --- waiting for the disk write ---------------------------------------

    def wait_for_saved_variables(
        self,
        sv_path: str,
        *,
        timeout: float = 120.0,
        poll_interval: float = 0.25,
        baseline: tuple[int, int, float] | None = None,
    ) -> bool:
        """Wait until the file's identity/size/mtime moves from the baseline.

        The change is only a wake-up hint, never proof of completion: callers
        must still verify the expected ticket inside the parsed file.
        """

        def snapshot() -> tuple[int, int, float]:
            info = os.stat(sv_path)
            return (info.st_dev, info.st_size, info.st_mtime)

        if baseline is None:
            try:
                baseline = snapshot()
            except OSError:
                baseline = (0, 0, 0.0)
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            try:
                if snapshot() != baseline:
                    return True
            except OSError:
                pass  # replaced mid-write; keep polling until the deadline
            time.sleep(poll_interval)
        return False

    def file_snapshot(self, sv_path: str) -> tuple[int, int, float]:
        try:
            info = os.stat(sv_path)
            return (info.st_dev, info.st_size, info.st_mtime)
        except OSError:
            return (0, 0, 0.0)
