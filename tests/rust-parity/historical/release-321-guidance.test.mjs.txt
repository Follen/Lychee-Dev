// Document-only custody; does not prove the described Windows behavior works.
import test from 'node:test';
import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {readFileSync} from 'node:fs';
import {dirname, resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const path = 'skills/lycheedev/references/live-recovery.md';
const pin = '10b561db5706702cd42e047eaeed4ccb5e825351';
const old = 'Held Ctrl, Alt, Shift, either Win key or a key in the selected chord delays input\nuntil release. The CLI never forces the user\'s physical keys up. A user can\nstill interrupt a chord after that check; only the addon receipt confirms execution.\n';
const paragraph = text => {
  const start = text.indexOf('Keyboard activity pauses new automatic chords.');
  assert(start >= 0);
  return text.slice(start, text.indexOf('\n\n', start) + 1);
};
const candidate = readFileSync(resolve(root, path), 'utf8');
const upstream = execFileSync('git', ['--no-replace-objects', '-C', root, 'cat-file', 'blob', `${pin}:${path}`], {maxBuffer: 1 << 20, stdio: ['ignore', 'pipe', 'pipe']}).toString();
test('Skill keyboard recovery paragraph is byte-for-byte fixed Go 3.2.1', () => {
  assert.equal(paragraph(candidate), paragraph(upstream));
});
test('every byte outside the authorized keyboard paragraph is preserved', () => {
  assert.equal(createHash('sha256').update(candidate.replace(paragraph(candidate), old)).digest('hex'), 'c7c11368ccebf16877b9f8d832c74847097af66cb22f241317d6d980a0e92ef2');
});
