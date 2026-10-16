#!/usr/bin/env python3
"""Prepare authenticated public EmmyLua sources without running upstream code.

All limits are implementation limits, independent of untrusted recipe counts.
Only our newline-terminated, exact-context unified diff format is accepted.
"""
import argparse
from contextlib import contextmanager
import hashlib
import gzip
import json
import os
from pathlib import Path, PurePosixPath
import queue
import re
import stat
import subprocess
import tarfile
import threading
import time
import traceback
import unicodedata
import urllib.request

OWNED_CRATE = 'crates/lycheedev-binding-contracts'
OWNED_FILES = ('Cargo.toml', 'src/lib.rs', 'tests/contracts.rs')

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
CHUNK = 64 * 1024
MAX_FILE_BYTES = 8 * 1024 * 1024
MAX_TREE_BYTES = 64 * 1024 * 1024
MAX_ARCHIVE_BYTES = 8 * 1024 * 1024
MAX_METADATA_BYTES = 2 * 1024 * 1024
MAX_PATCH_BYTES = 4 * 1024 * 1024
MAX_FILES = 4096
MAX_ENTRIES = 8192
MAX_PATCHES = 16
MAX_PATH_BYTES = 1024
MAX_DEPTH = 32
PREPARE_SECONDS = 300
WINDOWS = os.name == 'nt'
HEX = re.compile(r'^[0-9a-f]{64}$')
HUNK = re.compile(rb'^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@(?: [^\r\n]*)?\n$')


def digest(data):
    return hashlib.sha256(data).hexdigest()


def deadline_check(deadline):
    if deadline is not None and time.monotonic() > deadline:
        raise ValueError('emmy.deadline')


def is_reparse(info):
    return stat.S_ISLNK(info.st_mode) or bool(getattr(info, 'st_file_attributes', 0) & 0x400)


def stat_fields(info, *, cross_view=False):
    fields = {name: getattr(info, name, None) for name in (
        'st_dev', 'st_ino', 'st_mode', 'st_size', 'st_mtime_ns',
        'st_ctime_ns', 'st_nlink', 'st_file_attributes', 'st_birthtime_ns')}
    if cross_view and WINDOWS:
        # CPython 3.12 Windows path stat exposes birthtime as ctime, while
        # fstat exposes ChangeTime. Path stat also derives executable bits
        # from the filename. Neither difference denotes file mutation.
        # Full same-view snapshots below still retain ctime and permissions.
        fields.pop('st_ctime_ns')
        fields['st_mode'] = stat.S_IFMT(info.st_mode)
    return fields


def stamp(info):
    return tuple(stat_fields(info).values())


def assert_stat_equal(before, after, phase, *, cross_view=False):
    left = stat_fields(before, cross_view=cross_view)
    right = stat_fields(after, cross_view=cross_view)
    differences = {key: {'before': left[key], 'after': right[key]}
                   for key in left if left[key] != right[key]}
    if differences:
        # Fixed field names and numeric stat values keep diagnostics bounded.
        raise ValueError('emmy.file_changed: ' + json.dumps(
            {'phase': phase, 'differences': differences}, separators=(',', ':')))


def check_ancestors(path):
    path = Path(path).absolute()
    for part in [*reversed(path.parents), path]:
        try:
            info = part.lstat()
        except FileNotFoundError:
            continue
        if is_reparse(info):
            raise ValueError('emmy.symlink_destination')
        if part != path and not stat.S_ISDIR(info.st_mode):
            raise ValueError('emmy.non_directory_ancestor')


def portable_path(value, code='emmy.platform_path'):
    if not isinstance(value, str) or len(value.encode('utf-8')) > MAX_PATH_BYTES:
        raise ValueError(code)
    parts = value.split('/')
    if len(parts) > MAX_DEPTH or not parts:
        raise ValueError(code)
    for part in parts:
        if (not part or part in ('.', '..') or len(part.encode('utf-8')) > 255
                or unicodedata.normalize('NFC', part) != part
                or any(ord(c) < 32 or ord(c) == 127 or c in '\\:<>"|?*' for c in part)
                or part.endswith(('.', ' '))
                or re.match(r'^(CON|PRN|AUX|NUL|COM[1-9¹²³]|LPT[1-9¹²³])(?:\.|$)', part, re.I)):
            raise ValueError(code)
    return value


