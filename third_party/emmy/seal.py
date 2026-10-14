#!/usr/bin/env python3
"""Seal a reviewed new reconstruction; never claim historical private recovery.

The authenticated full public base, including its MIT license, is preserved.
This explicit maintainer operation changes the recipe, not the supplied source.
Semantic acceptance requires independent review and tests of the sealed bytes.
"""
import argparse
import difflib
import importlib.util
import json
from pathlib import Path
import time

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('prepare_emmy', HERE / 'prepare.py')
prep = importlib.util.module_from_spec(spec)
spec.loader.exec_module(prep)
COMMIT = '08b0873962e83bb8307e9a81052e3f41578fbc78'
ARCHIVE_SHA256 = 'ae529a06b551dbb301744bea2b58ba94cb3e30340f63a8c83f9335be9086fdb7'
BASE = {
    'repository': 'https://github.com/CppCXY/emmylua-analyzer-rust',
    'commit': COMMIT,
    'archiveURL': 'https://codeload.github.com/CppCXY/emmylua-analyzer-rust/tar.gz/' + COMMIT,
    'archiveRoot': 'emmylua-analyzer-rust-' + COMMIT,
    'archiveSHA256': ARCHIVE_SHA256, 'archiveBytes': 2386393,
    'files': 848, 'bytes': 8382174,
    'treeSHA256': '375d92c5d5e503d77232fabf22a21a82f504c2b38dc5def8fcca1d6ecf06e104',
    'license': {'path': 'LICENSE', 'spdx': 'MIT', 'sha256': '7f8fd5ae0962c6757a8980592c264c8bce5004aa9f369084db761874b8969ff8'},
}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--archive', required=True, type=Path)
    parser.add_argument('--source', required=True, type=Path)
    args = parser.parse_args()
    deadline = time.monotonic() + prep.PREPARE_SECONDS
    archive, source = args.archive.absolute(), args.source.absolute()
    prep.authenticate_archive(archive, BASE, deadline)
    files = prep.inventory(source, deadline=deadline)
    prep.validate_rows(files)
    current = {row['path']: row for row in files}
    inputs = {'archiveSHA256': ARCHIVE_SHA256, 'editedTreeSHA256': prep.tree_digest(files)}
    for name in ('seal.py', 'prepare.py', 'scratch.mjs'):
        inputs[name] = prep.read_regular(HERE / name, prep.MAX_METADATA_BYTES)[0]
    with prep.ManagedScratch(inputs, ['python3', str(HERE / 'seal.py'), '--archive', str(archive), '--source', str(source)]) as job:
        base_source = job.directory / 'public-base'
        base_rows = prep.extract_archive(archive, base_source, BASE, deadline=deadline)
        base = {row['path']: row for row in base_rows}
        patch_path = job.directory / 'reconstruction.patch'
        patch_bytes = 0
        with patch_path.open('xb') as output:
            for name in sorted(base.keys() | current.keys()):
                prep.deadline_check(deadline)
                if base.get(name) == current.get(name):
                    continue
                before = prep.read_regular(base_source / name, prep.MAX_FILE_BYTES, collect=True, deadline=deadline)[0] if name in base else b''
                after = prep.read_regular(source / name, prep.MAX_FILE_BYTES, collect=True, deadline=deadline)[0] if name in current else b''
                if name in current and prep.digest(after) != current[name]['sha256']:
                    raise ValueError('emmy.edited_source_changed')
                if (before and not before.endswith(b'\n')) or (after and not after.endswith(b'\n')):
                    raise ValueError('emmy.patch_requires_newline: ' + name)
                # Diff only bounded text; do not load the entire base/current tree.
                if max(len(before), len(after)) > 1024 * 1024:
                    raise ValueError('emmy.diff_file_budget: ' + name)
                oldlines = [x + '\n' for x in before.decode('utf-8')[:-1].split('\n')] if before else []
                newlines = [x + '\n' for x in after.decode('utf-8')[:-1].split('\n')] if after else []
                changed = False
                for line in difflib.unified_diff(oldlines, newlines, fromfile='a/' + name if name in base else '/dev/null', tofile='b/' + name if name in current else '/dev/null'):
                    prep.deadline_check(deadline)
                    encoded = line.encode('utf-8')
                    patch_bytes += len(encoded)
                    if patch_bytes > prep.MAX_PATCH_BYTES:
                        raise ValueError('emmy.patch_budget')
                    output.write(encoded)
                    changed = True
                if not changed:
                    raise ValueError('emmy.patch_empty_file_change: ' + name)
        patch, _ = prep.read_regular(patch_path, prep.MAX_PATCH_BYTES, collect=True, deadline=deadline)
        if patch:
            prep.apply_patch(base_source, patch, deadline=deadline)
        prep.verify(base_source, files, deadline=deadline)
        prep.verify(source, files, deadline=deadline)
        license_row = current.get(BASE['license']['path'])
        if not license_row or license_row['sha256'] != BASE['license']['sha256']:
            raise ValueError('emmy.license_changed')
        identity = {
            'schema': 'lycheedev.emmy-reconstruction.v1',
            'identity': 'public-base-binding-context-reconstruction-v1',
            'historicalPrivatePatchRecovered': False,
            'base': BASE,
            'patches': [{'path': 'patches/0001-binding-context.patch', 'sha256': prep.digest(patch)}] if patch else [],
            'patchedTreeSHA256': prep.tree_digest(files),
            'files': len(files), 'bytes': sum(row['bytes'] for row in files),
        }
        # Only this explicitly requested maintainer command replaces recipe files.
        # Preparation authenticates the complete triple and rejects partial edits.
        (HERE / 'patches/0001-binding-context.patch').write_bytes(patch)
        (HERE / 'files.json').write_text(json.dumps(files, indent=2) + '\n')
        (HERE / 'identity.json').write_text(json.dumps(identity, indent=2) + '\n')
        job.treeSHA256 = identity['patchedTreeSHA256']
    print(json.dumps(identity, indent=2))

if __name__ == '__main__':
    main()
