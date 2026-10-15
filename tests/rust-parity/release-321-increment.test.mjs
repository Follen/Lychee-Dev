// Source-derived increment checks only. Passing does not establish execution parity.
import test from 'node:test';
import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {readFileSync, statSync, mkdtempSync, rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {dirname, join, resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import {extractRustProducts, parseLiteral} from './oracle.mjs';
import {collectAddonZipEntries, verifyAddonZip} from '../../tools/addon-package.mjs';
import {writeZip} from '../../tools/zip.mjs';
import {CANDIDATE_RELEASE_FIELDS, CANDIDATE_VERSION} from './rust-3.5.0-release-fields.mjs';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const read = path => { assert(statSync(join(root, path)).size < 2 << 20); return readFileSync(join(root, path)); };
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const manifest = JSON.parse(read('tests/rust-parity/go-3.2.1-increment-oracle.json'));
const git = (...args) => execFileSync('git', ['--no-replace-objects', '-C', root, ...args], {maxBuffer: 2 << 20, stdio: ['ignore', 'pipe', 'pipe']});
const source = (commit, path) => git('cat-file', 'blob', `${commit}:${path}`);
async function zipBytes(t, entries) {
  const dir = mkdtempSync(join(tmpdir(), 'lycheedev-321-package-'));
  t.after(() => rmSync(dir, {recursive: true}));
  const path = join(dir, 'fixture.zip');
  await writeZip(entries, path);
  return readFileSync(path);
}

test('increment authenticates the exact previous/current Git objects and changed file set', () => {
  const before = manifest.previous.commit, after = manifest.current.commit;
  assert.equal(before, '3057a9666ab078270b63c7f06de748069b0d1f4b');
  assert.equal(after, '10b561db5706702cd42e047eaeed4ccb5e825351');
  assert.deepEqual(git('diff', '--name-only', before, after).toString().trim().split('\n'), manifest.changedFiles);
  for (const row of manifest.sources) for (const [key, commit] of [['previous', before], ['current', after]]) {
    const expected = row[key];
    if (!expected) { assert.throws(() => source(commit, row.path)); continue; }
    const bytes = source(commit, row.path);
    assert.equal(bytes.length, expected.bytes, row.path);
    assert.equal(hash(bytes), expected.sha256, row.path);
    assert.equal(createHash('sha1').update(`blob ${bytes.length}\0`).update(bytes).digest('hex'), expected.gitBlob, row.path);
  }
});

test('previous 3.2.0 fixture evidence is retained without rebasing', () => {
  assert.equal(hash(read(manifest.historicalOracle.path)), manifest.historicalOracle.sha256);
  const unchanged = ['internal/command/command_contract.go', 'internal/live/channel/client_identity.go', 'internal/live/memory/lua_mailbox.go'];
  for (const path of unchanged) assert.deepEqual(source(manifest.previous.commit, path), source(manifest.current.commit, path), path);
});

test('canonical products match the fixed 3.2.1 catalog while verified baselines retain both Retail series', () => {
  const products = extractRustProducts(read('crates/lycheedev-core/src/client.rs').toString());
  assert.deepEqual(products.map(p => Object.fromEntries(Object.keys(manifest.products[0]).map(k => [k, p[k]]))), manifest.products);
  const text = read('crates/lycheedev-core/src/client/baselines.rs').toString();
  const block = text.match(/static VERIFIED_BASELINES: \[ClientBaseline; 7\] = \[([\s\S]*?)\n\];/);
  assert(block, 'review the literal baseline extractor if its syntax changes');
  const observed = [...block[1].matchAll(/ClientBaseline (\{[^{}]*\})/g)].map(match => ({...parseLiteral(match[1])}));
  assert.deepEqual(observed, manifest.clientBaselines);
  assert.equal(products[0].interface, 120105);
});

test('321 business addon bytes survive the approved independent Rust release overlay', () => {
  assert.deepEqual(read('addon/Core/ClientGate.lua'), source(manifest.current.commit, 'addon/Core/ClientGate.lua'));
  for (const [path, commit, historicalText] of [
    ['addon/Lychee Dev.toc', manifest.current.commit, '## Version: 3.2.1'],
    ['addon/Core/Runtime.lua', manifest.previous.commit, 'ns.Release = "3.2.0"'],
  ]) {
    const allowed = CANDIDATE_RELEASE_FIELDS.find(row => row.path === path);
    assert(allowed, path);
    const candidate = read(path);
    assert.deepEqual({bytes: candidate.length, sha256: hash(candidate)}, allowed.retained, path);
    const text = candidate.toString();
    assert.equal(text.split(allowed.retainedText).length, 2, path);
    const restored = Buffer.from(text.replace(allowed.retainedText, historicalText));
    const original = source(commit, path);
    assert.equal(hash(restored), hash(original), path);
    assert.deepEqual(restored, original, path);
  }
  assert.equal(JSON.parse(read('release/version.json')).version, CANDIDATE_VERSION);
});

test('slot generator declares new interfaces and retains the exact previous Retail migration list', () => {
  const text = read('crates/lycheedev-delivery/src/slots.rs').toString();
  assert.equal(text.match(/pub const INTERFACES: &str = "([^"]*)";/)[1], manifest.interfaces);
  assert.equal(text.match(/const PREVIOUS_RETAIL_INTERFACES: &str = "([^"]*)";/)[1], '120100, 50504, 38002, 16001, 11509, 20506');
  assert.match(text, /for interfaces in \[[\s\S]*?PREVIOUS_RETAIL_INTERFACES,[\s\S]*?PREVIOUS_MAILBOX_INTERFACES,[\s\S]*?PREVIOUS_INTERFACES,/);
});

test('package shape accepts seven interfaces and six catalog families, retaining the version gate', async t => {
  const {entries, tocs} = collectAddonZipEntries(root);
  assert.equal(tocs[0].parse.interface, manifest.interfaces);
  assert.equal(tocs[0].parse.loads.filter(p => p.startsWith('Modules\\Events\\CatalogData_')).length, manifest.eventCatalogFamilies);
  const zip = await zipBytes(t, entries);
  assert.deepEqual(verifyAddonZip(zip).violations, []);
  const version = verifyAddonZip(zip, {repoRoot: root});
  assert.equal(version.ok, true);
  assert.deepEqual(version.violations, []);
  // The old mismatch remains frozen evidence. Prove the current gate still
  // rejects an intentionally stale release rather than weakening validation.
  const path = 'Lychee Dev/Lychee Dev.toc';
  const stale = Buffer.from(entries.get(path).toString().replace(`## Version: ${CANDIDATE_VERSION}\n`, '## Version: 3.2.0\n'));
  assert.notDeepEqual(stale, entries.get(path));
  entries.set(path, stale);
  const rejected = verifyAddonZip(await zipBytes(t, entries), {repoRoot: root});
  assert.equal(rejected.ok, false);
  assert.deepEqual(rejected.violations, [`Lychee Dev/Lychee Dev.toc: ## Version: 3.2.0 != ${CANDIDATE_VERSION} (release/version.json)`]);
});

test('package rejects loss of either supported Retail interface', async t => {
  for (const removed of ['120105', '120100']) {
    const {entries} = collectAddonZipEntries(root);
    const path = 'Lychee Dev/Lychee Dev.toc';
    entries.set(path, Buffer.from(entries.get(path).toString().replace(`${removed}, `, '')));
    const report = verifyAddonZip(await zipBytes(t, entries));
    assert.equal(report.ok, false);
    assert(report.violations.some(text => text.includes(`Interface: missing ${removed}`)));
  }
});
