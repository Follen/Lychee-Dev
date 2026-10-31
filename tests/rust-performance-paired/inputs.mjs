import assert from 'node:assert/strict';
import {lstatSync, readFileSync, readdirSync} from 'node:fs';
import {join, resolve} from 'node:path';
import {hashFile} from '../../tools/rust-build.mjs';
import {readTgz} from '../../tools/tgz.mjs';
import {sha256} from '../rust-source-full-corpus/assertions.mjs';
import {GO_COMMIT, GO_VERSION} from './contract.mjs';

// The repository never builds Go. The fixed Go 3.2.4 reference product is an
// external input prepared once outside the repository; only its explicit path
// and static byte identity are consumed here.
export const GO_REFERENCE = Object.freeze({
  schema: 'lycheedev.fixed-go-performance-artifact.v1',
  commit: GO_COMMIT,
  version: GO_VERSION,
});
export function safeRelative(path) {
  assert(typeof path === 'string' && path.length > 0 && !path.includes('\\'));
  assert(path.split('/').every(p => p && p !== '.' && p !== '..' && !/[:\x00-\x1f]/.test(p)), 'unsafe package path');
  return path;
}
export function inventory(directory, {maxFiles = 20000, maxBytes = 512 << 20} = {}) {
  const rows = []; let total = 0;
  function visit(path, name) {
    const stat = lstatSync(path); assert(!stat.isSymbolicLink(), 'input symlink/reparse rejected');
    if (stat.isDirectory()) for (const child of readdirSync(path).sort()) visit(join(path, child), name ? `${name}/${child}` : child);
    else {
      assert(stat.isFile() && rows.length < maxFiles && (total += stat.size) <= maxBytes, 'input inventory budget/type');
      rows.push({path: name, bytes: stat.size, sha256: hashFile(path)});
    }
  }
  visit(directory, ''); return {rows, bytes: total, sha256: sha256(Buffer.from(JSON.stringify(rows)))};
}
// Authenticate one explicitly supplied external fixed Go package directory.
// Its release.json identities and every package byte are re-derived from the
// supplied directory; the Go product itself is never rebuilt or inspected by a
// Go toolchain here. The Lua runtime payload is part of that opaque external
// package and carries no repository-side assumption.
export function verifyFixedGoPackage(packageRoot, {commit = GO_COMMIT, version = GO_VERSION} = {}) {
  packageRoot = resolve(packageRoot);
  const manifest = JSON.parse(readFileSync(join(packageRoot, 'release.json')));
  assert.equal(manifest.schema, 'lycheedev.release.v1'); assert.equal(manifest.version, version); assert.equal(manifest.commit, commit);
  const binary = manifest.binaries?.['windows-amd64']; assert.equal(binary?.path, 'native/windows-amd64/lycheedev.exe');
  const all = inventory(packageRoot), byName = new Map(all.rows.map(row => [row.path, row]));
  const check = (row, prefix = '') => { const actual = byName.get(prefix + safeRelative(row.path)); assert(actual && actual.bytes === row.bytes && actual.sha256 === row.sha256, `package integrity ${prefix}${row.path}`); };
  check(binary); assert(Array.isArray(manifest.resources) && manifest.resources.length > 0);
  const seen = new Set();
  for (const row of manifest.resources) { assert(!seen.has(row.path), 'duplicate resource'); seen.add(row.path); check(row, 'payload/'); }
  for (const row of all.rows.filter(row => row.path.startsWith('payload/'))) assert(seen.has(row.path.slice(8)), 'unlisted payload file');
  const binaryPath = join(packageRoot, safeRelative(binary.path));
  assert.equal(lstatSync(binaryPath).size, binary.bytes); assert.equal(hashFile(binaryPath), binary.sha256);
  return {root: packageRoot, commit, version, binary: binaryPath, binaryBytes: binary.bytes, binarySHA256: binary.sha256,
    manifestSHA256: hashFile(join(packageRoot, 'release.json')), packageSHA256: all.sha256, packageBytes: all.bytes};
}

// Authenticate the external fixed Go 3.2.4 reference artifact directory:
// artifact.json + dev-npm-stage/ + the matching private npm tarball.
export function verifyGoReferenceArtifact(referenceRoot) {
  referenceRoot = resolve(referenceRoot);
  const artifactPath = join(referenceRoot, 'artifact.json');
  const artifact = JSON.parse(readFileSync(artifactPath));
  assert.equal(artifact.schema, GO_REFERENCE.schema, 'external Go reference artifact schema');
  assert.equal(artifact.commit, GO_COMMIT, 'external Go reference commit');
  assert.equal(artifact.privateDevelopmentPackage, true); assert.equal(artifact.publishEligible, false);
  const packageRoot = join(referenceRoot, 'dev-npm-stage');
  const packageFacts = verifyFixedGoPackage(packageRoot);
  const tarballPath = join(referenceRoot, `lycheedev-local-${GO_VERSION}.tgz`);
  const tarball = lstatSync(tarballPath); assert(tarball.isFile());
  const tarballBytes = readFileSync(tarballPath), tarballSHA256 = sha256(tarballBytes);
  assert.equal(tarballBytes.length, tarball.size);
  assert.equal(tarballSHA256, artifact.tarballSHA256, 'external Go reference tarball changed');
  // The explicit tarball and the explicit dev-npm-stage directory must be the
  // same package: release.json and the native executable bytes are compared.
  const entries = new Map();
  for (const entry of readTgz(tarballBytes)) if (entry.type === '0') entries.set(entry.name, entry.bytes);
  for (const [relativePath, tarName] of [['release.json', 'package/release.json'], ['native/windows-amd64/lycheedev.exe', 'package/native/windows-amd64/lycheedev.exe']]) {
    const packaged = entries.get(tarName); assert(packaged, `external Go reference tarball missing ${tarName}`);
    const onDisk = readFileSync(join(packageRoot, relativePath));
    assert.equal(packaged.length, onDisk.length, `external Go reference tarball ${relativePath} bytes`);
    assert.equal(sha256(packaged), sha256(onDisk), `external Go reference tarball ${relativePath} identity`);
  }
  // The artifact record binds exactly the supplied package bytes; nothing is
  // taken from a repository-internal Go build, source checkout or toolchain.
  assert.equal(artifact.package?.binarySHA256, packageFacts.binarySHA256, 'external Go reference binary identity');
  assert.equal(artifact.package?.manifestSHA256, packageFacts.manifestSHA256, 'external Go reference manifest identity');
  assert.equal(artifact.package?.packageSHA256, packageFacts.packageSHA256, 'external Go reference package identity');
  return {...packageFacts, schema: GO_REFERENCE.schema, referenceRoot, artifactPath, artifactSHA256: hashFile(artifactPath),
    tarballPath, tarballSHA256, privateDevelopmentPackage: true, publishEligible: false,
    sourceCleanBeforeAndAfter: artifact.sourceCleanBeforeAndAfter === true, toolchain: artifact.toolchain,
    optimized: artifact.build?.optimized === true, buildMetadataSHA256: artifact.build?.metadataSHA256};
}
