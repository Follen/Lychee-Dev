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
const typedDeltaBytes = read('tests/rust-parity/typed-live-outcome-custody.json');
const TYPED_DELTA_SHA256 = '8b83dee588224707f5bfdf746920720432dc51515753e543eb686db288c67d83';
function restoreTypedOutcome(bytes, row) {
  assert.equal(hash(bytes), row.candidateSHA256, row.path);
  let restored = bytes;
  let previous = bytes.length;
  for (const edit of [...row.edits].reverse()) {
    assert(Number.isSafeInteger(edit.candidateStart) && edit.candidateStart >= 0);
    assert(Number.isSafeInteger(edit.candidateEnd) && edit.candidateEnd >= edit.candidateStart && edit.candidateEnd <= previous);
    assert.deepEqual(restored.subarray(edit.candidateStart, edit.candidateEnd), Buffer.from(edit.candidateText));
    restored = Buffer.concat([restored.subarray(0, edit.candidateStart), Buffer.from(edit.originalText), restored.subarray(edit.candidateEnd)]);
    previous = edit.candidateStart;
  }
  assert.equal(hash(restored), row.originalSHA256, row.path);
  return restored;
}

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
  const deltaBytes = read('tests/rust-parity/release-350-static-consumer-delta.json');
  assert.equal(hash(deltaBytes), 'cc52d3a8383a737f98c9b70cc81c312717caca3538107e22d0a761e6cb6e965d');
  const delta = JSON.parse(deltaBytes);
  assert.equal(delta.path, 'tests/rust-parity/release-321-increment.test.mjs');
  for (const row of manifest.historicalFiles) {
    if (row.path === 'tests/rust-parity/release-321-guidance.test.mjs') {
      const original = read('tests/rust-parity/historical/release-321-guidance.test.mjs.txt');
      assert.equal(hash(original), row.sha256, 'original guidance checker remains immutable');
      const adapted = original.toString()
        .replace('resolve(root, path)', "resolve(root, 'tests/rust-skill-usage/baselines/go3.2.3/references/live-recovery.md')")
        .replace('Skill keyboard recovery paragraph is byte-for-byte fixed Go 3.2.1',
          'historical Skill fixture keyboard recovery paragraph is byte-for-byte fixed Go 3.2.1');
      assert.equal(read(row.path).toString(), adapted, 'only historical fixture routing and test label change');
      continue;
    }
    if (row.path !== delta.path) { assert.equal(hash(read(row.path)), row.sha256, row.path); continue; }
    assert.equal(delta.beforeSHA256, row.sha256);
    const historical = read(delta.historicalPath);
    assert.equal(hash(historical), row.sha256);
    assert.deepEqual(historical, blob(delta.baseCommit, delta.path));
    const current = read(row.path);
    assert.equal(hash(current), delta.candidateSHA256);
    let restored = current.toString();
    for (const edit of delta.edits) {
      assert.equal(restored.split(edit.candidateText).length, 2);
      restored = restored.replace(edit.candidateText, edit.beforeText);
    }
    assert.equal(hash(Buffer.from(restored)), row.sha256);
    assert.equal(restored, historical.toString());
  }
  assert.equal(JSON.parse(read('tests/rust-parity/go-3.2.1.manifest.json')).oracle.commit, manifest.previous.commit);
  assert.equal(JSON.parse(read('tests/rust-parity/go-3.2.0.manifest.json')).oracle.commit, '3057a9666ab078270b63c7f06de748069b0d1f4b');
});

// The original candidate is an immutable historical fact. Later legitimate
// refactors and reviewed Skill text are not required to retain its source bytes.
const HISTORICAL_CANDIDATE = '351fcfdaceb4c064179bad3839b5dc1e01aa2043';
const HISTORICAL_SUCCESSOR = '0002ce02bec5fb7c9c31ae40fd96eadf0da17dc6';
test('historical 322 candidate retains all frozen increment bytes and predecessor identities', () => {
  assert.equal(manifest.productChanges.length, 20);
  for (const row of manifest.persistenceCompatibility.sources) {
    const bytes = blob(row.commit, row.path);
    assert.equal(bytes.length, row.bytes, row.path); assert.equal(hash(bytes), row.sha256, row.path);
  }
  for (const row of manifest.productChanges) {
    const original = blob(HISTORICAL_CANDIDATE, row.path);
    assert.equal(original.length, row.bytes, row.path); assert.equal(hash(original), row.sha256, row.path);
    if (row.beforeSHA256 !== null) assert.equal(hash(blob(manifest.candidateBaseCommit, row.path)), row.beforeSHA256, row.path);
  }
});

