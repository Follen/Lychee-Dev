"""Task block registry for the Lychee Dev automation task file.

The task file (``Modules/Automation/auto/auto.lua``) only ever contains Lua
data: an empty ``ns.AutomationTaskDefinitions`` table plus optional task blocks
between generated markers. This module owns reading and updating that file:

- the whole installation directory is protected by an OS-level exclusive lock
  file so concurrent writers cannot interleave,
- updates replace exactly one block and keep every other block and its order,
- generated Lua stays Lua 5.1 compatible and pure ASCII (quotes, backslashes,
  newlines, non-ASCII bytes and marker text are escaped),
- writes go through a temp file in the same directory followed by an atomic
  replace, so a crash leaves either the complete old or the complete new file,
- the new file's SHA-256 is recorded for every successful write,
- content outside the task blocks is preserved verbatim; only the block region
  is regenerated,
- the installed path is resolved through symlinks/junctions before any read or
  write, so two names for one directory cannot disagree,
- the parser only accepts the data syntax this module generates; it never
  executes Lua.
"""

from __future__ import annotations

import hashlib
import os
import re
import tempfile
import zlib
from contextlib import contextmanager

try:  # Windows-only; the registry stays importable on other hosts for tests.
    import msvcrt
except ImportError:  # pragma: no cover - exercised only off-Windows
    msvcrt = None

try:
    import fcntl
except ImportError:  # pragma: no cover
    fcntl = None

TASK_ID_PATTERN = re.compile(r"^[A-Za-z0-9_-]{1,64}$")
REQUEST_ID_PATTERN = re.compile(r"^[A-Za-z0-9_.-]{1,128}$")

REGISTRY_SCHEMA_VERSION = 1
REGISTRY_KINDS = ("lua", "bug_snapshot")

MAX_BLOCKS = 16
MAX_REGISTRY_BYTES = 1024 * 1024
MAX_SOURCE_BYTES = 256 * 1024

BEGIN_MARKER = "-- BEGIN LYCHEE DEV TASK "
END_MARKER = "-- END LYCHEE DEV TASK "

REGISTRY_HEADER = """local ADDON_NAME, ns = ...

-- Automation task registry. This file contains generated data only; loading it
-- must never execute task source. Task blocks are written between the
-- BEGIN/END markers by the Lychee Dev skill tooling and are removed before
-- release packaging ships an empty registry.

ns.AutomationTaskDefinitions = ns.AutomationTaskDefinitions or {}

-- Task blocks use exactly this marker shape (task id may not contain the
-- marker text): "-- BEGIN LYCHEE DEV TASK <task-id>" ... "-- END LYCHEE DEV TASK <task-id>".
-- The skill tooling inserts and replaces them; releases ship this file empty.
"""


class RegistryError(Exception):
    """Raised for every expected registry failure; the old file stays intact."""


def validate_task_id(task_id: str) -> str:
    if not isinstance(task_id, str) or not TASK_ID_PATTERN.match(task_id):
        raise RegistryError(f"invalid task id: {task_id!r}")
    return task_id


def validate_request_id(request_id: str) -> str:
    if not isinstance(request_id, str) or not REQUEST_ID_PATTERN.match(request_id):
        raise RegistryError(f"invalid request id: {request_id!r}")
    return request_id


def escape_lua_string(text: str) -> str:
    """Encode text as one Lua 5.1 double-quoted literal, ASCII only."""

    out = ['"']
    for byte in text.encode("utf-8"):
        char = chr(byte)
        if char == '"':
            out.append('\\"')
        elif char == "\\":
            out.append("\\\\")
        elif char == "\n":
            out.append("\\n")
        elif char == "\r":
            out.append("\\r")
        elif char == "\t":
            out.append("\\t")
        elif 0x20 <= byte <= 0x7E:
            out.append(char)
        else:
            out.append(f"\\{byte:03d}")
    out.append('"')
    return "".join(out)


def unescape_lua_string(literal: str) -> str:
    """Decode a Lua 5.1 double-quoted literal (without the quotes) to text."""

    if not literal.startswith('"') or not literal.endswith('"') or len(literal) < 2:
        raise RegistryError("expected a double-quoted Lua string literal")
    body = literal[1:-1]
    out = bytearray()
    index = 0
    while index < len(body):
        char = body[index]
        if char != "\\":
            byte_value = ord(char)
            if byte_value > 0x7E:
                raise RegistryError("non-ASCII byte in string literal")
            out.append(byte_value)
            index += 1
            continue
        index += 1
        if index >= len(body):
            raise RegistryError("dangling escape in string literal")
        escape = body[index]
        simple = {'"': '"', "\\": "\\", "n": "\n", "r": "\r", "t": "\t", "a": "\a",
                  "b": "\b", "f": "\f", "v": "\v"}
        if escape in simple:
            out.extend(simple[escape].encode("utf-8"))
            index += 1
        elif escape in "0123456789":
            # Lua 5.1 \ddd takes one to three decimal digits and must fit a byte.
            match = re.match(r"[0-9]{1,3}", body[index:])
            digits = match.group(0)
            value = int(digits)
            if value > 255:
                raise RegistryError(f"decimal escape too large: \\{digits}")
            out.append(value)
            index += len(digits)
        else:
            raise RegistryError(f"unsupported escape: \\{escape}")
    return out.decode("utf-8")


