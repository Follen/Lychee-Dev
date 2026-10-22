// Self-contained fixed inputs and candidate-package identity checks for the
// current Source native acceptance runner. Replaces the retired
// rust-skill-usage migration reconciliation inputs (workflow/baseline fixtures)
// with the fixed external Go reference identity, the retained Retail native
// callsite and the current product package/Skill bytes.
import assert from 'node:assert/strict';
import {lstatSync, readFileSync, readdirSync, realpathSync} from 'node:fs';
import {isAbsolute, join, relative, resolve, sep} from 'node:path';
import {createHash} from 'node:crypto';
import {hashFile} from '../../tools/rust-build.mjs';

// Fixed external Go 3.2.4 reference identity. This is an identity label for the
// current acceptance report; the repository never builds or inspects Go.
export const GO_CONTRACT = '29bdbdaff5f064ec212ff2064380b1ccac652eda';
export const GO_VERSION = '3.2.4';

// Fixed original Retail native callsite from the retained Source oracle.
export const NATIVE_CALLSITE = Object.freeze({
  path: 'Interface/AddOns/Blizzard_ActionBar/Shared/ActionButton.lua',
  line: 141,
  kind: 'call',
  target: 'GetActionButtonForID',
  sourceText: '\tlocal button = GetActionButtonForID(id);',
  fullFileSHA256: '54fb5625a5e8aa343cbbc0be805b23e2b3b329011e792448e6b732a4bebd8cba',
  site: Object.freeze({file: 'Interface/AddOns/Blizzard_ActionBar/Shared/ActionButton.lua', start: 3940, end: 3960}),
  siteText: 'GetActionButtonForID',
  scope: 'Native call source is the nearest syntax-authenticated callable; referenceExpression keeps the original callee token. Caller identity does not strengthen selected-value observations or completeness.',
  source: 'ActionButtonDown',
  referenceExpression: 'GetActionButtonForID',
  caller: Object.freeze({kind: 'named', name: 'ActionButtonDown', line: 136}),
});

const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');

export function within(root, name) {
  assert.equal(typeof name, 'string', 'relative file name');
  assert(name && !isAbsolute(name) && !name.includes('\\') && !name.includes(':') && !name.includes('\0'), 'safe relative file name');
  assert(name.split('/').every(part => part && part !== '.' && part !== '..'), 'safe path components');
  const base = realpathSync(root), path = resolve(base, name);
  const rel = relative(base, path);
  assert(rel && !isAbsolute(rel) && rel !== '..' && !rel.startsWith(`..${sep}`), 'contained path');
  let cursor = base;
  for (const part of name.split('/')) {
    cursor = join(cursor, part);
    assert(!lstatSync(cursor).isSymbolicLink(), 'no linked candidate path');
  }
  return path;
}
export function jsonFile(path, limit = 8 << 20) {
  const stat = lstatSync(path);
  assert(stat.isFile() && !stat.isSymbolicLink() && stat.size <= limit, 'bounded regular JSON input');
  return JSON.parse(readFileSync(path, 'utf8'));
}
export function inventory(root, {maxBytes = 512 << 20, maxFiles = 10000} = {}) {
  assert(lstatSync(root).isDirectory() && !lstatSync(root).isSymbolicLink(), 'regular input directory');
  const rows = []; let total = 0, entries = 0, bytes = 0;
  function visit(directory, prefix) {
    for (const entry of readdirSync(directory, {withFileTypes: true}).sort((a, b) => Buffer.compare(Buffer.from(a.name), Buffer.from(b.name)))) {
      const name = prefix ? `${prefix}/${entry.name}` : entry.name;
      assert(++entries <= maxFiles * 4, 'candidate entry budget');
      const path = within(root, name), stat = lstatSync(path);
      assert(!entry.isSymbolicLink(), 'no candidate symlinks');
      if (stat.isDirectory()) { assert(name.split('/').length <= 24, 'candidate directory depth'); visit(path, name); }
      else {
        assert(stat.isFile() && ++total <= maxFiles, 'candidate regular-file count');
        rows.push({path: name, bytes: stat.size, sha256: hashFile(path, 128 << 20)});
        bytes += stat.size; assert(bytes <= maxBytes, 'candidate total byte budget');
      }
    }
  }
  visit(root, ''); return rows;
}
export const inventoryHash = rows => sha256(Buffer.from(JSON.stringify(rows)));

