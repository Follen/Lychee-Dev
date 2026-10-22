// Shared test-controller client for the formal supervised Agent JSONL protocol.
// The enclosing controller MUST run under rust-owned-process; this client is
// not an alternative process-tree owner and does not certify cleanup itself.
import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {TextDecoder} from 'node:util';

export const AGENT_PROTOCOL = 'lycheedev.data-agent.v1';
const utf8 = new TextDecoder('utf-8', {fatal: true});
const MAX_FRAME = 32 << 20;
const exactTokens = new WeakSet();
export const exactNumber = text => { assert(/^-?(?:0|[1-9][0-9]*)$/.test(text),'agent.exact_integer_token'); const value=Object.freeze({exactNumber:text});exactTokens.add(value);return value; };
export const isExactNumber = value => !!value && typeof value==='object' && exactTokens.has(value);

// Preserve exact numeric tokens instead of rounding u64/i64 payloads. Unsafe
// context numbers are refused below; payload numbers remain explicit objects.
export function parseAgentJson(input, {maximumBytes = MAX_FRAME} = {}) {
  const bytes = Buffer.isBuffer(input) ? input : Buffer.from(input);
  assert(bytes.length <= maximumBytes, 'agent.json_bytes');
  const text = utf8.decode(bytes);
  let at = 0;
  const space = () => { while (at < text.length && /[ \t\r\n]/.test(text[at])) at++; };
  function value(depth = 0) {
    assert(depth <= 128, 'agent.json_depth'); space();
    const start = at, c = text[at];
    if (c === '"') {
      at++;
      while (at < text.length) {
        if (text[at] === '\\') { at += 2; continue; }
        if (text[at++] === '"') return JSON.parse(text.slice(start, at));
      }
      throw Error('agent.json_string');
    }
    if (c === '[' || c === '{') {
      at++; space(); const array = c === '[', result = array ? [] : {};
      const close = array ? ']' : '}';
      if (text[at] === close) { at++; return result; }
      for (;;) {
        if (array) result.push(value(depth + 1));
        else {
          assert.equal(text[at], '"', 'agent.json_key');
          const key = value(depth + 1); space();
          assert.equal(text[at++], ':', 'agent.json_colon');
          assert(!Object.hasOwn(result, key), 'agent.json_duplicate_key');
          Object.defineProperty(result, key, {value: value(depth + 1), enumerable: true, writable: true});
        }
        space(); if (text[at] === close) { at++; return result; }
        assert.equal(text[at++], ',', 'agent.json_separator'); space();
      }
    }
    for (const [word, result] of [['null', null], ['true', true], ['false', false]]) {
      if (text.startsWith(word, at)) { at += word.length; return result; }
    }
    const match = /^-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?/.exec(text.slice(at));
    assert(match, 'agent.json_value'); at += match[0].length;
    const number = Number(match[0]);
    assert(Number.isFinite(number), 'agent.json_nonfinite');
    if (/^-?\d+$/.test(match[0]) && !Number.isSafeInteger(number)) return exactNumber(match[0]);
    // Finite decimals follow binary64; raw frame bytes remain the exact evidence.
    return number;
  }
  const result = value(); space(); assert.equal(at, text.length, 'agent.json_trailing');
  return result;
}

const exactToken = isExactNumber;
export function assertSafeContext(value, label = 'context', depth = 0) {
  assert(depth <= 128, 'agent.context_depth');
  if (typeof value === 'number') assert(Number.isFinite(value) && (!Number.isInteger(value) || Number.isSafeInteger(value)), `agent.unsafe_${label}`);
  if (!value || typeof value !== 'object') return;
  assert(!exactToken(value)&&!Object.hasOwn(value,'exactNumber'), `agent.unsafe_${label}`);
  for (const item of Object.values(value)) assertSafeContext(item, label, depth + 1);
}

