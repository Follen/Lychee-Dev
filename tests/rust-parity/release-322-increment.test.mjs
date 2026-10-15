// Exact source/increment custody and declared coverage. No Rust/Lua/game execution.
import test from 'node:test';
import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {readFileSync} from 'node:fs';
import {resolve} from 'node:path';
const root = resolve(import.meta.dirname, '../..');
const read = path => readFileSync(resolve(root, path));
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const git = (...args) => execFileSync('git', ['--no-replace-objects', '-C', root, ...args], {timeout: 20000, maxBuffer: 4 << 20});
const pin = '0bec60e4406f5d5dc155ee25cef44f9a710e37e8';
const manifestBytes = read('tests/rust-parity/go-3.2.2-increment-oracle.json');
const manifest = JSON.parse(manifestBytes);
const FROZEN_MANIFEST_SHA256 = '5fa4b3ed2f600bfe1b1ce1fe367972c8f47760c53843c88cb9b1d28940252701';
const blob = (commit, path) => git('cat-file', 'blob', `${commit}:${path}`);

test('322 separately authenticates the fixed tag, business commit and complete increment', () => {
  assert.equal(hash(manifestBytes), FROZEN_MANIFEST_SHA256);
  assert.equal(manifest.current.commit, pin);
  assert.equal(manifest.previous.commit, '10b561db5706702cd42e047eaeed4ccb5e825351');
  assert.equal(manifest.current.tagObject, '97890b267a620a166889db0934e57a826d72a872');
  assert.equal(git('rev-parse', `${manifest.current.tagObject}^{}`).toString().trim(), pin);
  git('merge-base', '--is-ancestor', manifest.businessCommit, pin);
  assert.deepEqual(git('diff', '--name-only', manifest.previous.commit, pin).toString().trim().split('\n'), manifest.changedFiles);
  for (const row of manifest.sources) for (const [key, commit] of [['current', pin], ['previous', manifest.previous.commit]]) {
    const expected = row[key];
    if (!expected) { assert.throws(() => blob(commit, row.path)); continue; }
    const bytes = blob(commit, row.path);
    assert.equal(bytes.length, expected.bytes, row.path); assert.equal(hash(bytes), expected.sha256, row.path);
    assert.equal(createHash('sha1').update(`blob ${bytes.length}\0`).update(bytes).digest('hex'), expected.gitBlob, row.path);
  }
  assert.match(manifest.current.releaseArtifact, /^NOT_VERIFIED/);
});

test('all previous evidence keeps its original bytes and identity', () => {
  assert.equal(manifest.historicalFiles.length, 150);
  for (const row of manifest.historicalFiles) assert.equal(hash(read(row.path)), row.sha256, row.path);
  assert.equal(JSON.parse(read('tests/rust-parity/go-3.2.1.manifest.json')).oracle.commit, manifest.previous.commit);
  assert.equal(JSON.parse(read('tests/rust-parity/go-3.2.0.manifest.json')).oracle.commit, '3057a9666ab078270b63c7f06de748069b0d1f4b');
});

