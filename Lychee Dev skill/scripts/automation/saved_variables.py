"""Restricted reader and verifier for Lychee Dev SavedVariables.

The SavedVariables file is data, never code: this module parses it with a
bounded recursive-descent parser that only accepts WoW's stored assignment
syntax (``Name = { ... }`` with ``["key"] = value`` pairs, strings, numbers and
booleans). It refuses and never evaluates functions, calls, operators or any
other expression, so a hostile or truncated file cannot execute anything.

Verification follows the design's fixed order: exact ticket lookup under
``LycheeDevDB.exports.records``, envelope checks, metadata identity against the
host's delivery log, byte counts and the Adler-32 content checksum, then the
inner ``lychee.automation.result.v1`` JSON report.
"""

from __future__ import annotations

import json
import re
import zlib

EVIDENCE_SCHEMA = "lychee.evidence.v1"
RESULT_SCHEMA = "lychee.automation.result.v1"
ADLER_ALGORITHM = "adler32"
MEDIA_TYPE = "application/json"
ENCODING = "utf-8"
TICKET_PATTERN = re.compile(r"^LYCHEE-[A-Za-z0-9-]+$")

# The client emits an explicit ``nil`` for a declared-but-absent SavedVariables
# global (``DumperDB = nil``), and Lua tables can hold ``["key"] = nil``. Both
# mean "not there", so the parser returns this sentinel instead of a value.
NIL = object()

# Terminal states the Lua side may record (design section 8).
TERMINAL_STATUSES = ("succeeded", "failed", "cancelled", "interrupted")
REQUEST_TYPES = ("task", "bug")

# Parser limits: generous enough for a 16 MiB evidence budget with escaping
# overhead, strict enough to bound memory and reject runaway files.
MAX_FILE_BYTES = 256 * 1024 * 1024
MAX_DEPTH = 200
MAX_ENTRIES = 2_000_000
MAX_STRING_BYTES = 64 * 1024 * 1024
# A single automation report is capped at 1 MiB after serialization.
MAX_REPORT_BYTES = 1024 * 1024

STRING_ESCAPES = {'"': '"', "\\": "\\", "n": "\n", "r": "\r", "t": "\t",
                  "a": "\a", "b": "\b", "f": "\f", "v": "\v", "'": "'"}
DIGITS = "0123456789"
DECIMAL_ESCAPE = re.compile(r"[0-9]{1,3}")
ADLER_PATTERN = re.compile(r"^[0-9a-f]{8}$")


class SavedVariablesError(Exception):
    """Raised for parse failures, identity conflicts and verification failures."""