def ancestor_set(paths):
    return {str(parent) for name in paths for parent in PurePosixPath(name).parents if str(parent) != '.'}


def claim_path(name, aliases):
    # Claim every ancestor, so Foo/a and foo/b also collide on Windows.
    parts = name.split('/')
    for count in range(1, len(parts) + 1):
        part = '/'.join(parts[:count])
        for folded in (part.lower(), part.upper()):
            prior = aliases.setdefault(folded, part)
            if prior != part:
                raise ValueError('emmy.platform_collision')


@contextmanager
def open_regular(path, limit, *, expected_size=None):
    check_ancestors(path)
    before = path.lstat()
    if is_reparse(before):
        raise ValueError('emmy.symlink_in_tree')
    if not stat.S_ISREG(before.st_mode) or before.st_nlink != 1:
        raise ValueError('emmy.non_regular_tree_entry')
    if before.st_size < 0 or before.st_size > limit:
        raise ValueError('emmy.file_budget')
    if expected_size is not None and before.st_size != expected_size:
        raise ValueError('emmy.tree_identity_differs: ' + str(path))
    flags = os.O_RDONLY | getattr(os, 'O_BINARY', 0) | getattr(os, 'O_NOFOLLOW', 0)
    descriptor = os.open(path, flags)
    with os.fdopen(descriptor, 'rb') as stream:
        opened = os.fstat(stream.fileno())
        assert_stat_equal(before, opened, 'path_to_handle', cross_view=True)
        yield stream, before
        assert_stat_equal(opened, os.fstat(stream.fileno()), 'handle_after_read')
        assert_stat_equal(before, path.lstat(), 'path_after_read')


def read_regular(path, limit, *, expected_size=None, collect=False, deadline=None):
    chunks = []
    total = 0
    checksum = hashlib.sha256()
    with open_regular(Path(path), limit, expected_size=expected_size) as (stream, before):
        while True:
            deadline_check(deadline)
            chunk = stream.read(min(CHUNK, limit - total + 1))
            if not chunk:
                break
            total += len(chunk)
            if total > limit or total > before.st_size:
                raise ValueError('emmy.file_budget')
            checksum.update(chunk)
            if collect:
                chunks.append(chunk)
        if total != before.st_size:
            raise ValueError('emmy.file_changed')
    return (b''.join(chunks) if collect else checksum.hexdigest()), before


def validate_rows(rows):
    if not isinstance(rows, list) or not rows or len(rows) > MAX_FILES:
        raise ValueError('emmy.inventory_budget')
    names, aliases, total = set(), {}, 0
    for row in rows:
        if not isinstance(row, dict) or set(row) != {'path', 'bytes', 'sha256'}:
            raise ValueError('emmy.inventory_schema')
        name = portable_path(row['path'])
        if name in names or type(row['bytes']) is not int or not 0 <= row['bytes'] <= MAX_FILE_BYTES or not HEX.fullmatch(row['sha256']):
            raise ValueError('emmy.inventory_schema')
        names.add(name)
        claim_path(name, aliases)
        total += row['bytes']
    if total > MAX_TREE_BYTES or len(names | ancestor_set(names)) > MAX_ENTRIES:
        raise ValueError('emmy.inventory_budget')
    if names & ancestor_set(names) or rows != sorted(rows, key=lambda row: row['path']):
        raise ValueError('emmy.inventory_schema')
    return names


