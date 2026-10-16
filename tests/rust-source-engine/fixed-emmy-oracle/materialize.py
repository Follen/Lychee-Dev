#!/usr/bin/env python3
"""Test-only, offline historical source materialization inside an existing owner.

Never downloads, invokes Cargo, takes a channel, or alters the clean workspace.
The historical adapter/source/lock come only from the fixed cd871181 archive.
"""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import stat
import tarfile
import time

HERE = Path(__file__).resolve().parent
MAX_FILE = 8 << 20
MAX_TOTAL = 64 << 20
MAX_FILES = 4096
DEADLINE_SECONDS = 300


def digest(data):
    return hashlib.sha256(data).hexdigest()


def safe_name(name):
    parts = name.split('/')
    if not name or len(name) > 1024 or any(
        p in ('', '.', '..') or any(c in p for c in '\\:') for p in parts
    ):
        raise ValueError('oracle.invalid_path')
    return name


def checked_bytes(path, limit=MAX_FILE):
    path = Path(path)
    for parent in [path, *path.parents]:
        info = parent.lstat()
        if stat.S_ISLNK(info.st_mode) or getattr(info, 'st_file_attributes', 0) & 0x400:
            raise ValueError('oracle.reparse')
    before = path.stat()
    if not stat.S_ISREG(before.st_mode) or before.st_size > limit or before.st_nlink != 1:
        raise ValueError('oracle.file_budget')
    data = path.read_bytes()
    after = path.stat()
    fields = ('st_dev', 'st_ino', 'st_mode', 'st_size', 'st_mtime_ns', 'st_ctime_ns', 'st_nlink')
    if len(data) != before.st_size or any(getattr(after, key) != getattr(before, key) for key in fields):
        raise ValueError('oracle.file_changed')
    return data


def write_file(root, name, data, *, replace=False):
    path = root / safe_name(name)
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open('wb' if replace else 'xb') as stream:
        stream.write(data)
    if digest(checked_bytes(path)) != digest(data):
        raise ValueError('oracle.copy_changed')


def materialize(root, stage, receipt):
    deadline = time.monotonic() + DEADLINE_SECONDS
    def check():
        if time.monotonic() > deadline:
            raise ValueError('oracle.materialize_deadline')
    owner = json.loads(checked_bytes(receipt, 1 << 20))
    if (owner['owner'] != 'ci-emmy-selected-oracle' or owner['state'] != 'active'
            or Path(owner['directory']).resolve() != stage.parent.resolve()
            or stage.name != 'reference-source'
            or not stage.parent.resolve().is_relative_to(root / '.tmp/rust-scratch')):
        raise ValueError('oracle.foreign_owner')
    stage.mkdir()
    frozen = json.loads(checked_bytes(HERE / 'source-cd871181.json', 2 << 20))
    if frozen['commit'] != 'cd87118123fcba3d26f8ee161166629a4570bdaa':
        raise ValueError('oracle.reference_commit')
    archive = checked_bytes(HERE / frozen['archive'])
    if len(archive) != frozen['archiveBytes'] or digest(archive) != frozen['archiveSHA256']:
        raise ValueError('oracle.reference_archive')
    rows = frozen['files']
    if not 0 < len(rows) <= MAX_FILES or sum(r['bytes'] for r in rows) > MAX_TOTAL:
        raise ValueError('oracle.archive_budget')
    expected = {safe_name(row['path']): row for row in rows}
    if len(expected) != len(rows):
        raise ValueError('oracle.archive_duplicate')
    seen = set()
    with tarfile.open(HERE / frozen['archive'], 'r:gz') as source:
        for item in source:
            check()
            row = expected.get(item.name)
            if (row is None or item.name in seen or not item.isfile()
                    or item.size != row['bytes'] or item.size > MAX_FILE):
                raise ValueError('oracle.archive_entry')
            stream = source.extractfile(item)
            data = stream.read(MAX_FILE + 1)
            if len(data) != item.size or digest(data) != row['sha256']:
                raise ValueError('oracle.archive_content')
            write_file(stage, item.name, data)
            seen.add(item.name)
    if seen != expected.keys():
        raise ValueError('oracle.archive_missing')
    # Only the source files explicitly listed by the immutable harness manifest
    # can overlay this fixed source. No runtime candidate semantics are copied.
    manifest = json.loads(checked_bytes(HERE / 'inputs.json', 2 << 20))
    for row in manifest['files']:
        check()
        name = row['path']
        if name.startswith(('historical-engine/', 'reference/')):
            data = checked_bytes(HERE / safe_name(name))
            if len(data) != row['bytes'] or digest(data) != row['sha256']:
                raise ValueError('oracle.overlay_identity')
            target = ('third_party/emmy/' + name[len('historical-engine/'):]
                      if name.startswith('historical-engine/') else name[len('reference/'):])
            write_file(stage, target, data, replace=name.startswith('reference/'))
    spec = importlib.util.spec_from_file_location('fixed_emmy_prepare', stage / 'third_party/emmy/prepare.py')
    prepare = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(prepare)
    clean_rows = json.loads(checked_bytes(root / 'third_party/emmy/files.json', 2 << 20))
    engine = stage / '.tmp/emmy-reconstruction-v1/upstream'
    engine.mkdir(parents=True)
    for row in clean_rows:
        check()
        data = checked_bytes(root / '.tmp/emmy-reconstruction-v1/upstream' / safe_name(row['path']))
        if len(data) != row['bytes'] or digest(data) != row['sha256']:
            raise ValueError('oracle.clean_source_changed')
        write_file(engine, row['path'], data)
    if prepare.verify(engine, clean_rows, deadline=deadline) != '375d92c5d5e503d77232fabf22a21a82f504c2b38dc5def8fcca1d6ecf06e104':
        raise ValueError('oracle.clean_tree')
    identity, expected, patches, _ = prepare.load_recipe()
    for _, patch in patches:
        prepare.apply_patch(engine, patch, deadline=deadline)
    tree = prepare.verify(engine, expected, deadline=deadline)
    if tree != 'cc0a3496bad44f2dfede0799b86175dc3b3332587097bacf6ed691648130fcff':
        raise ValueError('oracle.historical_tree')
    print(json.dumps({'schema':'lycheedev.emmy-fixed-materialized.v1',
                      'commit':frozen['commit'], 'archiveSHA256':frozen['archiveSHA256'],
                      'treeSHA256':tree, 'files':len(expected), 'stage':str(stage)}))


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--stage', type=Path, required=True)
    parser.add_argument('--receipt', type=Path, required=True)
    args = parser.parse_args()
    materialize(args.root.resolve(), args.stage.absolute(), args.receipt.absolute())