class _Tokenizer:
    def __init__(self, text: str):
        self.text = text
        self.pos = 0
        self.length = len(text)
        self.entries = 0

    def skip_ws(self) -> None:
        while self.pos < self.length and self.text[self.pos] in " \t\r\n":
            self.pos += 1

    def peek(self) -> str:
        if self.pos >= self.length:
            return ""
        return self.text[self.pos]

    def expect(self, char: str) -> None:
        self.skip_ws()
        if self.peek() != char:
            raise SavedVariablesError(
                f"expected {char!r} at offset {self.pos}, found {self.peek()!r}")
        self.pos += 1

    def read_name(self) -> str:
        self.skip_ws()
        match = re.match(r"[A-Za-z_][A-Za-z0-9_]*", self.text[self.pos:])
        if not match:
            raise SavedVariablesError(f"expected a name at offset {self.pos}")
        self.pos += match.end()
        return match.group(0)

    def read_string(self) -> str:
        """Read one quoted string, preserving its exact bytes.

        WoW stores strings with Lua 5.1 escapes; ``\\ddd`` takes one to three
        decimal digits (not exactly three), and multi-byte UTF-8 may arrive as
        literal text or as consecutive ``\\ddd`` bytes. Bytes are accumulated and
        only decoded at the end, so the original content survives untouched.
        """

        self.expect('"')
        out = bytearray()
        while True:
            if self.pos >= self.length:
                raise SavedVariablesError("unterminated string literal")
            char = self.text[self.pos]
            if char == '"':
                self.pos += 1
                try:
                    return out.decode("utf-8")
                except UnicodeDecodeError as error:
                    raise SavedVariablesError(
                        f"string literal is not valid UTF-8: {error}") from error
            if char == "\\":
                self.pos += 1
                if self.pos >= self.length:
                    raise SavedVariablesError("dangling escape in string literal")
                escape = self.text[self.pos]
                if escape in STRING_ESCAPES:
                    out.extend(STRING_ESCAPES[escape].encode("utf-8"))
                    self.pos += 1
                elif escape in DIGITS:
                    match = DECIMAL_ESCAPE.match(self.text, self.pos)
                    digits = match.group(0)
                    value = int(digits)
                    if value > 255:
                        raise SavedVariablesError(
                            f"decimal escape too large: \\{digits}")
                    out.append(value)
                    self.pos += len(digits)
                elif escape == "z":
                    # Lua 5.1 \z skips the following whitespace.
                    self.pos += 1
                    while self.pos < self.length and self.text[self.pos] in " \t\r\n":
                        self.pos += 1
                else:
                    raise SavedVariablesError(f"unsupported escape: \\{escape}")
                if len(out) > MAX_STRING_BYTES:
                    raise SavedVariablesError("string literal exceeds the size limit")
                continue
            if char == "\n" or char == "\r":
                raise SavedVariablesError("raw newline inside a string literal")
            if ord(char) > 0x7E:
                # Literal non-ASCII text was decoded from UTF-8; put the same
                # bytes back so byte counts stay exact.
                out.extend(char.encode("utf-8"))
            else:
                out.append(ord(char))
            self.pos += 1
            if len(out) > MAX_STRING_BYTES:
                raise SavedVariablesError("string literal exceeds the size limit")

    def read_number(self) -> float | int:
        self.skip_ws()
        match = re.match(r"-?\d+\.\d+(?:[eE][+-]?\d+)?|-?\d+(?:[eE][+-]?\d+)?|-?\.\d+",
                         self.text[self.pos:])
        if not match:
            raise SavedVariablesError(f"expected a number at offset {self.pos}")
        self.pos += match.end()
        token = match.group(0)
        if re.fullmatch(r"-?\d+", token):
            return int(token)
        return float(token)

    def count_entry(self) -> None:
        self.entries += 1
        if self.entries > MAX_ENTRIES:
            raise SavedVariablesError("entry limit exceeded while parsing")


def parse_lua_assignments(text: str) -> dict:
    """Parse a SavedVariables document into {globalName: value}.

    The client writes an explicit ``nil`` for a declared SavedVariables global
    that does not exist in this session (for example ``DumperDB = nil``), so
    ``nil`` is accepted as data and means "this global is absent".
    """

    if len(text.encode("utf-8", errors="replace")) > MAX_FILE_BYTES:
        raise SavedVariablesError("saved variables file exceeds the size limit")
    tokenizer = _Tokenizer(text)
    document: dict = {}
    tokenizer.skip_ws()
    while tokenizer.pos < tokenizer.length:
        name = tokenizer.read_name()
        tokenizer.expect("=")
        value = _parse_value(tokenizer, 0)
        if value is NIL:
            document.pop(name, None)
        else:
            document[name] = value
        tokenizer.skip_ws()
    return document


def _parse_value(tokenizer: _Tokenizer, depth: int):
    if depth > MAX_DEPTH:
        raise SavedVariablesError("table nesting limit exceeded")
    tokenizer.skip_ws()
    char = tokenizer.peek()
    if char == "{":
        return _parse_table(tokenizer, depth)
    if char == '"':
        return tokenizer.read_string()
    if char == "-" or char.isdigit():
        return tokenizer.read_number()
    name = tokenizer.read_name()
    if name == "true":
        return True
    if name == "false":
        return False
    if name == "nil":
        return NIL
    raise SavedVariablesError(
        f"unsupported value {name!r}; saved variables are data, not code")