def adler32_hex(text: str) -> str:
    return format(zlib.adler32(text.encode("utf-8")) & 0xFFFFFFFF, "08x")


def build_block(
    task_id: str,
    *,
    request_id: str,
    revision: str,
    source: str,
    created_at: int,
    expires_at: int = 0,
    expected_interface: int | None = None,
    output_limit: int | None = None,
) -> list[str]:
    validate_task_id(task_id)
    validate_request_id(request_id)
    if not isinstance(revision, str) or not 1 <= len(revision) <= 128:
        raise RegistryError("revision must be a non-empty string of at most 128 chars")
    source_bytes = source.encode("utf-8")
    if not source_bytes:
        raise RegistryError("task source must not be empty")
    if len(source_bytes) > MAX_SOURCE_BYTES:
        raise RegistryError(
            f"task source exceeds the {MAX_SOURCE_BYTES} byte limit: {len(source_bytes)}")

    lines = [
        f"{BEGIN_MARKER}{task_id}",
        f'ns.AutomationTaskDefinitions["{task_id}"] = {{',
        "    schema = 1,",
        f"    requestId = {escape_lua_string(request_id)},",
        f"    revision = {escape_lua_string(revision)},",
        '    kind = "lua",',
        f"    createdAt = {int(created_at)},",
        f"    expiresAt = {int(expires_at or 0)},",
        f"    sourceBytes = {len(source_bytes)},",
        f'    sourceChecksum = "{adler32_hex(source)}",',
    ]
    if expected_interface:
        lines.append(f"    expectedClient = {{ interface = {int(expected_interface)} }},")
    if output_limit:
        lines.append(f"    outputLimit = {int(output_limit)},")
    lines.append(f"    source = {escape_lua_string(source)},")
    lines.append("}")
    lines.append(f"{END_MARKER}{task_id}")
    return lines


class _Block:
    __slots__ = ("task_id", "lines")

    def __init__(self, task_id: str, lines: list[str]):
        self.task_id = task_id
        self.lines = lines


FIELD_PATTERN = re.compile(r"^\s*(\w+) = (.+?),\s*$")


def _parse_field(block: _Block, field: str):
    """Extract one field value from a generated block (data syntax only)."""

    for line in block.lines:
        match = FIELD_PATTERN.match(line)
        if match and match.group(1) == field:
            return match.group(2)
    return None


def _decode_block(block: _Block) -> dict:
    task_id = block.task_id
    if block.lines[0] != f"{BEGIN_MARKER}{task_id}" or block.lines[-1] != f"{END_MARKER}{task_id}":
        raise RegistryError(f"damaged markers in block {task_id}")
    header = block.lines[1]
    if header != f'ns.AutomationTaskDefinitions["{task_id}"] = {{':
        raise RegistryError(f"unexpected block header in {task_id}")
    if block.lines[-2] != "}":
        raise RegistryError(f"unexpected block footer in {task_id}")
    for index, line in enumerate(block.lines[2:-2], start=2):
        if not FIELD_PATTERN.match(line):
            raise RegistryError(
                f"block {task_id} has an unexpected line {index + 1}: {line!r}")

    definition: dict = {"taskId": task_id}
    schema_raw = _parse_field(block, "schema")
    if schema_raw is None or not schema_raw.strip().isdigit():
        raise RegistryError(f"block {task_id} has a bad schema")
    schema = int(schema_raw)
    if schema != REGISTRY_SCHEMA_VERSION:
        raise RegistryError(
            f"block {task_id} uses unsupported schema {schema} "
            f"(supported: {REGISTRY_SCHEMA_VERSION})")
    definition["schema"] = schema
    kind_raw = _parse_field(block, "kind")
    if kind_raw is None:
        raise RegistryError(f"block {task_id} is missing kind")
    kind = unescape_lua_string(kind_raw) if kind_raw.startswith('"') else kind_raw
    if kind not in REGISTRY_KINDS:
        raise RegistryError(f"block {task_id} uses unsupported kind {kind!r}")
    definition["kind"] = kind
    for name in ("requestId", "revision"):
        raw = _parse_field(block, name)
        if raw is None:
            raise RegistryError(f"block {task_id} is missing {name}")
        definition[name] = unescape_lua_string(raw)
    for name in ("createdAt", "expiresAt", "sourceBytes"):
        raw = _parse_field(block, name)
        if raw is None or not raw.lstrip("-").isdigit():
            raise RegistryError(f"block {task_id} has a bad {name}")
        definition[name] = int(raw)
    checksum = _parse_field(block, "sourceChecksum")
    if checksum is None or not re.match(r'^"[0-9a-f]{8}"$', checksum):
        raise RegistryError(f"block {task_id} has a bad sourceChecksum")
    definition["sourceChecksum"] = checksum.strip('"')
    output_limit = _parse_field(block, "outputLimit")
    if output_limit is not None:
        definition["outputLimit"] = int(output_limit)
    expected = _parse_field(block, "expectedClient")
    if expected is not None:
        match = re.match(r"^\{ interface = (\d+) \}$", expected)
        if not match:
            raise RegistryError(f"block {task_id} has a bad expectedClient")
        definition["expectedClient"] = {"interface": int(match.group(1))}
    source_raw = _parse_field(block, "source")
    if source_raw is None:
        raise RegistryError(f"block {task_id} has no source")
    source = unescape_lua_string(source_raw)
    encoded = source.encode("utf-8")
    if len(encoded) != definition["sourceBytes"]:
        raise RegistryError(f"block {task_id} sourceBytes mismatch")
    if adler32_hex(source) != definition["sourceChecksum"]:
        raise RegistryError(f"block {task_id} sourceChecksum mismatch")
    definition["source"] = source
    return definition


