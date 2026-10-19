// Scope guard for four blind trials, not a substitute CLI parser or evaluator.
import assert from 'node:assert/strict';
import {FIXED_PIN} from './data-inputs.mjs';
export const PRODUCT_COMMIT = '86f2bbb0f65234bba1bc0454d916e85fc6855ad9';
export const SOURCE = Object.freeze({repository: 'wow-ui-source', product: 'retail', commit: '31c7f7b9cc79e56c986b365c06a6afbcf3c9177b'});
export const LIMITS = Object.freeze({requestBytes: 32768, inputBytes: 1 << 20, outputBytes: 32 << 20, streamBytes: 4 << 20, commands: 40, requests: 42, totalMillis: 1200000, idleMillis: 180000, commandMillis: 210000, pollMillis: 100});
const commands = new Map([
  ['version', []], ['describe', []], ['init', []], ['target show', []],
  ['target resolve', ['installation', 'product', 'build', 'region', 'locale', 'definitions', 'offline']],
  ['data sql', ['sql', 'snapshot', 'installation', 'offline', 'key-file', 'param', 'max-bytes', 'timeout-seconds', 'page-size']],
  ['data db2', ['snapshot', 'installation', 'offline', 'key-file', 'table', 'id', 'limit', 'after-id', 'max-bytes', 'timeout-seconds']],
  ['data db2 schema', ['snapshot', 'installation', 'offline', 'key-file', 'max-bytes', 'timeout-seconds']],
  ['source list', ['snapshot']], ['source sync', ['source', 'product', 'ref']],
  ['source query', ['snapshot', 'mode', 'topic', 'limit', 'cursor']],
  ['source inspect', ['snapshot', 'path', 'line', 'count', 'symbol', 'symbol-id']],
  ['source refs', ['snapshot', 'symbol', 'symbol-id', 'limit', 'cursor', 'direction', 'static-only']],
  ['source context', ['snapshot', 'symbol', 'symbol-id', 'limit', 'cursor', 'depth', 'max-lines', 'max-bytes', 'static-only']],
  ['source session status', []], ['source session close', []],
  ['evidence show', []], ['evidence verify', []],
]);
const booleans = new Set(['help', 'offline', 'static-only']);
export function checkedArgv(argv, context) {
  assert(Array.isArray(argv) && argv.length > 0 && argv.length <= 64, 'trial.argv_array');
  assert(argv.every(value => typeof value === 'string' && value.length > 0 && !value.includes('\0') && Buffer.byteLength(value) <= 16384), 'trial.argv_string');
  assert(Buffer.byteLength(JSON.stringify(argv)) <= LIMITS.requestBytes, 'trial.argv_bytes');
  const flags = new Map(), words = [];
  for (let i = 0; i < argv.length; i++) {
    const token = argv[i];
    if (!token.startsWith('-')) { words.push(token); continue; }
    assert(/^--[a-z][a-z-]*(?:=.*)?$/s.test(token), 'trial.flag_shape');
    const equal = token.indexOf('='), key = token.slice(2, equal < 0 ? undefined : equal);
    assert(!flags.has(key) || key === 'param', 'trial.duplicate_flag');
    let value = equal < 0 ? undefined : token.slice(equal + 1);
    if (booleans.has(key)) assert(value === undefined, 'trial.boolean_flag');
    else { if (value === undefined) value = argv[++i]; assert(typeof value === 'string' && !value.startsWith('--'), 'trial.flag_value'); }
    flags.set(key, value);
  }
  const path = [...commands.keys()].sort((a, b) => b.length - a.length).find(path => words.slice(0, path.split(' ').length).join(' ') === path);
  const groupHelp = flags.has('help') && ['','data','data db2','source','target','evidence'].includes(words.join(' '));
  assert(path || groupHelp, 'trial.command_out_of_scope');
  const positional = path ? words.slice(path.split(' ').length) : [];
  const allowed = new Set(['help','home','format', ...(commands.get(path) ?? [])]);
  for (const key of flags.keys()) assert(allowed.has(key), `trial.flag_out_of_scope:${key}`);
  if (flags.has('home')) assert.equal(flags.get('home'), context.home, 'trial.fixed_home');
  if (flags.has('format')) assert.equal(flags.get('format'),'json','trial.machine_json_required');
  const help = flags.has('help');
  const take = (key, value) => { if (flags.has(key)) assert.equal(flags.get(key), value, `trial.fixed_${key}`); };
  take('installation', context.installation); take('key-file', context.keyFile);
  take('definitions', FIXED_PIN.definitionCommit); take('build', FIXED_PIN.fullBuild); take('region', FIXED_PIN.region); take('locale', FIXED_PIN.language);
  take('source', SOURCE.repository); take('product', 'retail'); take('ref', SOURCE.commit);
  if (flags.has('path')) { const value = flags.get('path'); assert(!/^[\\/]|[\\:\0]/.test(value) && value.split('/').every(p => p && p !== '.' && p !== '..'), 'trial.source_relative_path'); }
  if (flags.has('snapshot')) assert(context.pins.includes(flags.get('snapshot')), 'trial.fixed_pin');
  if (!help && path?.startsWith('data ')) {
    assert(context.kind !== 'source', 'trial.data_fixture_required');
    assert(flags.has('offline') && flags.get('snapshot') === context.pins[0] && flags.get('installation') === context.installation, 'trial.fixed_offline_data');
  }
  if (!help && path === 'source sync') {
    assert.equal(context.kind, 'source', 'trial.source_task_required');
    for (const [key,value] of [['source',SOURCE.repository],['product',SOURCE.product],['ref',SOURCE.commit]]) assert.equal(flags.get(key),value,'trial.fixed_source_sync');
  }
  if (!help && ['source query','source inspect','source refs','source context'].includes(path)) assert(context.kind === 'source' && context.pins.includes(flags.get('snapshot')), 'trial.source_pin_required');
  if (!help && path === 'target resolve') assert(context.kind !== 'source' && flags.has('offline') && flags.get('installation') === context.installation, 'trial.fixed_offline_target');
  if (path === 'target show' && !help) assert(positional.length === 1 && context.pins.includes(positional[0]), 'trial.target_pin');
  else if (path?.startsWith('evidence ') && !help) assert(positional.length === 1 && context.captures.includes(positional[0]), 'trial.returned_capture');
  else if (path === 'source query' || path === 'data db2 schema') assert(positional.length <= 1, 'trial.positional_count');
  else assert(positional.length === 0, 'trial.positional_count');
  return {path, help, args: [...argv, ...(!flags.has('home') ? ['--home', context.home] : []), ...(!flags.has('format') ? ['--format','json'] : [])]};
}
export function observeResult(context, path, stdout, exit, {help = false} = {}) {
  if (help) return; // Help is real output, not execution of the named command.
  let envelope; try { envelope = JSON.parse(stdout); } catch { return; }
  for (const capture of envelope.captures ?? []) if (/^CAP-[a-f0-9]{64}$/.test(capture.id) && !context.captures.includes(capture.id)) context.captures.push(capture.id);
  if (exit === 0 && path === 'source sync') {
    const result = envelope.result;
    assert(result?.source?.exactCommit === SOURCE.commit && result.source.requestedRef === SOURCE.commit && result.source.repository === SOURCE.repository && result.source.product === SOURCE.product && /^PIN-[a-f0-9]{64}$/.test(result.id), 'trial.actual_source_identity');
    if (!context.pins.includes(result.id)) context.pins.push(result.id);
  }
}