def _parse_table(tokenizer: _Tokenizer, depth: int) -> dict:
    tokenizer.expect("{")
    table: dict = {}
    array_index = 0
    tokenizer.skip_ws()
    if tokenizer.peek() == "}":
        tokenizer.pos += 1
        return table
    while True:
        tokenizer.count_entry()
        tokenizer.skip_ws()
        char = tokenizer.peek()
        if char == "}":  # trailing separator before the closing brace
            tokenizer.pos += 1
            return table
        if char == "[":
            tokenizer.pos += 1
            key = _parse_value(tokenizer, depth + 1)
            tokenizer.expect("]")
            tokenizer.expect("=")
            value = _parse_value(tokenizer, depth + 1)
            if value is not NIL:  # a nil value leaves the key absent in Lua
                table[key if isinstance(key, str) else f"#{key}"] = value
        else:
            first = _parse_value(tokenizer, depth + 1)
            tokenizer.skip_ws()
            if tokenizer.peek() == "=":
                tokenizer.pos += 1
                value = _parse_value(tokenizer, depth + 1)
                if value is not NIL:
                    table[first if isinstance(first, str) else f"#{first}"] = value
            else:
                # Bare array entry: { "a", "b" }.
                array_index += 1
                table[f"#{array_index}"] = first
        tokenizer.skip_ws()
        char = tokenizer.peek()
        if char == "," or char == ";":
            tokenizer.pos += 1
            continue
        if char == "}":
            tokenizer.pos += 1
            return table
        raise SavedVariablesError(
            f"expected ',', ';' or '}}' at offset {tokenizer.pos}")


def load_database(path: str) -> dict:
    """Read one SavedVariables file and return its LycheeDevDB table."""

    with open(path, "rb") as handle:
        raw = handle.read(MAX_FILE_BYTES + 1)
    if len(raw) > MAX_FILE_BYTES:
        raise SavedVariablesError("saved variables file exceeds the size limit")
    text = raw.decode("utf-8", errors="strict")
    document = parse_lua_assignments(text)
    database = document.get("LycheeDevDB")
    if not isinstance(database, dict):
        raise SavedVariablesError("LycheeDevDB table not found in the file")
    return database


def get_record(database: dict, ticket: str) -> dict:
    """Exactly locate exports.records[TICKET]; no nearest-record fallback."""

    if not isinstance(ticket, str) or not TICKET_PATTERN.match(ticket):
        raise SavedVariablesError(f"invalid ticket format: {ticket!r}")
    exports = database.get("exports")
    records = exports.get("records") if isinstance(exports, dict) else None
    if not isinstance(records, dict):
        raise SavedVariablesError("exports.records table not found")
    record = records.get(ticket)
    if record is None:
        raise SavedVariablesError(f"ticket {ticket} not found in saved variables")
    if not isinstance(record, dict):
        raise SavedVariablesError(f"ticket {ticket} does not hold a record table")
    return record


def verify_evidence(
    record: dict,
    *,
    ticket: str,
    created_at: int | None = None,
    task_id: str | None = None,
    execution_id: str | None = None,
    revision: str | None = None,
) -> list[str]:
    """Validate the evidence envelope and identity; returns failure reasons.

    Follows the design's verification order: envelope, payload media type,
    encoding, real UTF-8 byte count against ``byteCount``, metadata identity
    against the host's delivery log, then the Adler-32 content checksum. A
    missing field is a failure, never a silent pass.
    """

    failures: list[str] = []
    if record.get("schema") != EVIDENCE_SCHEMA:
        failures.append(f"schema mismatch: {record.get('schema')!r}")
    if record.get("ticket") != ticket:
        failures.append(f"envelope ticket mismatch: {record.get('ticket')!r}")
    if created_at is not None and record.get("createdAt") != created_at:
        failures.append(
            f"createdAt mismatch: {record.get('createdAt')!r} != {created_at!r}")
    payload = record.get("payload")
    if not isinstance(payload, dict) or not isinstance(payload.get("content"), str):
        failures.append("payload.content is missing or not a string")
        return failures
    if payload.get("mediaType") != MEDIA_TYPE:
        failures.append(f"unexpected payload mediaType: {payload.get('mediaType')!r}")
    if payload.get("encoding") != ENCODING:
        failures.append(f"unexpected payload encoding: {payload.get('encoding')!r}")
    content_bytes = payload["content"].encode("utf-8")
    byte_count = len(content_bytes)
    declared = payload.get("byteCount")
    if not isinstance(declared, int) or isinstance(declared, bool) or declared != byte_count:
        failures.append(f"byteCount mismatch: {declared!r} != {byte_count}")
    if byte_count > MAX_REPORT_BYTES:
        failures.append(
            f"payload content exceeds the {MAX_REPORT_BYTES} byte report limit: {byte_count}")
    metadata = record.get("metadata")
    if not isinstance(metadata, dict):
        failures.append("metadata is missing or not a table")
        metadata = {}
    if task_id is not None and metadata.get("taskId") != task_id:
        failures.append(
            f"metadata taskId mismatch: {metadata.get('taskId')!r} != {task_id!r}")
    if execution_id is not None and metadata.get("executionId") != execution_id:
        failures.append(
            f"metadata executionId mismatch: {metadata.get('executionId')!r} != {execution_id!r}")
    if revision is not None and metadata.get("revision") != revision:
        failures.append(
            f"metadata revision mismatch: {metadata.get('revision')!r} != {revision!r}")
    if metadata.get("resultSchema") != RESULT_SCHEMA:
        failures.append(f"unexpected resultSchema: {metadata.get('resultSchema')!r}")
    if metadata.get("status") not in TERMINAL_STATUSES:
        failures.append(f"unexpected metadata status: {metadata.get('status')!r}")
    if not isinstance(metadata.get("complete"), bool):
        failures.append(f"metadata complete is not a boolean: {metadata.get('complete')!r}")
    if metadata.get("checksumAlgorithm") != ADLER_ALGORITHM:
        failures.append(
            f"unsupported checksumAlgorithm: {metadata.get('checksumAlgorithm')!r}")
    checksum = metadata.get("contentChecksum")
    if not isinstance(checksum, str) or not ADLER_PATTERN.match(checksum):
        failures.append(f"contentChecksum is missing or malformed: {checksum!r}")
    else:
        actual = format(zlib.adler32(content_bytes) & 0xFFFFFFFF, "08x")
        if checksum != actual:
            failures.append(f"contentChecksum mismatch: {checksum} != {actual}")
    return failures