def parse_blocks(text: str) -> tuple[dict[str, dict], list[_Block]]:
    """Parse the registry into decoded definitions plus ordered raw blocks."""

    blocks: list[_Block] = []
    current: _Block | None = None
    for line in text.splitlines():
        if line.startswith(BEGIN_MARKER):
            if current is not None:
                raise RegistryError("nested BEGIN marker inside a task block")
            task_id = line[len(BEGIN_MARKER):].strip()
            validate_task_id(task_id)
            current = _Block(task_id, [line])
        elif line.startswith(END_MARKER):
            if current is None:
                raise RegistryError("END marker without a matching BEGIN marker")
            task_id = line[len(END_MARKER):].strip()
            if task_id != current.task_id:
                raise RegistryError("END marker task id does not match its BEGIN marker")
            current.lines.append(line)
            blocks.append(current)
            current = None
        elif current is not None:
            if line.startswith(BEGIN_MARKER[:5]) and line.startswith("-- BEGIN"):
                raise RegistryError("corrupted marker inside a task block")
            current.lines.append(line)
    if current is not None:
        raise RegistryError("unterminated task block")
    if not blocks and "ns.AutomationTaskDefinitions" not in text:
        raise RegistryError("file does not look like a Lychee Dev task registry")

    seen: dict[str, dict] = {}
    for block in blocks:
        if block.task_id in seen:
            raise RegistryError(f"duplicate task block: {block.task_id}")
        seen[block.task_id] = _decode_block(block)
    return seen, blocks


@contextmanager
def _directory_lock(directory: str):
    """Hold an exclusive lock file inside the installation directory."""

    lock_path = os.path.join(directory, "auto.lua.lock")
    lock = open(lock_path, "a+b")
    try:
        if msvcrt is not None:
            msvcrt.locking(lock.fileno(), msvcrt.LK_LOCK, 1)
        elif fcntl is not None:
            fcntl.flock(lock.fileno(), fcntl.LOCK_EX)
        yield
    finally:
        try:
            if msvcrt is not None:
                lock.seek(0)
                msvcrt.locking(lock.fileno(), msvcrt.LK_UNLCK, 1)
            elif fcntl is not None:
                fcntl.flock(lock.fileno(), fcntl.LOCK_UN)
        finally:
            lock.close()


def resolve_registry_path(path: str) -> str:
    """Resolve symlinks and directory junctions before any read or write.

    Two paths that name the same file must lock, read and replace the same
    file; ``realpath`` follows Windows junctions and symlinks so a shared
    installation reached through a link cannot split the revision check.
    """

    return os.path.realpath(os.path.abspath(path))


def split_regions(text: str) -> tuple[str, str]:
    """Return the verbatim text before the first block and after the last one.

    The block region is the only part this module regenerates; anything a human
    or another tool placed outside the markers stays byte-for-byte. The blank
    separator lines right after the last END marker belong to the region so
    repeated writes do not grow the file.
    """

    lines = text.splitlines(keepends=True)
    first = None
    last = None
    for index, line in enumerate(lines):
        if first is None and line.startswith(BEGIN_MARKER):
            first = index
        elif line.startswith(END_MARKER):
            last = index
    if first is None or last is None or last < first:
        return text, ""
    tail = last + 1
    while tail < len(lines) and lines[tail].strip() == "":
        tail += 1
    return "".join(lines[:first]), "".join(lines[tail:])


