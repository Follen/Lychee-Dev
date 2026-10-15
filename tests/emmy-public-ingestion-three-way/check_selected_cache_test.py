"""Unit-only archive/reparse tests; these do not authenticate a real Cargo cache."""
import gzip
import hashlib
import importlib.util
import io
from pathlib import Path
import stat
import tarfile
import tempfile
import unittest
from unittest.mock import patch
HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('checker', HERE / 'check_selected_cache.py')
checker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checker)

class CacheTests(unittest.TestCase):
    def fixture(self, root):
        content = b'pub fn sample() {}\n'
        raw = io.BytesIO()
        with tarfile.open(fileobj=raw, mode='w') as tar:
            member = tarfile.TarInfo('sample-1.0.0/src/lib.rs'); member.size = len(content)
            tar.addfile(member, io.BytesIO(content))
        archive = gzip.compress(raw.getvalue())
        cache = root / 'registry/cache/index.crates.io-1949cf8c6b5b557f'; cache.mkdir(parents=True)
        (cache / 'sample-1.0.0.crate').write_bytes(archive)
        source = root / 'registry/src/index.crates.io-1949cf8c6b5b557f/sample-1.0.0'; (source / 'src').mkdir(parents=True)
        (source / 'src/lib.rs').write_bytes(content)
        (source / '.cargo-ok').write_bytes(b'{}')
        text = 'version = 4\n'
        for name,version in [('emmylua_code_analysis','0.30.0'),('emmylua_parser','0.30.0'),('emmylua_diagnostic_macro','0.5.0'),('lycheedev-emmy-adapter','3.5.0'),('lycheedev-binding-contracts','3.5.0')]:
            text += f'\n[[package]]\nname = "{name}"\nversion = "{version}"\n'
        text += '\n[[package]]\nname = "sample"\nversion = "1.0.0"\nsource = "registry+https://github.com/rust-lang/crates.io-index"\nchecksum = "'+hashlib.sha256(archive).hexdigest()+'"\n'
        lock = root / 'Cargo.lock'; lock.write_text(text)
        return lock, source, cache / 'sample-1.0.0.crate'

    def test_complete_archive_and_extra_or_missing_source(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); lock, source, _ = self.fixture(root)
            self.assertEqual(len(checker.verify(lock, root)), 1)
            (source / 'extra').write_bytes(b'x')
            with self.assertRaisesRegex(ValueError, 'source_inventory'): checker.verify(lock, root)
            (source / 'extra').unlink(); (source / 'src/lib.rs').unlink()
            with self.assertRaises((ValueError, FileNotFoundError)): checker.verify(lock, root)

    def test_archive_corruption_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); lock, _, archive = self.fixture(root)
            archive.write_bytes(archive.read_bytes()+b'x')
            with self.assertRaisesRegex(ValueError, 'archive_hash'): checker.verify(lock, root)

    def test_windows_reparse_attribute_cannot_be_ignored(self):
        class Info: st_mode = stat.S_IFDIR; st_file_attributes = 0x400
        with patch.object(Path, 'lstat', return_value=Info()):
            with self.assertRaisesRegex(ValueError, 'reparse_path'): checker.no_reparse(Path('owned/cache'))

    def test_crlf_tree_reads_full_selected_closure(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); lock, _, _ = self.fixture(root)
            tree = root / 'tree.txt'
            tree.write_bytes(b'emmylua_code_analysis v0.30.0\r\nemmylua_parser v0.30.0\r\nemmylua_diagnostic_macro v0.5.0\r\nlycheedev-emmy-adapter v3.5.0\r\nlycheedev-binding-contracts v3.5.0\r\nsample v1.0.0\r\n')
            self.assertEqual(checker.verify_selected(lock, root, tree, root)['selected'], 6)

if __name__ == '__main__': unittest.main()