def parse_inner_report(
    content: str,
    *,
    task_id: str | None = None,
    execution_id: str | None = None,
    revision: str | None = None,
    request_type: str | None = None,
    metadata: dict | None = None,
) -> dict:
    """Strictly parse and verify the inner automation report JSON.

    Design section 7.3 step 5 requires the report version, terminal status,
    ``complete`` flag, client/character environment and the request type and
    parameters to be verified, not just the schema. When ``metadata`` is given,
    the report identity must agree with the stored evidence metadata.
    """

    try:
        report = json.loads(content)
    except (ValueError, UnicodeDecodeError) as error:
        raise SavedVariablesError(f"inner report is not valid JSON: {error}") from error
    if not isinstance(report, dict):
        raise SavedVariablesError("inner report is not a JSON object")
    if report.get("schema") != RESULT_SCHEMA:
        raise SavedVariablesError(
            f"unexpected inner report schema: {report.get('schema')!r}")
    if report.get("status") not in TERMINAL_STATUSES:
        raise SavedVariablesError(
            f"inner report has a non-terminal status: {report.get('status')!r}")
    if not isinstance(report.get("complete"), bool):
        raise SavedVariablesError("inner report lacks a boolean complete flag")
    if report["complete"] is False and not isinstance(report.get("incompleteReasons"), list):
        raise SavedVariablesError(
            "incomplete inner report does not list incompleteReasons")
    report_type = report.get("requestType")
    if report_type not in REQUEST_TYPES:
        raise SavedVariablesError(
            f"inner report has an unknown requestType: {report_type!r}")
    environment = report.get("environment")
    if not isinstance(environment, dict) or not environment:
        raise SavedVariablesError("inner report lacks a client/character environment")
    params = report.get("params")
    if report_type == "bug":
        if not isinstance(params, dict):
            raise SavedVariablesError("bug report lacks its request params")
        if not isinstance(params.get("count"), int) or isinstance(params.get("count"), bool):
            raise SavedVariablesError("bug report params.count is not an integer")
        if not isinstance(params.get("scope"), str):
            raise SavedVariablesError("bug report params.scope is not a string")
    elif params is not None and not isinstance(params, dict):
        raise SavedVariablesError("inner report params are not a table")

    expected = {
        "taskId": task_id,
        "executionId": execution_id,
        "revision": revision,
        "requestType": request_type,
    }
    for key, value in expected.items():
        if value is not None and report.get(key) != value:
            raise SavedVariablesError(
                f"inner report {key} mismatch: {report.get(key)!r} != {value!r}")
    if metadata is not None:
        for key in ("taskId", "executionId", "revision", "status"):
            if key in metadata and metadata.get(key) != report.get(key):
                raise SavedVariablesError(
                    f"inner report {key} disagrees with the evidence metadata: "
                    f"{report.get(key)!r} != {metadata.get(key)!r}")
        if "complete" in metadata and bool(metadata.get("complete")) != report["complete"]:
            raise SavedVariablesError(
                "inner report complete flag disagrees with the evidence metadata")
    return report