def inventory(root, expected=None, *, deadline=None):
    root = Path(root)
    check_ancestors(root)
    root_stat = root.lstat()
    if not stat.S_ISDIR(root_stat.st_mode):
        raise ValueError('emmy.non_directory_tree')
    wanted = {row['path']: row for row in expected} if expected is not None else None
    wanted_dirs = ancestor_set(validate_rows(expected)) if expected is not None else None
    stack, rows, dirs, snapshots, aliases = [root], [], set(), {root: stamp(root_stat)}, {}
    entries = total = 0
    while stack:
        current = stack.pop()
        deadline_check(deadline)
        with os.scandir(current) as children:
            for entry in children:
                deadline_check(deadline)
                entries += 1
                if entries > MAX_ENTRIES:
                    raise ValueError('emmy.inventory_budget')
                path = current / entry.name
                name = portable_path(path.relative_to(root).as_posix())
                claim_path(name, aliases)
                # Windows DirEntry.stat caches FindFirstFile data, whose
                # inode/device/link-count are zero. Query the path instead.
                info = path.lstat()
                if is_reparse(info):
                    raise ValueError('emmy.symlink_in_tree')
                if stat.S_ISDIR(info.st_mode):
                    if wanted_dirs is not None and name not in wanted_dirs:
                        raise ValueError('emmy.unexpected_directory: ' + name)
                    dirs.add(name)
                    snapshots[path] = stamp(info)
                    stack.append(path)
                    continue
                if wanted is not None and name not in wanted:
                    raise ValueError('emmy.unexpected_file: ' + name)
                if not stat.S_ISREG(info.st_mode) or info.st_nlink != 1:
                    raise ValueError('emmy.non_regular_tree_entry')
                if len(rows) >= MAX_FILES or info.st_size > MAX_FILE_BYTES or total + info.st_size > MAX_TREE_BYTES:
                    raise ValueError('emmy.inventory_budget')
                checksum, observed = read_regular(path, MAX_FILE_BYTES, expected_size=wanted[name]['bytes'] if wanted is not None else None, deadline=deadline)
                if stamp(info) != stamp(observed):
                    raise ValueError('emmy.file_changed')
                total += info.st_size
                rows.append({'path': name, 'bytes': info.st_size, 'sha256': checksum})
                snapshots[path] = stamp(info)
    # Empty directories are input to include_dir!, so they are never ignored.
    if dirs != ancestor_set(row['path'] for row in rows):
        raise ValueError('emmy.unexpected_directory')
    for path, before in snapshots.items():
        deadline_check(deadline)
        if stamp(path.lstat()) != before:
            raise ValueError('emmy.tree_changed')
    return sorted(rows, key=lambda row: row['path'])


def tree_digest(rows):
    return digest(json.dumps(rows, ensure_ascii=True, separators=(',', ':')).encode())


def verify(root, expected, *, deadline=None):
    actual = inventory(root, expected, deadline=deadline)
    if actual != expected:
        want = {row['path']: row for row in expected}
        have = {row['path']: row for row in actual}
        changed = sorted(name for name in want.keys() | have.keys() if want.get(name) != have.get(name))
        raise ValueError('emmy.tree_identity_differs: ' + ', '.join(changed[:12]))
    return tree_digest(actual)


def owned_inventory():
    rows = inventory(ROOT / OWNED_CRATE)
    if [row['path'] for row in rows] != list(OWNED_FILES):
        raise ValueError('emmy.owned_contract_inventory')
    return [{**row, 'path': OWNED_CRATE + '/' + row['path']} for row in rows]


def verify_owned_contracts(identity):
    expected = identity.get('ownedFiles')
    if not isinstance(expected, list):
        raise ValueError('emmy.owned_contract_inventory')
    validate_rows(expected)
    actual = owned_inventory()
    if actual != expected or tree_digest(actual) != identity.get('ownedTreeSHA256'):
        raise ValueError('emmy.owned_contract_identity')
    return tree_digest(actual)


def load_recipe():
    identity_bytes, _ = read_regular(HERE / 'identity.json', MAX_METADATA_BYTES, collect=True)
    files_bytes, _ = read_regular(HERE / 'files.json', MAX_METADATA_BYTES, collect=True)
    identity, expected = json.loads(identity_bytes), json.loads(files_bytes)
    if identity['schema'] != 'lycheedev.emmy-reconstruction.v1':
        raise ValueError('emmy.identity_schema')
    validate_rows(expected)
    owned_digest = verify_owned_contracts(identity)
    if tree_digest(expected) != identity['patchedTreeSHA256']:
        raise ValueError('emmy.tracked_inventory_identity')
    base = identity['base']
    for name, cap in [('archiveBytes', MAX_ARCHIVE_BYTES), ('files', MAX_FILES), ('bytes', MAX_TREE_BYTES)]:
        if type(base[name]) is not int or not 0 < base[name] <= cap:
            raise ValueError('emmy.recipe_budget')
    portable_path(base['archiveRoot'], 'emmy.archive_path')
    if '/' in base['archiveRoot'] or not HEX.fullmatch(base['archiveSHA256']) or not HEX.fullmatch(base['treeSHA256']):
        raise ValueError('emmy.identity_schema')
    if not isinstance(identity['patches'], list) or len(identity['patches']) > MAX_PATCHES:
        raise ValueError('emmy.patch_budget')
    patches, seen = [], set()
    for patch in identity['patches']:
        name = portable_path(patch['path'], 'emmy.patch_path')
        if not name.startswith('patches/') or name in seen:
            raise ValueError('emmy.patch_path')
        seen.add(name)
        data, _ = read_regular(HERE / name, MAX_PATCH_BYTES, collect=True)
        if digest(data) != patch['sha256']:
            raise ValueError('emmy.patch_identity')
        patches.append((name, data))
    inputs = {'identity.json': digest(identity_bytes), 'files.json': digest(files_bytes),
              'ownedContracts': owned_digest,
              **{name: digest(data) for name, data in patches}}
    for name in ('prepare.py', 'scratch.mjs'):
        # Unit fixtures provide their own recipe; production records executable support.
        if (HERE / name).exists():
            inputs[name] = read_regular(HERE / name, MAX_METADATA_BYTES)[0]
    for name in ('tools/rust-disk.mjs', 'release/rust-disk-policy.json'):
        inputs[name] = read_regular(ROOT / name, MAX_METADATA_BYTES)[0]
    return identity, expected, patches, inputs