def _atomic_write(path: str, data: bytes) -> None:
    directory = os.path.dirname(os.path.abspath(path))
    handle = tempfile.NamedTemporaryFile(
        "wb", dir=directory, delete=False, prefix=".auto-", suffix=".tmp")
    try:
        with handle:
            handle.write(data)
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(handle.name, path)
    except BaseException:
        try:
            os.unlink(handle.name)
        except OSError:
            pass
        raise


class TaskRegistry:
    """Reader/writer for one installed auto.lua task registry."""

    def __init__(self, path: str):
        self.path = resolve_registry_path(path)
        self.last_write_hash: str | None = None

    def _read_text(self) -> str:
        if not os.path.exists(self.path):
            return REGISTRY_HEADER
        with open(self.path, "r", encoding="utf-8") as handle:
            return handle.read()

    def read(self) -> dict[str, dict]:
        text = self._read_text()
        definitions, _ = parse_blocks(text)
        return definitions

    def _write(self, blocks: list[_Block], template: str | None = None) -> tuple[str, str]:
        """Write the block region, preserving everything outside it.

        Returns the written text and the SHA-256 of the exact bytes on disk.
        """

        if template is None:
            template = self._read_text()
        prefix, suffix = split_regions(template)
        if prefix and not prefix.endswith("\n"):
            prefix += "\n"
        if blocks and prefix and not prefix.endswith("\n\n"):
            prefix += "\n"
        parts = [prefix]
        for block in blocks:
            for line in block.lines:
                parts.append(line + "\n")
            parts.append("\n")
        parts.append(suffix)
        text = "".join(parts)
        encoded = text.encode("utf-8")
        if len(encoded) > MAX_REGISTRY_BYTES:
            raise RegistryError(
                f"registry exceeds the {MAX_REGISTRY_BYTES} byte limit: {len(encoded)}")
        if "\r" in text or any(ord(char) > 127 for char in text):
            raise RegistryError("registry must stay ASCII with LF endings")
        digest = hashlib.sha256(encoded).hexdigest()
        _atomic_write(self.path, encoded)
        self.last_write_hash = digest
        return text, digest

    def upsert(
        self,
        task_id: str,
        *,
        request_id: str,
        revision: str,
        source: str,
        created_at: int,
        expires_at: int = 0,
        expected_interface: int | None = None,
        output_limit: int | None = None,
        expect_revision: str | None = None,
    ) -> dict:
        validate_task_id(task_id)
        directory = os.path.dirname(self.path) or "."
        os.makedirs(directory, exist_ok=True)
        with _directory_lock(directory):
            text = self._read_text()
            definitions, blocks = parse_blocks(text)
            if task_id in definitions and expect_revision is not None:
                if definitions[task_id]["revision"] != expect_revision:
                    raise RegistryError(
                        f"revision conflict for {task_id}: expected {expect_revision!r}, "
                        f"found {definitions[task_id]['revision']!r}")
            new_lines = build_block(
                task_id,
                request_id=request_id,
                revision=revision,
                source=source,
                created_at=created_at,
                expires_at=expires_at,
                expected_interface=expected_interface,
                output_limit=output_limit,
            )
            replaced = False
            for index, block in enumerate(blocks):
                if block.task_id == task_id:
                    blocks[index] = _Block(task_id, new_lines)
                    replaced = True
                    break
            if not replaced:
                if len(blocks) >= MAX_BLOCKS:
                    raise RegistryError(f"registry already holds {MAX_BLOCKS} task blocks")
                blocks.append(_Block(task_id, new_lines))
            written, file_hash = self._write(blocks, template=text)
            parsed, _ = parse_blocks(written)
        definition = dict(parsed[task_id])
        definition["fileHash"] = file_hash
        return definition

    def remove(self, task_id: str, *, expect_revision: str | None = None) -> bool:
        validate_task_id(task_id)
        directory = os.path.dirname(self.path) or "."
        if not os.path.exists(self.path):
            return False
        with _directory_lock(directory):
            text = self._read_text()
            definitions, blocks = parse_blocks(text)
            if task_id not in definitions:
                return False
            if expect_revision is not None and definitions[task_id]["revision"] != expect_revision:
                raise RegistryError(
                    f"revision conflict for {task_id}: expected {expect_revision!r}, "
                    f"found {definitions[task_id]['revision']!r}")
            remaining = [block for block in blocks if block.task_id != task_id]
            self._write(remaining, template=text)
        return True
