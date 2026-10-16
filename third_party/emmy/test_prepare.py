#!/usr/bin/env python3
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest import mock
import os
import stat
from types import SimpleNamespace

SPEC = importlib.util.spec_from_file_location('prepare_emmy', Path(__file__).with_name('prepare.py'))
prep = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(prep)

class PrepareTests(unittest.TestCase):
    def setUp(self):
        self.custody = prep.ManagedScratch({'test_prepare.py': prep.read_regular(Path(__file__), prep.MAX_METADATA_BYTES)[0]}, ['python3', str(Path(__file__))])
        self.job = self.custody.__enter__()
        self.temp = tempfile.TemporaryDirectory(prefix='fixture-', dir=self.job.directory)
        self.root = Path(self.temp.name)
        self.old_here = prep.HERE
        prep.HERE = self.root / 'recipe'
        prep.HERE.mkdir()
        (prep.HERE / 'patches').mkdir()
        self.archive = self.root / 'base.tar.gz'
        self.destination = self.root / 'prepared'
        self.content = b'old\n'
        with tarfile.open(self.archive, 'w:gz') as tar:
            member = tarfile.TarInfo('base/a.txt')
            member.size = len(self.content)
            tar.addfile(member, io.BytesIO(self.content))
        self.patch = b'--- a/a.txt\n+++ b/a.txt\n@@ -1 +1 @@\n-old\n+new\n'
        (prep.HERE / 'patches/change.patch').write_bytes(self.patch)
        self.files = [{'path': 'a.txt', 'bytes': 4, 'sha256': prep.digest(b'new\n')}]
        base = [{'path': 'a.txt', 'bytes': 4, 'sha256': prep.digest(self.content)}]
        self.identity = {'schema': 'lycheedev.emmy-reconstruction.v1', 'identity': 'test', 'base': {
            'archiveSHA256': prep.digest(self.archive.read_bytes()), 'archiveBytes': self.archive.stat().st_size,
            'archiveRoot': 'base', 'files': 1, 'bytes': 4, 'treeSHA256': prep.tree_digest(base)},
            'ownedFiles': prep.owned_inventory(), 'ownedTreeSHA256': prep.tree_digest(prep.owned_inventory()),
            'patches': [{'path': 'patches/change.patch', 'sha256': prep.digest(self.patch)}],
            'patchedTreeSHA256': prep.tree_digest(self.files)}
        self.save()

    def save(self):
        (prep.HERE / 'identity.json').write_text(json.dumps(self.identity))
        (prep.HERE / 'files.json').write_text(json.dumps(self.files))

    def tearDown(self):
        prep.HERE = self.old_here
        self.temp.cleanup()
        self.custody.__exit__(None, None, None)

    def test_unmodified_recipe_reproduces_base_without_a_patch(self):
        self.identity.update(identity='public-upstream-unmodified-v1', patches=[], files=1, bytes=4,
                             patchedTreeSHA256=self.identity['base']['treeSHA256'])
        self.files = [{'path': 'a.txt', 'bytes': 4, 'sha256': prep.digest(self.content)}]
        (prep.HERE / 'patches/change.patch').unlink()
        (prep.HERE / 'patches').rmdir()
        self.save()
        prep.prepare(self.archive, self.destination)
        self.assertEqual((self.destination / 'a.txt').read_bytes(), self.content)

    def test_unmodified_recipe_rejects_changed_inventory(self):
        self.identity.update(identity='public-upstream-unmodified-v1', patches=[], files=1, bytes=4)
        (prep.HERE / 'patches/change.patch').unlink()
        self.save()
        with self.assertRaisesRegex(ValueError, 'unmodified_identity'):
            prep.load_recipe()

    def test_unmodified_recipe_rejects_unlisted_patch(self):
        self.identity.update(identity='public-upstream-unmodified-v1', patches=[], files=1, bytes=4,
                             patchedTreeSHA256=self.identity['base']['treeSHA256'])
        self.files = [{'path': 'a.txt', 'bytes': 4, 'sha256': prep.digest(self.content)}]
        self.save()
        with self.assertRaisesRegex(ValueError, 'unknown_patch'):
            prep.load_recipe()

    def test_owned_contract_inventory_is_required(self):
        del self.identity['ownedFiles']
        self.save()
        with self.assertRaisesRegex(ValueError, 'owned_contract_inventory'):
            prep.prepare(self.archive, self.destination)

    def test_owned_contract_digest_is_required(self):
        self.identity['ownedTreeSHA256'] = '0' * 64
        self.save()
        with self.assertRaisesRegex(ValueError, 'owned_contract_identity'):
            prep.prepare(self.archive, self.destination)

    def test_prepare_then_verify_without_archive(self):
        prep.prepare(self.archive, self.destination)
        self.assertEqual((self.destination / 'a.txt').read_bytes(), b'new\n')
        self.archive.unlink()
        prep.prepare(self.archive, self.destination, check_only=True)

    def test_immutable_executable_filename_with_changed_metadata(self):
        # On Windows .exe derives path permission bits; utime makes the
        # handle ChangeTime differ from the path's legacy creation ctime.
        target = self.root / 'immutable.exe'
        target.write_bytes(b'content')
        os.utime(target, ns=(1000000000000000000, 1000000000000000000))
        checksum, _ = prep.read_regular(target, 100)
        self.assertEqual(checksum, prep.digest(b'content'))

    def test_archive_tamper_is_rejected(self):
        self.archive.write_bytes(b'bad')
        with self.assertRaisesRegex(ValueError, 'archive_identity'):
            prep.prepare(self.archive, self.destination)
        self.assertFalse(self.destination.exists())

    def test_patch_tamper_is_rejected(self):
        (prep.HERE / 'patches/change.patch').write_bytes(b'bad')
        with self.assertRaisesRegex(ValueError, 'patch_identity'):
            prep.prepare(self.archive, self.destination)

    def test_inventory_tamper_is_rejected(self):
        self.files[0]['sha256'] = '0' * 64
        self.save()
        with self.assertRaisesRegex(ValueError, 'tracked_inventory_identity'):
            prep.prepare(self.archive, self.destination)

    def test_existing_modified_destination_is_preserved(self):
        self.destination.mkdir()
        (self.destination / 'a.txt').write_bytes(b'foreign\n')
        with self.assertRaisesRegex(ValueError, 'tree_identity_differs'):
            prep.prepare(self.archive, self.destination)
        self.assertEqual((self.destination / 'a.txt').read_bytes(), b'foreign\n')

    def test_missing_tree_check_never_builds(self):
        with self.assertRaisesRegex(ValueError, 'prepared_tree_missing'):
            prep.prepare(self.archive, self.destination, check_only=True)
        self.assertFalse(self.destination.exists())

    def test_source_symlink_is_rejected(self):
        self.destination.mkdir()
        target = self.root / 'foreign'
        target.write_bytes(b'new\n')
        try:
            (self.destination / 'a.txt').symlink_to(target)
        except OSError:
            self.skipTest('platform denies symlink creation')
        with self.assertRaisesRegex(ValueError, 'symlink_in_tree'):
            prep.prepare(self.archive, self.destination)

    def test_archive_traversal_rejected_after_authentication(self):
        with tarfile.open(self.archive, 'w:gz') as tar:
            member = tarfile.TarInfo('base/../escaped')
            member.size = 4
            tar.addfile(member, io.BytesIO(b'bad\n'))
        self.identity['base']['archiveSHA256'] = prep.digest(self.archive.read_bytes())
        self.identity['base']['archiveBytes'] = self.archive.stat().st_size
        self.save()
        with self.assertRaisesRegex(ValueError, 'archive_path'):
            prep.prepare(self.archive, self.destination)
        self.assertFalse((self.root / 'escaped').exists())

    def test_archive_symlink_rejected(self):
        with tarfile.open(self.archive, 'w:gz') as tar:
            member = tarfile.TarInfo('base/link')
            member.type = tarfile.SYMTYPE
            member.linkname = '/tmp'
            tar.addfile(member)
        self.identity['base']['archiveSHA256'] = prep.digest(self.archive.read_bytes())
        self.identity['base']['archiveBytes'] = self.archive.stat().st_size
        self.save()
        with self.assertRaisesRegex(ValueError, 'archive_entry_kind'):
            prep.prepare(self.archive, self.destination)

    def test_patch_cannot_escape_recipe(self):
        self.identity['patches'][0]['path'] = '../foreign.patch'
        self.save()
        with self.assertRaisesRegex(ValueError, 'patch_path'):
            prep.prepare(self.archive, self.destination)

    def test_empty_directory_in_existing_tree_is_rejected(self):
        prep.prepare(self.archive, self.destination)
        (self.destination / 'unexpected').mkdir()
        with self.assertRaisesRegex(ValueError, 'unexpected_directory'):
            prep.verify(self.destination, self.files)

    def test_unknown_file_rejected_before_content_read(self):
        prep.prepare(self.archive, self.destination)
        (self.destination / 'unknown').write_bytes(b'x')
        original = prep.read_regular
        def guarded(path, *args, **kwargs):
            self.assertNotEqual(Path(path).name, 'unknown')
            return original(path, *args, **kwargs)
        with mock.patch.object(prep, 'read_regular', guarded), self.assertRaisesRegex(ValueError, 'unexpected_file'):
            prep.verify(self.destination, self.files)

    def test_oversized_file_rejected_before_content_read(self):
        self.destination.mkdir()
        (self.destination / 'huge').touch()
        with (self.destination / 'huge').open('r+b') as stream:
            stream.truncate(prep.MAX_FILE_BYTES + 1)
        with mock.patch.object(prep, 'read_regular', side_effect=AssertionError('must not read')), self.assertRaisesRegex(ValueError, 'inventory_budget'):
            prep.inventory(self.destination)

    def test_entry_budget_rejected(self):
        self.destination.mkdir()
        for name in ('a', 'b', 'c'):
            (self.destination / name).write_bytes(b'x')
        with mock.patch.object(prep, 'MAX_ENTRIES', 2), self.assertRaisesRegex(ValueError, 'inventory_budget'):
            prep.inventory(self.destination)

    def test_total_budget_rejected(self):
        self.destination.mkdir()
        for name in ('a', 'b'):
            (self.destination / name).write_bytes(b'xx')
        with mock.patch.object(prep, 'MAX_TREE_BYTES', 3), self.assertRaisesRegex(ValueError, 'inventory_budget'):
            prep.inventory(self.destination)

    def test_same_file_changed_during_read_rejected(self):
        path = self.root / 'racy'
        path.write_bytes(b'one')
        with self.assertRaisesRegex(ValueError, 'file_changed'):
            with prep.open_regular(path, 10) as (stream, before):
                self.assertEqual(stream.read(), b'one')
                path.write_bytes(b'two')
                # A same-size immediate write need not advance observable
                # Windows timestamps. Make the real metadata mutation explicit
                # instead of relying on filesystem clock resolution or sleeps.
                os.utime(path, ns=(before.st_atime_ns, before.st_mtime_ns + 2_000_000_000))
                self.assertNotEqual(path.lstat().st_mtime_ns, before.st_mtime_ns)

    def test_same_file_size_changed_during_read_rejected(self):
        path = self.root / 'racy-size'
        path.write_bytes(b'one')
        with self.assertRaisesRegex(ValueError, 'file_changed'):
            with prep.open_regular(path, 10) as (stream, before):
                self.assertEqual(stream.read(), b'one')
                path.write_bytes(b'longer')
                self.assertNotEqual(path.lstat().st_size, before.st_size)

    def test_file_changed_after_inventory_read_rejected(self):
        self.destination.mkdir()
        path = self.destination / 'a.txt'
        path.write_bytes(b'old\n')
        original = prep.read_regular
        def change(path, *args, **kwargs):
            result = original(path, *args, **kwargs)
            Path(path).write_bytes(b'new\n')
            before = result[1]
            os.utime(path, ns=(before.st_atime_ns, before.st_mtime_ns + 2_000_000_000))
            self.assertNotEqual(Path(path).lstat().st_mtime_ns, before.st_mtime_ns)
            return result
        with mock.patch.object(prep, 'read_regular', change), self.assertRaisesRegex(ValueError, 'tree_changed'):
            prep.inventory(self.destination)

    def test_directory_case_alias_rejected(self):
        self.destination.mkdir()
        for name in ('Case', 'case'):
            directory = self.destination / name
            if directory.exists():
                self.skipTest('filesystem is case insensitive')
            directory.mkdir()
            (directory / 'a').write_bytes(b'x')
        with self.assertRaisesRegex(ValueError, 'platform_collision'):
            prep.inventory(self.destination)

    def test_nonportable_paths_rejected(self):
        for path in ('../escape', '/absolute', 'a\\b', 'C:drive', 'a//b', 'a/./b', 'NUL.txt', 'COM1', 'LPT².txt', 'trailing.', 'trailing ', 'a?b', 'e\u0301'):
            with self.subTest(path=path), self.assertRaises(ValueError):
                prep.portable_path(path)

    def test_new_file_patch_supported(self):
        self.destination.mkdir()
        prep.apply_patch(self.destination, b'--- /dev/null\n+++ b/sub/new.txt\n@@ -0,0 +1,2 @@\n+new\n+file\n')
        self.assertEqual((self.destination / 'sub/new.txt').read_bytes(), b'new\nfile\n')

    def test_multiple_hunks_exact_offsets(self):
        self.destination.mkdir()
        path = self.destination / 'a'
        path.write_bytes(b'a\nb\nc\nd\n')
        prep.apply_patch(self.destination, b'--- a/a\n+++ b/a\n@@ -1 +1,2 @@\n-a\n+x\n+y\n@@ -4 +5 @@\n-d\n+z\n')
        self.assertEqual(path.read_bytes(), b'x\ny\nb\nc\nz\n')

    def test_delete_patch_removes_only_empty_ancestors(self):
        (self.destination / 'sub').mkdir(parents=True)
        (self.destination / 'sub/a').write_bytes(b'a\n')
        prep.apply_patch(self.destination, b'--- a/sub/a\n+++ /dev/null\n@@ -1 +0,0 @@\n-a\n')
        self.assertEqual(list(self.destination.iterdir()), [])

    def test_patch_rejections(self):
        cases = [
            b'--- a/a\n+++ b/a\n@@ -1 +1 @@\n-wrong\n+new\n',
            b'--- a/a\n+++ b/a\n@@ -2 +1 @@\n-old\n+new\n',
            b'--- a/a\n+++ b/a\n@@ -1 +2 @@\n-old\n+new\n',
            b'--- a/a\n+++ b/other\n@@ -1 +1 @@\n-old\n+new\n',
            b'--- /dev/null\n+++ b/../escape\n@@ -0,0 +1 @@\n+new\n',
            b'--- a/a\n+++ b/a\n@@ -1 +1 @@\n-old\n+new',
            b'--- a/a\n+++ b/a\n@@ -1 +1 @@\n-old\n',
            b'diff --git a/a b/a\n',
            b'--- a/a\n+++ b/a\n@@ -1 +1 @@\n-old\n+new\n\\ No newline at end of file\n',
        ]
        self.destination.mkdir()
        for patch in cases:
            (self.destination / 'a').write_bytes(b'old\n')
            with self.subTest(patch=patch), self.assertRaises(ValueError):
                prep.apply_patch(self.destination, patch)
        self.assertFalse((self.root / 'escape').exists())

    def test_patch_new_file_never_replaces_existing(self):
        self.destination.mkdir()
        target = self.destination / 'a'
        target.write_bytes(b'foreign\n')
        with self.assertRaisesRegex(ValueError, 'new_file_exists'):
            prep.apply_patch(self.destination, b'--- /dev/null\n+++ b/a\n@@ -0,0 +1 @@\n+new\n')
        self.assertEqual(target.read_bytes(), b'foreign\n')

    def test_archive_empty_directory_rejected(self):
        with tarfile.open(self.archive, 'w:gz') as tar:
            member = tarfile.TarInfo('base/empty')
            member.type = tarfile.DIRTYPE
            tar.addfile(member)
        self.identity['base']['archiveSHA256'] = prep.digest(self.archive.read_bytes())
        self.identity['base']['archiveBytes'] = self.archive.stat().st_size
        self.save()
        with self.assertRaisesRegex(ValueError, 'archive_empty_directory'):
            prep.prepare(self.archive, self.destination)

    def test_archive_case_alias_rejected(self):
        with tarfile.open(self.archive, 'w:gz') as tar:
            for name in ('base/Case/a', 'base/case/b'):
                member = tarfile.TarInfo(name)
                member.size = 1
                tar.addfile(member, io.BytesIO(b'x'))
        self.identity['base']['archiveSHA256'] = prep.digest(self.archive.read_bytes())
        self.identity['base']['archiveBytes'] = self.archive.stat().st_size
        self.identity['base']['files'] = 2
        self.save()
        with self.assertRaisesRegex(ValueError, 'platform_collision'):
            prep.prepare(self.archive, self.destination)

    def test_publish_preserves_concurrent_empty_directory(self):
        self.destination.mkdir()
        source = self.root / 'source'
        source.mkdir()
        (source / 'a.txt').write_bytes(b'new\n')
        before = self.destination.stat()
        with self.assertRaisesRegex(ValueError, 'destination_appeared'):
            prep.publish(source, self.destination, self.files)
        self.assertEqual(self.destination.stat().st_ino, before.st_ino)
        self.assertEqual(list(self.destination.iterdir()), [])

    def test_recipe_changed_before_publish_rejected(self):
        original = prep.apply_patch
        def change(*args, **kwargs):
            result = original(*args, **kwargs)
            self.identity['identity'] = 'changed'
            self.save()
            return result
        with mock.patch.object(prep, 'apply_patch', change), self.assertRaisesRegex(ValueError, 'recipe_changed'):
            prep.prepare(self.archive, self.destination)
        self.assertFalse(self.destination.exists())

    def test_recipe_budget_checked_before_admission(self):
        self.identity['base']['bytes'] = prep.MAX_TREE_BYTES + 1
        self.save()
        with mock.patch.object(prep, 'ManagedScratch', side_effect=AssertionError('must not admit')), self.assertRaisesRegex(ValueError, 'recipe_budget'):
            prep.prepare(self.archive, self.destination)

    def test_hardlink_rejected(self):
        self.destination.mkdir()
        target = self.root / 'foreign'
        target.write_bytes(b'new\n')
        os.link(target, self.destination / 'a.txt')
        with self.assertRaisesRegex(ValueError, 'non_regular_tree_entry'):
            prep.verify(self.destination, self.files)

    def test_expanded_archive_reader_bounded(self):
        reader = prep.LimitedReader(io.BytesIO(b'12345'))
        reader.remaining = 4
        with self.assertRaisesRegex(ValueError, 'expanded_budget'):
            reader.read(1000000000)

    def test_patch_output_budget(self):
        self.destination.mkdir()
        patch = b'--- /dev/null\n+++ b/a\n@@ -0,0 +1 @@\n+12345\n'
        with mock.patch.object(prep, 'MAX_FILE_BYTES', 4), self.assertRaisesRegex(ValueError, 'patch_budget'):
            prep.apply_patch(self.destination, patch)