test('fixed historical successors retain the independently authenticated correction evidence', () => {
  assert.equal(hash(typedDeltaBytes), TYPED_DELTA_SHA256);
  const typedDelta = JSON.parse(typedDeltaBytes);
  assert.equal(typedDelta.baseCommit, '913112a6ce628812f824513aac853ccd088fd265');
  assert.equal(typedDelta.candidateCommit, 'e7341e3902216de38654eaeefe8e0ce42ec62f83');
  assert.deepEqual(typedDelta.sources.map(row => row.path), ['crates/lycheedev-live/src/go_mailbox_v1/activation_service.rs', 'crates/lycheedev-live/src/go_mailbox_v1/activation_tests.rs']);
  for (const row of typedDelta.sources) {
    assert.equal(row.originalSHA256, manifest.productChanges.find(part => part.path === row.path).sha256);
    const restored = restoreTypedOutcome(blob(typedDelta.candidateCommit, row.path), row);
    assert.deepEqual(restored, blob(typedDelta.baseCommit, row.path));
    assert.deepEqual(restored, blob(HISTORICAL_CANDIDATE, row.path));
  }
  const input = 'crates/lycheedev-live/src/go_mailbox_v1/input.rs';
  const text = blob(HISTORICAL_SUCCESSOR, input).toString();
  const anchor = "}\n\nimpl<'de> Deserialize<'de> for InputObservation {";
  assert.equal((text.match(/fn is_false\(/g) ?? []).length, 1);
  assert.equal(text.split(anchor).length, 2);
  // Preserve the real historical E0428 duplicate-helper failure and its exact
  // correction. Neither frozen snapshot claims today's Rust execution passed.
  assert.deepEqual(Buffer.from(text.replace(anchor,
    "}\nfn is_false(value: &bool) -> bool { !*value }\n\nimpl<'de> Deserialize<'de> for InputObservation {")), blob(HISTORICAL_CANDIDATE, input));
  const correction = JSON.parse(read('tests/rust-parity/b16-mailbox-fixture-corrections.json'));
  assert.equal(correction.path, 'crates/lycheedev-live/src/go_mailbox_v1/driver_tests.rs');
  assert.equal(correction.originalSHA256, manifest.productChanges.find(row => row.path === correction.path).sha256);
  let restored = blob(HISTORICAL_SUCCESSOR, correction.path).toString();
  assert.equal(hash(Buffer.from(restored)), correction.candidateSHA256);
  for (const part of correction.replacements) {
    assert.equal(restored.split(part.replacement).length, 2);
    restored = restored.replace(part.replacement, part.original);
  }
  assert.equal(hash(Buffer.from(restored)), correction.originalSHA256);
  assert.deepEqual(Buffer.from(restored), blob(HISTORICAL_CANDIDATE, correction.path));
  const env = blob(HISTORICAL_SUCCESSOR, 'tests/addon/env.lua').toString();
  assert.equal(env.split('return "3.5.0"').length, 2);
  assert.deepEqual(Buffer.from(env.replace('return "3.5.0"', 'return "3.2.3"')),
    blob('3766f9cacb52e93a803a626187e2a1d54a445468', 'tests/addon/env.lua'));
});

test('current candidate keeps only the approved release version and channel overlay', () => {
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
  for (const path of ['addon/Core/Compat.lua','addon/Bridge/SlotRuntime.lua','addon/Bridge/InputState.lua','tests/addon/t_slot_enable.lua']) assert.deepEqual(read(path), blob(pin, path), path);
  assert.deepEqual(read('tests/rust-skill-usage/baselines/go3.2.3/references/live-startup.md'), blob(pin, 'skills/lycheedev/references/live-startup.md'), 'historical startup Skill fixture');
  const env = read('tests/addon/env.lua').toString();
  assert.equal(env.split('return "3.5.0"').length, 2);
  assert.deepEqual(Buffer.from(env.replace('return "3.5.0"', 'return "3.2.3"')),
    blob('3766f9cacb52e93a803a626187e2a1d54a445468', 'tests/addon/env.lua'), 'current Lua environment retains the fixed Go323 version overlay');
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

function sourceBlock(source, start, end) {
  const first = source.indexOf(start), last = source.indexOf(end);
  assert(first >= 0 && last > first, 'existing ordered function boundaries');
  return source.slice(first, last);
}
function assertOrder(source, before, after) {
  const first = source.indexOf(before), last = source.indexOf(after);
  assert(first >= 0 && last >= 0, 'both ordering anchors must exist');
  assert(first < last, 'required gate ordering');
}
function assertFreshInputGates(driver, tx) {
  const connect = sourceBlock(driver, '    pub fn connect(', '    pub fn prepare_request(');
  assertOrder(connect, 'self.observe_input(', 'self.begin(');
  const submit = sourceBlock(tx, '    pub(crate) fn submit_input(', '    pub(super) fn observe_input(');
  assertOrder(submit, 'return Ok(())', 'self.observe_input(');
  assert.match(submit, /kind == InputKind::Invoke && observation.slot_reload_required/);
  assertOrder(submit, 'self.request_slot_reload(', 'self.perform(');
}
test('fresh pre-bind and invoke gates preserve ordering and never gate reload itself', () => {
  assertFreshInputGates(read('crates/lycheedev-live/src/go_mailbox_v1/driver.rs').toString(),
    read('crates/lycheedev-live/src/go_mailbox_v1/transactions.rs').toString());
  const native = read('crates/lycheedev-live/src/go_mailbox_v1/native_backend.rs').toString();
  assert.match(native, /!s.same_input_profile\(sample\)/);
  assert.match(native, /action.kind == InputKind::Invoke && s.slot_reload_required/);
  assert.match(read('crates/lycheedev-live/src/go_mailbox_v1/reload.rs').toString(), /if progress\(&err\) \{ continue; \}/);
});
test('current gate checker rejects missing boundaries and either ordering anchor', () => {
  const driver = read('crates/lycheedev-live/src/go_mailbox_v1/driver.rs').toString();
  const tx = read('crates/lycheedev-live/src/go_mailbox_v1/transactions.rs').toString();
  assertFreshInputGates(driver, tx);
  for (const marker of ['    pub fn connect(', '    pub fn prepare_request(', 'self.observe_input(', 'self.begin(']) {
    assert.throws(() => assertFreshInputGates(driver.replace(marker, ''), tx));
  }
  const submit = sourceBlock(tx, '    pub(crate) fn submit_input(', '    pub(super) fn observe_input(');
  for (const marker of ['return Ok(())', 'self.observe_input(', 'self.request_slot_reload(', 'self.perform(']) {
    assert.throws(() => assertFreshInputGates(driver, tx.replace(submit, submit.replace(marker, ''))));
  }
  for (const marker of ['    pub(crate) fn submit_input(', '    pub(super) fn observe_input(']) {
    assert.throws(() => assertFreshInputGates(driver, tx.replace(marker, '')));
  }
});

test('typed outcome custody rejects changed bytes, offsets and replacement text', () => {
  assert.equal(hash(typedDeltaBytes), TYPED_DELTA_SHA256);
  const delta = JSON.parse(typedDeltaBytes);
  for (const row of delta.sources) {
    const current = blob(delta.candidateCommit, row.path);
    const changed = Buffer.from(current); changed[0] ^= 1;
    assert.throws(() => restoreTypedOutcome(changed, row));
    const altered = structuredClone(row); altered.edits[0].originalText += 'x';
    assert.throws(() => restoreTypedOutcome(current, altered));
    const wrongOffset = structuredClone(row); wrongOffset.edits[0].candidateStart += 1;
    assert.throws(() => restoreTypedOutcome(current, wrongOffset));
  }
});