test('current product is exactly the frozen increment on its authenticated Rust base', () => {
  assert.equal(manifest.productChanges.length, 20);
  for (const row of manifest.persistenceCompatibility.sources) {
    const bytes = blob(row.commit, row.path);
    assert.equal(bytes.length, row.bytes, row.path); assert.equal(hash(bytes), row.sha256, row.path);
  }
  for (const row of manifest.productChanges) {
    let current = read(row.path);
    // This generated fixture's sole release string follows the independently
    // approved candidate. Preserve and authenticate the original 322 record.
    if (row.path === 'tests/addon/env.lua') {
      const text = current.toString();
      assert.equal(text.split('return "3.5.0"').length, 2);
      current = Buffer.from(text.replace('return "3.5.0"', 'return "3.1.2"'));
    }
    if (row.path === 'crates/lycheedev-live/src/go_mailbox_v1/input.rs') {
      const text = current.toString();
      const anchor = "}\n\nimpl<'de> Deserialize<'de> for InputObservation {";
      assert.equal((text.match(/fn is_false\(/g) ?? []).length, 1);
      assert.equal(text.split(anchor).length, 2);
      // 351's real E0428 came from this duplicate helper. Authenticate the
      // exact one-line removal against the immutable failed candidate.
      current = Buffer.from(text.replace(anchor,
        "}\nfn is_false(value: &bool) -> bool { !*value }\n\nimpl<'de> Deserialize<'de> for InputObservation {"));
    }
    assert.equal(hash(current), row.sha256, row.path);
    if (row.beforeSHA256 !== null) assert.equal(hash(blob(manifest.candidateBaseCommit, row.path)), row.beforeSHA256, row.path);
  }
  for (const path of ['release/version.json','packages/npm/lycheedev/package.json','packages/npm/lycheedev/package-lock.json','release/npm-channel.json']) {
    const before = JSON.parse(blob(manifest.candidateBaseCommit, path));
    const actual = JSON.parse(read(path));
    assert.equal(actual.version, '3.5.0', path);
    actual.version = before.version;
    if (path.endsWith('package-lock.json')) {
      assert.equal(actual.packages[''].version, '3.5.0');
      actual.packages[''].version = before.packages[''].version;
    }
    if (path === 'release/npm-channel.json') {
      assert.equal(actual.distTag, 'beta'); actual.distTag = before.distTag;
    }
    assert.deepEqual(actual, before, path);
  }
});

test('322 Lua and startup are upstream bytes, including failure and disabled-bridge coverage', () => {
  for (const path of ['addon/Core/Compat.lua','addon/Bridge/SlotRuntime.lua','addon/Bridge/InputState.lua','tests/addon/t_slot_enable.lua','skills/lycheedev/references/live-startup.md']) assert.deepEqual(read(path), blob(pin, path), path);
  const fixture = read('tests/addon/t_slot_enable.lua').toString();
  assert.equal(fixture.trimEnd().split('\n').length, 128);
  for (const text of ['queries==503 and saves==1 and loads==0','character==guid','queries==499','"read","enable","verify","save","unavailable"','saves==2 and #writes==1','enabled[1] and enabled[2]==false and saves==1','Env.framesCreated==frames and ns.Mailbox==nil']) assert(fixture.includes(text), text);
});

test('322 runtime tests are declared, wired and explicitly unexecuted here', () => {
  const rootModule = read('crates/lycheedev-live/src/go_mailbox_v1/mod.rs').toString();
  assert(rootModule.includes('mod slot_reload_tests;'));
  const source = read('crates/lycheedev-live/src/go_mailbox_v1/slot_reload_tests.rs').toString();
  const native = read('crates/lycheedev-live/src/go_mailbox_v1/native_backend/tests.rs').toString();
  const names = [...(source + native).matchAll(/fn (go_322_[a-z0-9_]+)\(/g)].map(m => m[1]);
  assert.equal(names.length, 21); assert.equal(new Set(names).size, 21);
  for (const value of Object.values(manifest.execution)) assert.equal(value, 'NOT_RUN');
  for (const text of ['DurableChannelJournal','journal.load(&OperationControl::default())','original-reload','slot_enable_reload_intent','runtime_changed_during_recovery','live.channel_budget_clock_rollback']) assert(source.includes(text), text);
});

test('fresh pre-bind and invoke gates preserve ordering and never gate reload itself', () => {
  const driver = read('crates/lycheedev-live/src/go_mailbox_v1/driver.rs').toString();
  const connect = driver.slice(driver.indexOf('    pub fn connect('), driver.indexOf('    pub fn prepare_request('));
  assert(connect.indexOf('self.observe_input(') < connect.indexOf('self.begin('));
  const tx = read('crates/lycheedev-live/src/go_mailbox_v1/transactions.rs').toString();
  const submit = tx.slice(tx.indexOf('    pub(crate) fn submit_input('), tx.indexOf('    pub(super) fn observe_input('));
  assert(submit.indexOf('return Ok(())') < submit.indexOf('self.observe_input('));
  assert.match(submit, /kind == InputKind::Invoke && observation.slot_reload_required/);
  assert(submit.indexOf('self.request_slot_reload(') < submit.indexOf('self.perform('));
  const native = read('crates/lycheedev-live/src/go_mailbox_v1/native_backend.rs').toString();
  assert.match(native, /!s.same_input_profile\(sample\)/);
  assert.match(native, /action.kind == InputKind::Invoke && s.slot_reload_required/);
  assert.match(read('crates/lycheedev-live/src/go_mailbox_v1/reload.rs').toString(), /if progress\(&err\) \{ continue; \}/);
});