// WDC partition key IDs and CASC entry name hashes are u64 identity payloads.
// A wide account, request ID, row count or budget remains an invalid context.
function assertProvenance(value,path=[]) {
 const partitionKey=path.length>=4 && path.at(-1)==='keyId' && /^(0|[1-9][0-9]*)$/.test(path.at(-2)) && path.at(-3)==='partitions' && (path.at(-4)==='sourceLayout' || (path.at(-4)==='layout' && path.at(-5)==='sourceIdentity'));
 const nameHash=path.length>=3 && path.at(-1)==='nameHash' && path.at(-2)==='entry' && path.at(-3)==='sourceReading';
 if(partitionKey || nameHash) {
  if(isExactNumber(value)){assert(/^(0|[1-9][0-9]*)$/.test(value.exactNumber));assert(BigInt(value.exactNumber)<=18446744073709551615n);}
  else assert(Number.isSafeInteger(value)&&value>=0,'agent.provenance_u64');
  return;
 }
 if(isExactNumber(value)||(value&&typeof value==='object'&&Object.hasOwn(value,'exactNumber')))throw Error('agent.unsafe_provenance');
 if(value&&typeof value==='object'){for(const [key,item]of Object.entries(value))assertProvenance(item,[...path,key]);}
 else assertSafeContext(value,'provenance');
}
// Narrow canonical fixture serializer: exact integer tokens, no float rewriting.
export function stringifyExactIntegerJson(value) {
 if(isExactNumber(value))return value.exactNumber;
 if(typeof value==='number'){assert(Number.isSafeInteger(value)&&!Object.is(value,-0),'agent.canonical_integer_only');return String(value);}
 if(value===null||typeof value==='string'||typeof value==='boolean')return JSON.stringify(value);
 if(Array.isArray(value))return '['+value.map(stringifyExactIntegerJson).join(',')+']';
 assert(value&&typeof value==='object');return '{'+Object.entries(value).map(([key,item])=>JSON.stringify(key)+':'+stringifyExactIntegerJson(item)).join(',')+'}';
}

export function decodeAgentFrame(raw, expectedId) {
  const frame = parseAgentJson(raw);
  assert.equal(frame.protocol, AGENT_PROTOCOL);
  assert(Number.isSafeInteger(frame.id) && frame.id > 0, 'agent.frame_id');
  assert.equal(frame.id, expectedId, 'agent.response_identity');
  assert.equal(typeof frame.ok, 'boolean');
  assert.equal(Object.hasOwn(frame, frame.ok ? 'result' : 'error'), true);
  assert.equal(Object.hasOwn(frame, frame.ok ? 'error' : 'result'), false);
  if (!frame.ok) assertSafeContext(frame.error, 'error');
  else {
    // Row/cell payloads and designated u64 source identities retain exact tokens.
    // Request identity and accounting context must remain safe integers.
    for (const key of ['provenance', 'offset', 'totalRows', 'complete', 'result', 'capture']) {
      if (Object.hasOwn(frame.result ?? {}, key)) { if(key==='provenance')assertProvenance(frame.result[key]);else {assertSafeContext(frame.result[key], key);if(['offset','totalRows'].includes(key))assert(Number.isSafeInteger(frame.result[key])&&frame.result[key]>=0);if(key==='complete')assert.equal(typeof frame.result[key],'boolean');} }
    }
  }
  return frame;
}