class StatViewTests(unittest.TestCase):
    @staticmethod
    def info(**changes):
        fields = {'st_dev': 123, 'st_ino': 456, 'st_mode': stat.S_IFREG | 0o666,
                  'st_size': 100, 'st_mtime_ns': 200, 'st_ctime_ns': 300,
                  'st_nlink': 1, 'st_file_attributes': 32, 'st_birthtime_ns': 300}
        fields.update(changes)
        return SimpleNamespace(**fields)

    def test_windows_distinct_ctime_and_filename_permissions(self):
        path = self.info(st_mode=stat.S_IFREG | 0o777)
        handle = self.info(st_ctime_ns=400)
        with mock.patch.object(prep, 'WINDOWS', True):
            prep.assert_stat_equal(path, handle, 'path_to_handle', cross_view=True)
            prep.assert_stat_equal(path, path, 'path_after_read')
            prep.assert_stat_equal(handle, handle, 'handle_after_read')

    def test_windows_same_view_changes_are_not_masked(self):
        for field, value in [('st_ctime_ns', 999), ('st_mode', stat.S_IFREG | 0o444)]:
            with self.subTest(field=field), mock.patch.object(prep, 'WINDOWS', True), self.assertRaisesRegex(ValueError, field):
                prep.assert_stat_equal(self.info(), self.info(**{field: value}), 'handle_after_read')

    def test_windows_cross_view_identity_size_time_type_links_attributes(self):
        for field, value in [('st_dev', 999), ('st_ino', 999), ('st_size', 999),
                             ('st_mtime_ns', 999), ('st_birthtime_ns', 999),
                             ('st_nlink', 2), ('st_file_attributes', 0x400),
                             ('st_mode', stat.S_IFDIR | 0o777)]:
            with self.subTest(field=field), mock.patch.object(prep, 'WINDOWS', True), self.assertRaisesRegex(ValueError, field):
                prep.assert_stat_equal(self.info(), self.info(**{field: value}), 'path_to_handle', cross_view=True)

    def test_posix_cross_view_retains_ctime_and_permissions(self):
        for field, value in [('st_ctime_ns', 999), ('st_mode', stat.S_IFREG | 0o444)]:
            with self.subTest(field=field), mock.patch.object(prep, 'WINDOWS', False), self.assertRaisesRegex(ValueError, field):
                prep.assert_stat_equal(self.info(), self.info(**{field: value}), 'path_to_handle', cross_view=True)

    def test_diagnostics_include_phase_and_differing_numeric_fields(self):
        with self.assertRaisesRegex(ValueError, 'emmy.file_changed') as caught:
            prep.assert_stat_equal(self.info(), self.info(st_size=999), 'path_to_handle')
        detail = json.loads(str(caught.exception).split(': ', 1)[1])
        self.assertEqual(detail, {'phase': 'path_to_handle', 'differences': {'st_size': {'before': 100, 'after': 999}}})

    def test_inventory_ignores_windows_findfirstfile_cached_stat(self):
        with prep.ManagedScratch({}, ['python3', 'windows-direntry-fixture']) as job:
            root = job.directory / 'source'
            root.mkdir()
            (root / 'a').write_bytes(b'a')
            # Windows DirEntry metadata has zero dev/ino/nlink. The scanner
            # must never use it for identity or link-count authorization.
            entry = SimpleNamespace(name='a', stat=mock.Mock(side_effect=AssertionError('cached Windows stat must not be used')))
            scan = mock.MagicMock()
            scan.__enter__.return_value = iter([entry])
            with mock.patch.object(prep.os, 'scandir', return_value=scan):
                self.assertEqual(prep.inventory(root), [{'path': 'a', 'bytes': 1, 'sha256': prep.digest(b'a')}])
            entry.stat.assert_not_called()