// Authenticate the assembled development candidate package. This checks current
// release identity, payload resources, compiler/launcher bytes and that the
// packaged Skill exactly equals the sealed current product Skill source.
export function verifyCandidate(root, reportPath, artifact) {
  const report = jsonFile(reportPath, 1 << 20);
  assert.equal(report.schema, 'lycheedev.rust-npm-smoke.v1');
  assert(['assembled', 'partial_passed'].includes(report.state), 'successful candidate assembly');
  assert.equal(report.developmentOnly, true, 'unpublished candidate only');
  assert.equal(report.binarySHA256, artifact.binarySHA256, 'candidate and sealed EXE identity');
  assert.equal(report.sourceTreeSHA256, artifact.inputHashes?.sourceTree, 'candidate and sealed source identity');
  const packageRelative = relative(realpathSync(root), resolve(report.packageRoot)).split(sep).join('/');
  const packageRoot = within(root, packageRelative);
  const manifestPath = within(packageRoot, 'release.json');
  assert.equal(hashFile(manifestPath), report.releaseManifestSHA256, 'candidate report binds release manifest');
  const manifest = jsonFile(manifestPath), pkg = jsonFile(within(packageRoot, 'package.json'));
  assert.equal(manifest.schema, 'lycheedev.release.v1');
  assert.equal(manifest.runtime, 'rust');
  assert.equal(manifest.version, pkg.version); assert.equal(pkg.private, true);
  assert.equal(manifest.commit, report.commit);
  assert.equal(manifest.binaries?.['windows-amd64']?.path, 'native/windows-amd64/lycheedev.exe');
  const binary = within(packageRoot, manifest.binaries['windows-amd64'].path);
  assert.equal(hashFile(binary), artifact.binarySHA256);
  assert.equal(lstatSync(binary).size, manifest.binaries['windows-amd64'].bytes);
  assert.equal(manifest.binaries['windows-amd64'].sha256, artifact.binarySHA256);
  assert(Array.isArray(manifest.resources) && manifest.resources.length <= 10000, 'bounded resource inventory');
  assert.equal(sha256(Buffer.from(JSON.stringify(manifest.resources))), report.resourceInventorySHA256);
  const seen = new Set();
  for (const resource of manifest.resources) {
    assert(!seen.has(resource.path), 'unique resource path'); seen.add(resource.path);
    const file = within(packageRoot, `payload/${resource.path}`);
    assert.equal(lstatSync(file).size, resource.bytes, `resource bytes: ${resource.path}`);
    assert.equal(hashFile(file, 64 << 20), resource.sha256, `resource digest: ${resource.path}`);
  }
  // Current packaged Skill identity: the sealed candidate source is authoritative.
  // The retired fixed Go Skill file list and reviewed migration baselines are not
  // current acceptance inputs.
  const skillRoot = within(packageRoot, 'payload/skill');
  const packagedSkill = inventory(skillRoot, {maxBytes: 4 << 20, maxFiles: 100});
  assert.deepEqual(packagedSkill, inventory(within(root, 'skills/lycheedev'), {maxBytes: 4 << 20, maxFiles: 100}), 'entire packaged Skill equals sealed candidate source');
  for (const row of packagedSkill) assert(seen.has(`skill/${row.path}`), `unmanifested packaged Skill resource: ${row.path}`);
  assert.equal(hashFile(within(packageRoot, 'payload/tool/luac/luac.exe')), report.compilerSHA256);
  assert.equal(hashFile(within(packageRoot, 'bin/lycheedev.mjs')), hashFile(within(root, 'packages/npm/lycheedev/bin/lycheedev.mjs')), 'packaged launcher is selected candidate launcher');
  // Do not silently test a stale or reduced Skill while evaluating today's tree.
  for (const row of inventory(within(root, 'skills/lycheedev'), {maxBytes: 4 << 20, maxFiles: 100})) {
    assert.equal(hashFile(within(packageRoot, `payload/skill/${row.path}`)), row.sha256, `packaged Skill source: ${row.path}`);
  }
  const files = inventory(packageRoot);
  const payload = files.filter(row => row.path.startsWith('payload/'));
  assert.equal(payload.length, seen.size, 'no unmanifested payload resources');
  return {packageRoot, report, manifest, files, packageSHA256: inventoryHash(files), reportSHA256: hashFile(reportPath)};
}
