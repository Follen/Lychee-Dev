// One owner, one coordinator publisher, one outstanding request. No daemon.
import assert from 'node:assert/strict';
import {existsSync, lstatSync, openSync, closeSync, fsyncSync, readFileSync, readdirSync, renameSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {LIMITS} from './trial-policy.mjs';
export function regular(path, directory = false) {
  const st = lstatSync(path); assert(!st.isSymbolicLink() && (directory ? st.isDirectory() : st.isFile() && st.nlink === 1), 'trial.regular_unlinked_path'); return st;
}
export function readBounded(path, max) { assert(regular(path).size <= max, 'trial.file_bytes'); const bytes = readFileSync(path); assert(bytes.length <= max, 'trial.file_bytes'); return bytes; }
export function atomicNew(directory, name, bytes) {
  regular(directory, true); assert(/^[a-z0-9.-]+$/.test(name), 'trial.file_name');
  const final = join(directory, name), temp = final + '.tmp'; assert(!existsSync(final), 'trial.duplicate_publication');
  const fd = openSync(temp, 'wx', 0o600);
  try { writeFileSync(fd, bytes); fsyncSync(fd); } finally { closeSync(fd); }
  assert(!existsSync(final), 'trial.duplicate_publication'); renameSync(temp, final);
}
export function decodeRequest(bytes, seq) {
  assert(bytes.length <= LIMITS.requestBytes, 'trial.request_bytes');
  const request = JSON.parse(bytes.toString('utf8'));
  assert(request && !Array.isArray(request) && Object.getPrototypeOf(request) === Object.prototype, 'trial.request_object');
  assert.equal(request.seq, seq, 'trial.sequence'); assert(['call','finish','cancel','restore-key'].includes(request.op), 'trial.operation');
  const expected = request.op === 'call' ? ['argv','op','seq'] : request.op === 'restore-key' ? ['op','reason','seq'] : ['op','seq'];
  assert.deepEqual(Object.keys(request).sort(), expected, 'trial.request_fields');
  if (request.op === 'call') assert(Array.isArray(request.argv), 'trial.argv_array');
  if (request.op === 'restore-key') assert(typeof request.reason === 'string' && request.reason.length > 0 && Buffer.byteLength(request.reason) <= 512, 'trial.key_request_reason');
  return request;
}
const numbered = seq => String(seq).padStart(3, '0');
export function publishRequest(inbox, request) {
  assert(Number.isInteger(request.seq) && request.seq >= 1 && request.seq <= LIMITS.requests, 'trial.sequence_budget');
  const raw = Buffer.from(JSON.stringify(request)); decodeRequest(raw, request.seq);
  const prior = request.seq - 1;
  if (prior) assert(existsSync(join(inbox, `${numbered(prior)}.response.json`)), 'trial.previous_response_required');
  atomicNew(inbox, `${numbered(request.seq)}.request.json`, raw);
}
export function takeRequest(inbox, seq) {
  regular(inbox, true);
  const names = readdirSync(inbox); assert(names.length <= (LIMITS.requests) * 3, 'trial.inbox_count');
  for (const name of names) {
    assert(/^\d{3}\.(request|response)\.json(?:\.tmp)?$/.test(name), 'trial.inbox_unexpected');
    assert(regular(join(inbox,name)).size <= (name.includes('.request.') ? LIMITS.requestBytes : 12<<20),'trial.inbox_file_bytes');
    const n = Number(name.slice(0,3)); assert(n >= 1 && n <= seq && n <= LIMITS.requests, 'trial.sequence_out_of_order');
  }
  const path = join(inbox, `${numbered(seq)}.request.json`);
  return existsSync(path) ? readBounded(path, LIMITS.requestBytes) : null;
}
export function publishResponse(inbox, seq, response) { const bytes=Buffer.from(JSON.stringify(response)); assert(bytes.length<=12<<20,'trial.response_bytes'); atomicNew(inbox, `${numbered(seq)}.response.json`, bytes); }
export const responsePath = (inbox, seq) => join(inbox, `${numbered(seq)}.response.json`);
export function makeBudget(now = () => performance.now(), limits = LIMITS) {
  const start = now(); let last = start, calls = 0, input = 0, output = 0;
  return {
    check() { assert(now() - start < limits.totalMillis, 'trial.total_deadline'); assert(now() - last < limits.idleMillis, 'trial.idle_deadline'); },
    accept(bytes, call) { this.check(); input += bytes; assert(input <= limits.inputBytes, 'trial.total_input_bytes'); if (call) assert(++calls <= limits.commands, 'trial.command_count'); last = now(); },
    record(stdout, stderr) { output += stdout.length + stderr.length; assert(stdout.length <= limits.streamBytes && stderr.length <= limits.streamBytes && output <= limits.outputBytes, 'trial.output_budget'); last = now(); this.check(); },
    timeout() { this.check(); return Math.max(1, Math.min(limits.commandMillis, Math.floor(limits.totalMillis - (now() - start)))); },
    snapshot() { return {calls,inputBytes: input,outputBytes: output,elapsedMillis: now() - start}; },
  };
}

export function childOutcome(run) {
  assert(!run.error && !run.signal && Number.isInteger(run.status), 'trial.child_process_failure');
  // All real business exits, including arguments/capability/partial failures,
  // remain observable. They are neither rewritten to zero nor fatal to trial.
  return {exit: run.status, stdout: (run.stdout ?? Buffer.alloc(0)).toString('utf8'), stderr: (run.stderr ?? Buffer.alloc(0)).toString('utf8')};
}