class ScratchTests(unittest.TestCase):
    def helper_result(self, terminal):
        request = json.dumps({'inputs': {}, 'command': ['python3', 'custody-protocol-test'], 'scratchReserveBytes': 0, 'requiredBytes': 0})
        process = prep.subprocess.Popen(['node', str(Path(__file__).with_name('scratch.mjs')), str(prep.ROOT), request], stdin=prep.subprocess.PIPE, stdout=prep.subprocess.PIPE, stderr=prep.subprocess.PIPE)
        try:
            output, error = process.communicate(terminal, timeout=30)
        except BaseException:
            process.kill()
            process.wait(timeout=5)
            raise
        self.assertEqual(process.returncode, 0, error.decode())
        self.assertLess(len(output), 65536)
        ready, final = [json.loads(line) for line in output.splitlines()]
        receipt = json.loads(Path(ready['receipt']).read_text())
        return ready, final, receipt

    def test_eof_retains_interrupted_scratch(self):
        ready, final, receipt = self.helper_result(b'')
        self.assertEqual(final['state'], 'interrupted')
        self.assertEqual(receipt['state'], 'interrupted')
        self.assertFalse(receipt['handlesReaped'])
        self.assertTrue(Path(ready['directory']).is_dir())
        self.assertTrue(receipt['sealedLogs'])

    def test_malformed_terminal_retains_interrupted_scratch(self):
        ready, final, receipt = self.helper_result(b'{bad\n')
        self.assertEqual(final['state'], 'interrupted')
        self.assertFalse(receipt['handlesReaped'])
        self.assertTrue(Path(ready['directory']).is_dir())

    def test_oversized_terminal_retains_interrupted_scratch(self):
        ready, final, receipt = self.helper_result(b'x' * 65537 + b'\n')
        self.assertEqual(final['state'], 'interrupted')
        self.assertFalse(receipt['handlesReaped'])
        self.assertTrue(Path(ready['directory']).is_dir())

    def test_success_seals_evidence_and_retires_only_owned_scratch(self):
        ready, final, receipt = self.helper_result(json.dumps({'state': 'succeeded', 'error': None, 'treeSHA256': '1'*64}).encode() + b'\n')
        self.assertEqual(final['state'], 'succeeded')
        self.assertEqual(receipt['state'], 'retired')
        self.assertTrue(receipt['handlesReaped'])
        self.assertFalse(Path(ready['directory']).exists())
        log = Path(receipt['sealedLogs'][0]['path'])
        self.assertEqual(prep.digest(log.read_bytes()), receipt['sealedLogs'][0]['sha256'])
        self.assertEqual(json.loads(log.read_text())['treeSHA256'], '1'*64)

if __name__ == '__main__':
    unittest.main()
