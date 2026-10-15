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

test('new behavioral regression names and unchanged receipt/recovery production are recorded honestly', () => {
  const map = JSON.parse(read('tests/rust-parity/release-321-keyboard-test-map.json'));
  assert.equal(map.oracle, oracle.current.commit);
  for (const row of map.rustTests) {
    assert.equal(row.state, 'NOT_RUN');
    const text = read(row.candidatePath);
    for (const name of row.candidateTests) assert.match(text, new RegExp(`fn ${name}\\(`), name);
  }
  for (const row of map.retainedProduction) {
    const current = read(row.path);
    const delta = increment.productChanges.find(change => change.path === row.path);
    if (delta) {
      assert.equal(delta.beforeSHA256, row.sha256, row.path);
      const original = execFileSync('git', ['--no-replace-objects', '-C', root, 'cat-file', 'blob', `${increment.candidateBaseCommit}:${row.path}`]);
      assert.equal(sha256(original), row.sha256, row.path);
      assert.equal(sha256(current), delta.sha256, row.path);
    } else assert.equal(sha256(current), row.sha256, row.path);
    if (row.testOnlyIntegration) {
      const delta = row.testOnlyIntegration;
      assert.equal(delta.commit, '5152cc90a69e2108f159b009bc38df6e7cd2c09c');
      assert.equal(current.split(delta.replacementText).length, 2);
      assert.equal(sha256(current.replace(delta.replacementText, delta.originalText)), delta.originalSHA256);
    }
  }
  assert.deepEqual(map.execution, {rustCompilation: 'NOT_RUN', rustTests: 'NOT_RUN', windowsHook: 'NOT_RUN', windowsFixture: 'NOT_RUN', actualGame: 'NOT_RUN'});
});
