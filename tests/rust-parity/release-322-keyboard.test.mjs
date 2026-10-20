// Go 3.2.2 composition of the unchanged 3.2.1 keyboard tests and exact reviewed increment.
// The old test/map/oracle files remain immutable. Lexical/source custody checks only. These do not execute Rust or Windows hooks.
import test from 'node:test';
import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {readFileSync, statSync} from 'node:fs';
import {dirname, join, resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import {inspectSource} from '../../tools/rust-unsafe-boundaries.mjs';
import {RETAINED_SUCCESSORS, assertSuccessorCustody, assertSuccessorSafety} from './keyboard-retained-custody.mjs';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const read = p => { assert(statSync(join(root, p)).size < 2 << 20); return readFileSync(join(root, p), 'utf8'); };
const sha256 = s => createHash('sha256').update(s).digest('hex');
const windows = 'crates/lycheedev-windows/src/mailbox_input';
const live = 'crates/lycheedev-live/src/go_mailbox_v1';
const oracle = JSON.parse(read('tests/rust-parity/go-3.2.1-increment-oracle.json'));
const increment = JSON.parse(read('tests/rust-parity/go-3.2.2-increment-oracle.json'));
assert.equal(sha256(read('tests/rust-parity/go-3.2.2-increment-oracle.json')), '5fa4b3ed2f600bfe1b1ce1fe367972c8f47760c53843c88cb9b1d28940252701');
const source = path => execFileSync('git', ['--no-replace-objects', '-C', root, 'cat-file', 'blob', `${oracle.current.commit}:${path}`], {maxBuffer: 2 << 20, stdio: ['ignore', 'pipe', 'pipe']});

test('keyboard source oracle remains exact released Go 3.2.1, separate from historical results', () => {
  assert.equal(oracle.current.commit, '10b561db5706702cd42e047eaeed4ccb5e825351');
  assert.equal(oracle.previous.commit, '3057a9666ab078270b63c7f06de748069b0d1f4b');
  for (const path of [
    'internal/desktop/input.go', 'internal/desktop/input_native_windows.go',
    'internal/desktop/input_modifiers_test.go', 'internal/desktop/keyboard_activity_windows.go',
    'internal/desktop/keyboard_activity_windows_test.go', 'internal/desktop/mailbox_input_windows_test.go',
    'internal/live/channel/input_windows.go', 'internal/live/channel/native_windows.go',
    'internal/live/channel/keyboard_wait_test.go', 'docs/live-protocol.md',
    'skills/lycheedev/references/live-recovery.md',
  ]) {
    const row = oracle.sources.find(r => r.path === path);
    assert(row, path);
    assert.equal(sha256(source(path)), row.current.sha256, path);
  }
  assert.equal(sha256(read(oracle.historicalOracle.path)), oracle.historicalOracle.sha256);
});

test('pure readiness keeps all-key and exact 500ms timestamp-only state', () => {
  const text = read(`${windows}/keyboard_state.rs`);
  const inspected = inspectSource(text);
  assert(inspected.forbidden);
  assert.deepEqual(inspected.sites, []);
  assert.match(text, /QUIET_PERIOD: Duration = Duration::from_millis\(500\)/);
  assert.match(text, /for vk in 0x03\.\.0xff/);
  assert.match(text, /!\(0x04\.\.=0x07\)\.contains\(&vk\)/);
  const state = text.match(/struct KeyboardState \{([\s\S]*?)\n\}/)[1];
  assert.equal((state.match(/:/g) || []).length, 4);
  assert.match(state, /origin: Instant/);
  assert.match(state, /last_event_nanos: AtomicU64/);
  assert.doesNotMatch(state, /Vec|String|HashMap|BTree|vk|key_code/);
  for (const name of ['every_keyboard_vk_blocks_but_mouse_and_reserved_values_do_not', 'startup_hold_release_repeat_and_short_key_each_restart_half_second_quiet', 'stopped_or_invalid_clock_is_fail_closed']) assert.match(text, new RegExp(`fn ${name}\\(`));
});

test('two try-only readiness checks exclude active chord and emergency release', () => {
  const text = read(`${windows}/engine.rs`);
  assert(inspectSource(text).forbidden);
  assert.deepEqual(inspectSource(text).sites, []);
  assert.equal((text.match(/\.keyboard_ready\(\)/g) || []).length, 2);
  const run = text.slice(text.indexOf('    fn run('), text.indexOf('    fn key('));
  assert(run.indexOf('.keyboard_ready()') < run.indexOf('self.check()?'));
  const key = text.slice(text.indexOf('    fn key('), text.indexOf('    fn wait('));
  assert.match(key, /if !up && self\.receipt\.messages_queued == 0/);
  assert(key.indexOf('.keyboard_ready()') < key.indexOf('self.check()?'));
  assert(key.indexOf('self.check()?') < key.indexOf('.post('));
  assert(key.indexOf('.keyboard_ready()') < key.indexOf('.post('));
  const release = text.slice(text.indexOf('    fn release('));
  assert.doesNotMatch(release, /keyboard_ready|KeyboardActivity|QUIET_PERIOD/);
  assert.match(release, /self\.down\.drain\(\.\.\)\.rev\(\)/);
});

test('hook callback retains no event contents and lifecycle is bounded/event-driven', () => {
  const text = read(`${windows}/keyboard_activity.rs`);
  const callback = text.slice(text.indexOf('unsafe extern "system" fn keyboard_hook'), text.indexOf('fn os_error'));
  assert.match(callback, /state\.note\(Instant::now\(\)\)/);
  assert.match(callback, /CallNextHookEx\(None, code, message, data\)/);
  assert.doesNotMatch(callback, /KBDLLHOOKSTRUCT|from_raw|as \*|data\.0|format!|println!|\.lock\(/);
  assert.doesNotMatch(text, /unsafe impl|SendInput|keybd_event|SetForegroundWindow|SetKeyboardState/);
  assert.match(text, /deadline[\s\S]*?saturating_duration_since\(Instant::now\(\)\)[\s\S]*?\.min\(START_LIMIT\)/);
  assert.match(text, /MsgWaitForMultipleObjectsEx\(/);
  assert.match(text, /WaitForSingleObject\(HANDLE\(worker\.as_raw_handle\(\)\), CLOSE_MILLIS\)/);
  assert.match(text, /UnhookWindowsHookEx\(hook\)/);
  const lines = text.split('\n');
  for (const {line} of inspectSource(text).sites) {
    assert(lines.slice(Math.max(0, line - 7), line).some(s => s.includes('SAFETY:')), `unsafe site line ${line}`);
  }
});

test('native adapter starts once before leases and closes independently of process errors', () => {
  const core = read(`${live}/native_backend.rs`), adapter = read(`${live}/native_backend/platform.rs`);
  assert.deepEqual(inspectSource(core).sites, []);
  assert.deepEqual(inspectSource(adapter).sites, []);
  const input = core.slice(core.indexOf('let monitor = self.sender.prepare'), core.indexOf('fn runtime_candidate'));
  assert(input.indexOf('sender.prepare') < input.indexOf('publications.acquire'));
  assert.match(adapter, /if self\.keyboard\.is_none\(\) \{/);
  assert.match(adapter, /KeyboardActivity::new\(control\.deadline\(\)\)/);
  assert.match(core, /finish\(self\.sender\.close\(\), self\.reader\.close\(\)\)/);
  assert.match(core, /"desktop\.key_already_held" \| "desktop\.keyboard_active"/);
  assert.match(adapter, /send_mailbox_action\(&self\.target, bindings, action, control, keyboard, guard\)/);
});

test('historical receipt/recovery custody and explicit current ownership successors remain separate', () => {
  const mapBytes = read('tests/rust-parity/release-321-keyboard-test-map.json');
  assert.equal(sha256(mapBytes), 'f2599c7cde4546d8eb43b38738467d9341f8d7d17ad573ba86a00185b70b2dc9');
  const map = JSON.parse(mapBytes);
  assert.equal(map.oracle, oracle.current.commit);
  for (const row of map.rustTests) {
    assert.equal(row.state, 'NOT_RUN');
    const text = read(row.candidatePath);
    for (const name of row.candidateTests) assert.match(text, new RegExp(`fn ${name}\\(`), name);
  }
  for (const row of map.retainedProduction) {
    const current = read(row.path);
    const delta = increment.productChanges.find(change => change.path === row.path);
    const successor = RETAINED_SUCCESSORS.find(change => change.path === row.path);
    let retained = current;
    if (successor) {
      assert.equal(delta, undefined, 'successors do not override a frozen 3.2.2 delta');
      const fixed = commit => execFileSync('git', ['--no-replace-objects', '-C', root, 'cat-file', 'blob', `${commit}:${row.path}`], {maxBuffer: 2 << 20, timeout: 10000});
      retained = fixed(increment.candidateBaseCommit).toString('utf8');
      assertSuccessorCustody(successor, current, retained, row.sha256, fixed(successor.commit));
    }
    if (delta) {
      assert.equal(delta.beforeSHA256, row.sha256, row.path);
      const original = execFileSync('git', ['--no-replace-objects', '-C', root, 'cat-file', 'blob', `${increment.candidateBaseCommit}:${row.path}`]);
      assert.equal(sha256(original), row.sha256, row.path);
      assert.equal(sha256(current), delta.sha256, row.path);
    } else assert.equal(sha256(retained), row.sha256, row.path);
    if (row.testOnlyIntegration) {
      const delta = row.testOnlyIntegration;
      assert.equal(delta.commit, '5152cc90a69e2108f159b009bc38df6e7cd2c09c');
      assert.equal(retained.split(delta.replacementText).length, 2);
      assert.equal(sha256(retained.replace(delta.replacementText, delta.originalText)), delta.originalSHA256);
    }
  }
  assert.deepEqual(map.execution, {rustCompilation: 'NOT_RUN', rustTests: 'NOT_RUN', windowsHook: 'NOT_RUN', windowsFixture: 'NOT_RUN', actualGame: 'NOT_RUN'});
});

// These checks exercise the current worktree and reject both custody corruption
// and specific safety guard removal. They do not execute the mapped Rust tests.
test('successor custody rejects candidate edits, wrong reference bytes and unlisted paths', () => {
  const map = JSON.parse(read('tests/rust-parity/release-321-keyboard-test-map.json'));
  for (const revision of RETAINED_SUCCESSORS) {
    const row = map.retainedProduction.find(row => row.path === revision.path);
    const git = commit => execFileSync('git', ['--no-replace-objects', '-C', root, 'cat-file', 'blob', `${commit}:${revision.path}`], {maxBuffer: 2 << 20, timeout: 10000});
    const historical = git(increment.candidateBaseCommit), fixed = git(revision.commit), current = read(revision.path);
    assertSuccessorCustody(revision, current, historical, row.sha256, fixed);
    assert.throws(() => assertSuccessorCustody(revision, `${current}\n// unreviewed\n`, historical, row.sha256, fixed), /candidate successor/);
    assert.throws(() => assertSuccessorCustody(revision, current, `${historical}\n`, row.sha256, fixed), /original retained/);
    assert.throws(() => assertSuccessorCustody(revision, current, historical, row.sha256, `${fixed}\n`), /fixed successor/);
    assert.throws(() => assertSuccessorCustody({...revision, path: 'unreviewed/journal.rs'}, current, historical, row.sha256, fixed), /exact reviewed/);
    assert.throws(() => assertSuccessorSafety('unreviewed/journal.rs', current), /exact reviewed/);
  }
});
test('successor safety assertions independently reject control, checksum, durability and ownership regressions', () => {
  const mutations = {
    'observations.rs': [
      ['check_control(control)?;\n    let Some(record) = body.into_iter().next()', 'let Some(record) = body.into_iter().next()'],
      ['checksum(&record.payload) != receipt.report_checksum', 'false'],
      ['!lycheedev_core::raw_json::is_valid(&record.payload)', 'false'],
    ],
    'durable_journal.rs': [
      ['if bytes == prior.bytes {', 'if bytes.len() == prior.bytes.len() {'],
      ['const CHAIN_CACHE_BYTES: usize = CHECKPOINT_BYTES;', 'const CHAIN_CACHE_BYTES: usize = MAX_LOG_BYTES;'],
      ['drop(self.take_cache());\n        let result = self.lease', 'let result = self.lease'],
      ['            let sync = file.sync_all().map_err(io_error);', '            let sync = file.flush().map_err(io_error);'],
      ['if !quiet && !(allow_pending_known_zero && known_zero)', 'if !quiet && !allow_pending_known_zero'],
      ['for chunk in bytes.chunks(65536) {\n                    check_control(control)?;', 'for chunk in bytes.chunks(65536) {'],
    ],
    'journal.rs': [
      ['rest = &rest[valid + 1..];', 'rest = &rest[valid + 2..];'],
      ['artifacts.entry(digest).or_insert(bytes);', 'artifacts.insert(digest, bytes);'],
      ['Cow::Borrowed(&o.result_bytes.0)', 'Cow::Owned(o.result_bytes.0.clone())'],
      ['if hash(&bytes) != digest {', 'if false {'],
    ],
  };
  for (const [file, changes] of Object.entries(mutations)) {
    const path = `${live}/${file}`, original = read(path);
    assertSuccessorSafety(path, original);
    for (const [before, after] of changes) {
      assert.equal(original.split(before).length, 2, `${file}: unique mutation`);
      assert.throws(() => assertSuccessorSafety(path, original.replace(before, after)), {code: 'ERR_ASSERTION'}, `${file}: ${before}`);
    }
  }
});
test('successor resource and borrowed-payload regression names remain present without claiming execution', () => {
  const tests = {
    'durable_journal_tests.rs': [
      'go_mailbox_v1_durable_verified_cache_reuses_only_equal_complete_bytes',
      'go_mailbox_v1_durable_verified_cache_preserves_external_change_errors',
      'go_mailbox_v1_durable_verified_cache_changed_equivalent_json_still_replays',
      'go_mailbox_v1_durable_verified_cache_checkpoint_boundary_and_close_release',
      'go_mailbox_v1_durable_verified_cache_capacity_and_real_read_growth_evict',
      'go_mailbox_v1_durable_verified_cache_never_certifies_writer_only_line_boundary',
      'go_mailbox_v1_durable_borrowed_snapshot_rechecks_artifacts_and_retains_no_state_borrow',
      'go_mailbox_v1_durable_artifact_failure_and_cancel_leave_log_unchanged',
    ],
    'journal_snapshot_tests.rs': [
      'go_mailbox_v1_snapshot_borrowed_payloads_preserve_pointer_capacity_and_call_lifetime',
      'go_mailbox_v1_snapshot_borrowed_normalization_keeps_each_bad_byte_and_first_digest_owner',
    ],
  };
  for (const [file, names] of Object.entries(tests)) for (const name of names) {
    assert.match(read(`${live}/${file}`), new RegExp(`fn ${name}\\(`));
  }
});
