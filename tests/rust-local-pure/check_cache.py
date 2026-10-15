"""Read-only proof that the reused Cargo sources equal official locked archives."""
import gzip
import hashlib
import io
import json
from pathlib import Path, PurePosixPath
import sys
import tarfile
import tomllib


def digest(data):
    return hashlib.sha256(data).hexdigest()


def bounded(path, limit):
    if path.is_symlink() or not path.is_file() or path.stat().st_size > limit:
        raise ValueError(f"cache.invalid_file:{path.name}")
    with path.open("rb") as stream:
        data = stream.read(limit + 1)
    if len(data) > limit:
        raise ValueError("cache.file_budget")
    return data


def verify(lock, home):
    packages = tomllib.loads(bounded(lock, 128 << 10).decode())["package"]
    if len(packages) > 64:
        raise ValueError("cache.package_budget")
    registry = "index.crates.io-1949cf8c6b5b557f"
    rows = []
    for package in packages:
        if "source" not in package:
            if package["name"] != "lycheedev-local-pure":
                raise ValueError("cache.unexpected_local_package")
            continue
        if package["source"] != "registry+https://github.com/rust-lang/crates.io-index":
            raise ValueError("cache.unofficial_source")
        prefix = f'{package["name"]}-{package["version"]}'
        if "/" in prefix or "\\" in prefix or prefix in (".", ".."):
            raise ValueError("cache.invalid_package")
        archive = bounded(home / "registry/cache" / registry / f"{prefix}.crate", 16 << 20)
        if digest(archive) != package["checksum"]:
            raise ValueError(f"cache.archive_hash:{prefix}")
        with gzip.GzipFile(fileobj=io.BytesIO(archive)) as stream:
            unpacked = stream.read((64 << 20) + 1)
        if len(unpacked) > 64 << 20:
            raise ValueError("cache.archive_budget")
        source = home / "registry/src" / registry / prefix
        expected, directories = {}, set()
        with tarfile.open(fileobj=io.BytesIO(unpacked), mode="r:") as tar:
            for index, member in enumerate(tar):
                if index >= 10000:
                    raise ValueError("cache.entry_budget")
                path = PurePosixPath(member.name)
                if path.is_absolute() or ".." in path.parts or path.parts[0] != prefix:
                    raise ValueError("cache.archive_path")
                name = str(PurePosixPath(*path.parts[1:]))
                if name == ".":
                    if not member.isdir():
                        raise ValueError("cache.archive_root")
                    continue
                directories.update(str(parent) for parent in PurePosixPath(name).parents if str(parent) != ".")
                if member.isdir():
                    directories.add(name)
                    continue
                if not member.isfile() or name in expected or member.size > 16 << 20:
                    raise ValueError("cache.archive_entry")
                data = tar.extractfile(member).read((16 << 20) + 1)
                if len(data) != member.size:
                    raise ValueError("cache.archive_length")
                target = source / name
                for parent in [source, *target.parents]:
                    if parent.is_symlink():
                        raise ValueError("cache.source_symlink")
                actual = bounded(target, 16 << 20)
                if actual != data:
                    raise ValueError(f"cache.source_bytes:{prefix}/{name}")
                expected[name] = digest(actual)
        actual_files, actual_dirs = set(), set()
        for index, path in enumerate(source.rglob("*")):
            if index >= 10000 or path.is_symlink():
                raise ValueError("cache.source_tree")
            name = path.relative_to(source).as_posix()
            if path.is_dir():
                actual_dirs.add(name)
            elif path.is_file():
                if name == ".cargo-ok":
                    bounded(path, 256)  # Cargo's extraction marker, not source.
                else:
                    actual_files.add(name)
            else:
                raise ValueError("cache.source_special")
        if actual_files != set(expected) or actual_dirs != directories:
            raise ValueError(f"cache.source_inventory:{prefix}")
        rows.append({"name": package["name"], "version": package["version"],
                     "archiveSHA256": package["checksum"], "files": len(expected),
                     "treeSHA256": digest(json.dumps(sorted(expected.items()), separators=(",", ":")).encode())})
    if not rows:
        raise ValueError("cache.empty_closure")
    return rows


if __name__ == "__main__":
    print(json.dumps(verify(Path(sys.argv[1]), Path(sys.argv[2])), separators=(",", ":")))