def verify_recipe_inputs(inputs):
    if load_recipe()[3] != inputs:
        raise ValueError('emmy.recipe_changed')


class ManagedScratch:
    """Own one local custody-helper handle. It never executes source programs."""
    def __init__(self, inputs, command=None):
        self.inputs = inputs
        self.command = command or ['python3', str(HERE / 'prepare.py')]
        self.process = None
        self.directory = None
        self.evidence = None
        self.events = queue.Queue(maxsize=2)

    def _reader(self):
        try:
            for _ in range(2):
                line = self.process.stdout.readline(65537)
                if len(line) > 65536 or not line.endswith(b'\n'):
                    raise ValueError('emmy.scratch_protocol')
                self.events.put(json.loads(line), timeout=1)
        except BaseException as error:
            try:
                self.events.put(error, timeout=1)
            except queue.Full:
                pass

    def _receive(self):
        try:
            value = self.events.get(timeout=30)
        except queue.Empty as error:
            raise ValueError('emmy.scratch_timeout') from error
        if isinstance(value, BaseException):
            raise value
        if 'error' in value:
            raise ValueError('emmy.scratch: ' + value['error'])
        return value

    def _reap(self):
        if self.process is None:
            return
        if self.process.stdin:
            self.process.stdin.close()
        try:
            self.process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.process.terminate()
            try:
                self.process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=5)
        self.process.stdout.close()
        self.reader.join(timeout=5)
        if self.reader.is_alive():
            raise ValueError('emmy.scratch_reader_unreaped')

    def __enter__(self):
        helper = Path(__file__).with_name('scratch.mjs')
        request = json.dumps({'inputs': self.inputs, 'command': self.command,
                              'scratchReserveBytes': MAX_TREE_BYTES + MAX_ARCHIVE_BYTES,
                              'requiredBytes': 2 * MAX_TREE_BYTES + MAX_ARCHIVE_BYTES})
        if len(request.encode()) > 32768:
            raise ValueError('emmy.scratch_protocol')
        self.process = subprocess.Popen(['node', str(helper), str(ROOT), request], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
        self.reader = threading.Thread(target=self._reader, daemon=True)
        self.reader.start()
        try:
            result = self._receive()
            self.directory, self.evidence = Path(result['directory']), Path(result['evidence'])
            return self
        except BaseException:
            self._reap()
            raise

    def __exit__(self, kind, error, tb):
        state = 'succeeded' if kind is None else ('interrupted' if not issubclass(kind, Exception) else 'failed')
        # No external source process exists; this protocol does not assert POSIX
        # Cargo process-tree custody. The single helper is explicitly reaped.
        report = {'state': state, 'treeSHA256': getattr(self, 'treeSHA256', None),
                  'error': ''.join(traceback.format_exception(kind, error, tb))[-24000:] if kind else None}
        try:
            self.process.stdin.write(json.dumps(report).encode() + b'\n')
            self.process.stdin.flush()
            result = self._receive()
            if result.get('state') != state:
                raise ValueError('emmy.scratch_terminal')
        finally:
            self._reap()
        if self.process.returncode != 0:
            raise ValueError('emmy.scratch_exit')
        return False


def authenticate_archive(archive, base, deadline=None):
    checksum, info = read_regular(archive, MAX_ARCHIVE_BYTES, deadline=deadline)
    if info.st_size != base['archiveBytes'] or checksum != base['archiveSHA256']:
        raise ValueError('emmy.archive_identity')
    return info


def download_archive(target, base, deadline):
    # Download only into admitted, private scratch. Publish only verified bytes.
    url = base.get('archiveURL', '')
    if not url.startswith('https://codeload.github.com/CppCXY/emmylua-analyzer-rust/tar.gz/'):
        raise ValueError('emmy.download_url')
    checksum, total = hashlib.sha256(), 0
    with urllib.request.urlopen(url, timeout=15) as response, target.open('xb') as output:
        if not response.geturl().startswith('https://codeload.github.com/'):
            raise ValueError('emmy.download_redirect')
        while True:
            deadline_check(deadline)
            chunk = response.read(min(CHUNK, base['archiveBytes'] - total + 1))
            if not chunk:
                break
            total += len(chunk)
            if total > base['archiveBytes']:
                raise ValueError('emmy.download_identity')
            checksum.update(chunk)
            output.write(chunk)
    if total != base['archiveBytes'] or checksum.hexdigest() != base['archiveSHA256']:
        raise ValueError('emmy.download_identity')


class LimitedReader:
    # Bound decompressed headers/PAX metadata as well as declared file bodies.
    def __init__(self, stream):
        self.stream = stream
        self.remaining = MAX_TREE_BYTES + MAX_ENTRIES * 2048

    def read(self, size):
        data = self.stream.read(min(size, self.remaining + 1))
        self.remaining -= len(data)
        if self.remaining < 0:
            raise ValueError('emmy.archive_expanded_budget')
        return data


def extract_archive(archive, source, base, *, deadline=None):
    before = authenticate_archive(archive, base, deadline)
    source.mkdir()
    names, dirs, aliases, total, entries = set(), set(), {}, 0, 0
    with open_regular(archive, MAX_ARCHIVE_BYTES) as (stream, opened):
        if stamp(before) != stamp(opened):
            raise ValueError('emmy.archive_changed')
        with gzip.GzipFile(fileobj=stream) as expanded, tarfile.open(fileobj=LimitedReader(expanded), mode='r|') as tar:
            for member in tar:
                deadline_check(deadline)
                entries += 1
                if entries > MAX_ENTRIES:
                    raise ValueError('emmy.archive_budget')
                raw = member.name.rstrip('/') if member.isdir() else member.name
                portable_path(raw, 'emmy.archive_path')
                parts = raw.split('/')
                if parts[0] != base['archiveRoot']:
                    raise ValueError('emmy.archive_path')
                if not (member.isdir() or member.isfile()) or (len(parts) == 1 and not member.isdir()):
                    raise ValueError('emmy.archive_entry_kind')
                if len(parts) == 1:
                    continue
                name = '/'.join(parts[1:])
                claim_path(name, aliases)
                if member.isdir():
                    if name in dirs or name in names:
                        raise ValueError('emmy.archive_duplicate_or_platform_path')
                    dirs.add(name)
                    continue
                if name in names or name in dirs or len(names) >= base['files'] or member.size < 0 or member.size > MAX_FILE_BYTES or total + member.size > base['bytes']:
                    raise ValueError('emmy.archive_budget')
                names.add(name)
                total += member.size
                target = source / name
                target.parent.mkdir(parents=True, exist_ok=True)
                remaining = member.size
                with tar.extractfile(member) as data, target.open('xb') as output:
                    while remaining:
                        deadline_check(deadline)
                        chunk = data.read(min(CHUNK, remaining))
                        if not chunk:
                            raise ValueError('emmy.archive_short_read')
                        output.write(chunk)
                        remaining -= len(chunk)
                target.chmod(0o755 if member.mode & 0o111 else 0o644)
    # Authenticated base directories must not contain empty include_dir inputs.
    if dirs - ancestor_set(names):
        raise ValueError('emmy.archive_empty_directory')
    rows = inventory(source, deadline=deadline)
    if len(rows) != base['files'] or total != base['bytes'] or tree_digest(rows) != base['treeSHA256']:
        raise ValueError('emmy.base_tree_identity')
    return rows


def apply_patch(source, patch, *, deadline=None):
    if len(patch) > MAX_PATCH_BYTES:
        raise ValueError('emmy.patch_budget')
    # bytes.splitlines treats CR as a separator; split on LF only to preserve
    # the authenticated source bytes, including CRLF context and non-ASCII text.
    if not patch or not patch.endswith(b'\n') or b'\x00' in patch:
        raise ValueError('emmy.patch_format')
    lines = [line + b'\n' for line in patch[:-1].split(b'\n')]
    initial_rows = inventory(source, deadline=deadline)
    tree_names = {row['path'] for row in initial_rows}
    tree_bytes = sum(row['bytes'] for row in initial_rows)
    cursor, touched = 0, set()
    while cursor < len(lines):
        deadline_check(deadline)
        if not lines[cursor].startswith(b'--- ') or cursor + 1 >= len(lines) or not lines[cursor + 1].startswith(b'+++ '):
            raise ValueError('emmy.patch_format')
        old = lines[cursor][4:-1].decode('utf-8')
        new = lines[cursor + 1][4:-1].decode('utf-8')
        cursor += 2
        oldname = None if old == '/dev/null' else portable_path(old[2:], 'emmy.patch_path') if old.startswith('a/') else None
        newname = None if new == '/dev/null' else portable_path(new[2:], 'emmy.patch_path') if new.startswith('b/') else None
        if (oldname is None and old != '/dev/null') or (newname is None and new != '/dev/null') or (oldname is None and newname is None) or (oldname and newname and oldname != newname):
            raise ValueError('emmy.patch_path')
        name = oldname or newname
        if name in touched:
            raise ValueError('emmy.patch_duplicate')
        touched.add(name)
        if len(touched) > MAX_FILES:
            raise ValueError('emmy.patch_budget')
        target = source / name
        if oldname is None:
            if target.exists() or target.is_symlink():
                raise ValueError('emmy.patch_new_file_exists')
            original = b''
        else:
            original, _ = read_regular(target, MAX_FILE_BYTES, collect=True, deadline=deadline)
        if original and not original.endswith(b'\n'):
            raise ValueError('emmy.patch_requires_newline')
        before = [line + b'\n' for line in original[:-1].split(b'\n')] if original else []
        result, position, result_bytes, hunks = [], 0, 0, 0
        while cursor < len(lines) and not lines[cursor].startswith(b'--- '):
            match = HUNK.fullmatch(lines[cursor])
            if not match:
                raise ValueError('emmy.patch_hunk')
            oldstart, oldcount, newstart, newcount = (int(x) if x is not None else 1 for x in match.groups())
            oldindex = oldstart - 1 if oldcount else oldstart
            newindex = newstart - 1 if newcount else newstart
            if min(oldindex, newindex) < 0 or oldindex < position or oldindex > len(before) or oldindex + oldcount > len(before):
                raise ValueError('emmy.patch_range')
            result.extend(before[position:oldindex])
            result_bytes += sum(map(len, before[position:oldindex]))
            if len(result) != newindex:
                raise ValueError('emmy.patch_range')
            position, removed, added = oldindex, 0, 0
            cursor += 1
            while removed < oldcount or added < newcount:
                if cursor >= len(lines):
                    raise ValueError('emmy.patch_short_hunk')
                line = lines[cursor]
                cursor += 1
                kind, data = line[:1], line[1:]
                if kind not in (b' ', b'-', b'+'):
                    raise ValueError('emmy.patch_hunk')
                if kind in (b' ', b'-'):
                    if removed >= oldcount or position >= len(before) or before[position] != data:
                        raise ValueError('emmy.patch_context')
                    position += 1
                    removed += 1
                if kind in (b' ', b'+'):
                    if added >= newcount:
                        raise ValueError('emmy.patch_hunk')
                    result.append(data)
                    result_bytes += len(data)
                    added += 1
                if result_bytes > MAX_FILE_BYTES:
                    raise ValueError('emmy.patch_budget')
            hunks += 1
        if not hunks:
            raise ValueError('emmy.patch_hunk')
        result.extend(before[position:])
        result_bytes += sum(map(len, before[position:]))
        if result_bytes > MAX_FILE_BYTES:
            raise ValueError('emmy.patch_budget')
        next_names = (tree_names - {name}) if newname is None else (tree_names | {name})
        next_bytes = tree_bytes - len(original) + result_bytes
        if next_bytes > MAX_TREE_BYTES or len(next_names) > MAX_FILES or len(next_names | ancestor_set(next_names)) > MAX_ENTRIES:
            raise ValueError('emmy.patch_tree_budget')
        tree_names, tree_bytes = next_names, next_bytes
        if newname is None:
            if result_bytes:
                raise ValueError('emmy.patch_delete_content')
            target.unlink()
            # Remove only newly empty ancestors inside this private owned tree.
            for parent in target.parents:
                if parent == source:
                    break
                try:
                    parent.rmdir()
                except OSError:
                    break
        else:
            target.parent.mkdir(parents=True, exist_ok=True)
            check_ancestors(target)
            with target.open('xb' if oldname is None else 'wb') as output:
                output.writelines(result)
    return touched


def copy_exclusive(source, target, limit, *, expected_size=None, deadline=None):
    check_ancestors(target)
    with open_regular(source, limit, expected_size=expected_size) as (input_file, before), target.open('xb') as output:
        total = 0
        while True:
            deadline_check(deadline)
            chunk = input_file.read(min(CHUNK, limit - total + 1))
            if not chunk:
                break
            total += len(chunk)
            if total > limit or total > before.st_size:
                raise ValueError('emmy.file_budget')
            output.write(chunk)
        if total != before.st_size:
            raise ValueError('emmy.file_changed')
    target.chmod(stat.S_IMODE(before.st_mode) & 0o777)


def publish(source, destination, expected, *, deadline=None):
    check_ancestors(destination)
    destination.parent.mkdir(parents=True, exist_ok=True)
    # mkdir is the portable atomic no-clobber claim. Rename could replace a
    # concurrent empty directory on POSIX. On failure retain partial output;
    # never delete a tree that a different actor could have modified.
    try:
        destination.mkdir()
    except FileExistsError as error:
        raise ValueError('emmy.destination_appeared') from error
    for name in sorted(ancestor_set(row['path'] for row in expected), key=lambda name: (name.count('/'), name)):
        check_ancestors(destination / name)
        (destination / name).mkdir()
    for row in expected:
        copy_exclusive(source / row['path'], destination / row['path'], MAX_FILE_BYTES, expected_size=row['bytes'], deadline=deadline)
    verify(destination, expected, deadline=deadline)


def prepare(archive, destination, download=False, check_only=False):
    deadline = time.monotonic() + PREPARE_SECONDS
    archive, destination = Path(archive).absolute(), Path(destination).absolute()
    identity, expected, patches, inputs = load_recipe()
    check_ancestors(destination)
    if destination.exists():
        verify(destination, expected, deadline=deadline)
        verify_recipe_inputs(inputs)
        return identity
    if check_only:
        raise ValueError('emmy.prepared_tree_missing')
    check_ancestors(archive)
    if not archive.exists() and not download:
        raise ValueError('emmy.archive_missing: pass --download or supply --archive')
    job_inputs = {**inputs, 'archiveSHA256': identity['base']['archiveSHA256'],
                  'expectedTreeSHA256': identity['patchedTreeSHA256']}
    with ManagedScratch(job_inputs, ['python3', str(HERE / 'prepare.py'), '--archive', str(archive), '--destination', str(destination)]) as job:
        admitted_archive = archive
        downloaded = not archive.exists()
        if downloaded:
            admitted_archive = job.directory / 'base.tar.gz'
            download_archive(admitted_archive, identity['base'], deadline)
        source = job.directory / 'source'
        extract_archive(admitted_archive, source, identity['base'], deadline=deadline)
        for _, patch in patches:
            apply_patch(source, patch, deadline=deadline)
        job.treeSHA256 = verify(source, expected, deadline=deadline)
        if downloaded:
            archive.parent.mkdir(parents=True, exist_ok=True)
            copy_exclusive(admitted_archive, archive, MAX_ARCHIVE_BYTES, expected_size=identity['base']['archiveBytes'], deadline=deadline)
        verify_recipe_inputs(inputs)
        publish(source, destination, expected, deadline=deadline)
        verify_recipe_inputs(inputs)
    return identity


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--archive', type=Path, default=ROOT / '.tmp/emmy-reconstruction-v1/base.tar.gz')
    parser.add_argument('--destination', type=Path, default=ROOT / '.tmp/emmy-reconstruction-v1/upstream')
    parser.add_argument('--download', action='store_true')
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    identity = prepare(args.archive.absolute(), args.destination.absolute(), args.download, args.check)
    print(json.dumps({'state': 'verified', 'identity': identity['identity'], 'patchedTreeSHA256': identity['patchedTreeSHA256'], 'directory': str(args.destination.absolute())}))

if __name__ == '__main__':
    main()
