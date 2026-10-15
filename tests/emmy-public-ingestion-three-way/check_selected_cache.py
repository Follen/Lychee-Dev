"""Read-only proof that the reused Cargo sources equal official locked archives."""
import gzip
import hashlib
import io
import json
from pathlib import Path, PurePosixPath
import sys
import stat
import tarfile
import tomllib


def digest(data):
    return hashlib.sha256(data).hexdigest()


def no_reparse(path):
    for parent in [path, *path.parents]:
        info = parent.lstat()
        if stat.S_ISLNK(info.st_mode) or getattr(info, "st_file_attributes", 0) & 0x400:
            raise ValueError("cache.reparse_path")


def bounded(path, limit):
    no_reparse(path)
    if not path.is_file() or path.stat().st_size > limit:
        raise ValueError(f"cache.invalid_file:{path.name}")
    with path.open("rb") as stream:
        data = stream.read(limit + 1)
    if len(data) > limit:
        raise ValueError("cache.file_budget")
    return data


def verify(lock, home):
    packages = tomllib.loads(bounded(lock, 256 << 10).decode())["package"]
    if len(packages) > 256:
        raise ValueError("cache.package_budget")
    registry = "index.crates.io-1949cf8c6b5b557f"
    rows = []
    local_packages = {"emmylua_code_analysis": "0.30.0", "emmylua_parser": "0.30.0", "emmylua_diagnostic_macro": "0.5.0",
                      "lycheedev-emmy-adapter": "3.5.0", "lycheedev-binding-contracts": "3.5.0"}
    local_seen = set()
    for package in packages:
        if "source" not in package:
            name = package["name"]
            if name not in local_packages or package["version"] != local_packages[name] or name in local_seen or "checksum" in package:
                raise ValueError("cache.unexpected_local_package")
            local_seen.add(name)
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
                    no_reparse(parent)
                actual = bounded(target, 16 << 20)
                if actual != data:
                    raise ValueError(f"cache.source_bytes:{prefix}/{name}")
                expected[name] = digest(actual)
        actual_files, actual_dirs = set(), set()
        for index, path in enumerate(source.rglob("*")):
            no_reparse(path)
            if index >= 10000:
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
    if local_seen != set(local_packages):
        raise ValueError("cache.owned_lock_inventory")
    if not rows:
        raise ValueError("cache.empty_closure")
    return rows




def verify_selected(lock, home, tree, evidence):
    import re
    original = tomllib.loads(bounded(lock, 256 << 10).decode())
    selected = set()
    for line in bounded(tree, 2 << 20).decode().splitlines():
        match = re.match(r"^([A-Za-z0-9_-]+) v([^ ]+)(?: |$)", line)
        if not match:
            raise ValueError("cache.unrecognized_cargo_tree_line")
        selected.add((match[1], match[2]))
    if not selected or len(selected) > 256:
        raise ValueError("cache.selected_package_budget")
    packages = [p for p in original["package"] if (p["name"], p["version"]) in selected]
    if len(packages) != len(selected):
        raise ValueError("cache.selected_not_in_original_lock")
    # Copy verbatim lockfile blocks, no dependency resolution or version edits.
    text = bounded(lock, 256 << 10).decode()
    blocks = text.split("[[package]]")
    chosen = []
    for block in blocks[1:]:
        p = tomllib.loads("[[package]]"+block)["package"][0]
        if (p["name"],p["version"]) in selected:
            chosen.append("[[package]]"+block)
    subset = evidence / "selected-packages.lock"
    subset.write_bytes((blocks[0]+"".join(chosen)).encode("utf-8"))
    rows = verify(subset, home)
    local = sorted(p["name"] for p in packages if "source" not in p)
    if local != ["emmylua_code_analysis", "emmylua_diagnostic_macro", "emmylua_parser", "lycheedev-binding-contracts", "lycheedev-emmy-adapter"]:
        raise ValueError("cache.local_package_inventory")
    return {"selected":len(selected), "registry":len(rows), "local":local, "packages":rows}

if __name__ == "__main__":
    print(json.dumps(verify_selected(Path(sys.argv[1]), Path(sys.argv[2]), Path(sys.argv[3]), Path(sys.argv[4])), separators=(",", ":")))