export class AgentClient {
  constructor(command, args, {cwd, env, deadline, onFrame = () => {}}) {
    assert(Number.isFinite(deadline) && deadline > performance.now(), 'agent.deadline');
    this.deadline = deadline; this.onFrame = onFrame; this.nextId = 1;
    this.pending = null; this.bytes = Buffer.alloc(0); this.stderr = [];
    this.closed = false; this.stderrBytes = 0; this.totalBytes = 0; this.frames = []; this.failed = null;
    this.child = spawn(command, args, {cwd, env, shell: false, windowsHide: true, stdio: ['pipe', 'pipe', 'pipe']});
    this.exit = new Promise(resolve => {
      this.child.once('error', error => this.fail(error));
      this.child.once('close', (code, signal) => {
        this.closed = true;
        if (this.bytes.length) this.fail(Error('agent.incomplete_frame'));
        if (this.pending) this.fail(Error('agent.exit_before_response'));
        resolve({code, signal, error: this.failed?.message});
      });
    });
    this.child.stdout.on('data', chunk => {
      try {
        this.totalBytes += chunk.length;
        assert(this.totalBytes <= 128 << 20, 'agent.session_output_bytes');
        assert(this.bytes.length + chunk.length <= MAX_FRAME, 'agent.frame_bytes');
        this.bytes = Buffer.concat([this.bytes, chunk]);
        for (;;) {
          const end = this.bytes.indexOf(10); if (end < 0) break;
          const line = this.bytes.subarray(0, end); this.bytes = this.bytes.subarray(end + 1);
          assert(line.length > 0 && this.pending, 'agent.unsolicited_frame');
          const pending = this.pending, frame = decodeAgentFrame(line, pending.request.id);
          this.pending = null; clearTimeout(pending.timer);
          const observation = {request: pending.request, frame, raw: Buffer.from(line)};
          this.frames.push(observation);
          try { this.onFrame(observation); pending.resolve(frame); }
          catch (error) { pending.reject(error); throw error; }
        }
      } catch (error) { this.fail(error); }
    });
    this.child.stderr.on('data', chunk => {
      this.stderrBytes += chunk.length;
      if (this.stderrBytes > 1 << 20) this.fail(Error('agent.stderr_bytes'));
      else this.stderr.push(Buffer.from(chunk));
    });
    this.child.stdin.on('error', error => this.fail(error));
  }
  fail(error) {
    this.failed ??= error;
    if (this.pending) {
      const pending = this.pending; this.pending = null;
      clearTimeout(pending.timer); pending.reject(error);
    }
    this.child.stdin.destroy();
    // A plain child.kill is not a tree cleanup receipt. The outer owned runner
    // remains responsible for termination/reaping if this child cannot exit.
  }
  request(task, {timeoutMs = 30000} = {}) {
    if (this.failed) return Promise.reject(this.failed);
    if (this.closed) return Promise.reject(Error('agent.session_closed')); 
    assert(!this.pending, 'agent.one_controller_request_at_a_time');
    assertSafeContext(task, 'request');
    for (const key of ['protocol','id','timeout_ms']) assert(!Object.hasOwn(task,key), 'agent.reserved_request_key');
    assert(Number.isSafeInteger(timeoutMs) && timeoutMs > 0 && timeoutMs <= 3600000, 'agent.timeout');
    const remaining = Math.floor(this.deadline - performance.now());
    assert(remaining > 0, 'agent.controller_deadline');
    const request = JSON.parse(JSON.stringify({protocol: AGENT_PROTOCOL, id: this.nextId++, timeout_ms: Math.min(timeoutMs, remaining), ...task}));
    const bytes = Buffer.from(JSON.stringify(request) + '\n');
    assert(bytes.length <= 1 << 20, 'agent.request_bytes');
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => this.fail(Error('agent.response_deadline')), remaining);
      this.pending = {request, resolve, reject, timer};
      this.child.stdin.write(bytes, error => { if (error) this.fail(error); });
    });
  }
  async success(task, options) {
    const frame = await this.request(task, options);
    assert.equal(frame.ok, true, JSON.stringify(frame.error));
    return frame.result;
  }
  async close() {
    const result = await this.success({task: 'close'});
    this.child.stdin.end();
    let timer;
    try {
      const process = await Promise.race([this.exit, new Promise((_,reject) => {
        timer = setTimeout(() => reject(Error('agent.exit_deadline')), Math.max(1, this.deadline-performance.now()));
      })]);
      assert.equal(process.code, 0, 'agent.close_exit'); assert.equal(process.signal, null);
      assert.equal(process.error, undefined);
      return {result, process};
    } finally { clearTimeout(timer); }
  }
}
