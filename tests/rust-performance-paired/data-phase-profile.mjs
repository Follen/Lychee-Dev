// Diagnostic-only contract for the query-owned Rust Data recorder.
// Keep in sync with crates/lycheedev-data/src/phase_profile.rs; never subtract
// these overlapping wall-clock observations from the original owned interval.
import assert from 'node:assert/strict';

export const DATA_PHASE_NAMES = Object.freeze([
  'admission', 'prepare_inclusive', 'payload_inclusive', 'texture_decode',
  'pixel_transform_and_hash', 'encode', 'source_capture_metadata_inclusive',
  'artifact_capture_metadata_inclusive', 'manifest_capture_metadata_inclusive',
  'output', 'source_close',
]);
export const DATA_PHASE_TIMING = 'wall-clock; phases may nest or overlap across workers; not additive; budget lifetime and phases are diagnostics, not accepted command total or CPU time';
export const DATA_CAPTURE_TIMING = 'capture_metadata_inclusive includes capture and metadata work; metadata open/write/close are not separately observed';
const VARIABLE = 'LYCHEEDEV_DATA_PHASE_PROFILE';

export function dataPhaseEnvironment(environment, {enabled = false, measured = false, engine = null} = {}) {
  // Windows environment names are case-insensitive. An inherited opt-in must
  // not leak into Go, preparation, ordinary measurement or startup-only mode.
  const result = Object.fromEntries(Object.entries(environment).filter(([key]) => key.toUpperCase() !== VARIABLE));
  if (enabled && measured && engine === 'rust') result[VARIABLE] = '1';
  return result;
}

export function dataPhaseProfile(stderr, {entry = null, successful = false} = {}) {
  assert(Buffer.isBuffer(stderr), 'Data phase stderr must retain original bytes');
  assert(stderr.length > 0 && stderr.length <= 16384, 'missing or oversized Data phase profile');
  const text = new TextDecoder('utf-8', {fatal: true, ignoreBOM: true}).decode(stderr);
  const line = text.endsWith('\r\n') ? text.slice(0, -2) : text.endsWith('\n') ? text.slice(0, -1) : text;
  assert(line.startsWith('{') && line.endsWith('}') && !/[\r\n]/.test(line), 'expected exactly one Data phase JSON record; unknown stderr rejected');
  const value = JSON.parse(line);
  // The recorder emits compact JSON. Round-trip identity also rejects duplicate
  // keys, lossy numbers, BOMs, and additional non-profile stderr bytes.
  assert.equal(JSON.stringify(value), line, 'Data phase record must be compact, exact JSON without duplicate keys');
  assert.deepEqual(Object.keys(value).sort(), ['captureTiming', 'elapsedMicros', 'phases', 'schema', 'timing']);
  assert.equal(value.schema, 'lycheedev.data-query-phases.v1');
  assert.equal(value.timing, DATA_PHASE_TIMING); assert.equal(value.captureTiming, DATA_CAPTURE_TIMING);
  const integer = (value, label) => assert(Number.isSafeInteger(value) && value >= 0, `${label}: nonnegative exact integer required`);
  integer(value.elapsedMicros, 'budget elapsedMicros');
  assert(Array.isArray(value.phases)); assert.equal(value.phases.length, DATA_PHASE_NAMES.length);
  for (const [index, phase] of value.phases.entries()) {
    assert(phase && !Array.isArray(phase) && typeof phase === 'object');
    assert.deepEqual(Object.keys(phase).sort(), ['calls', 'elapsedMicros', 'phase']);
    assert.equal(phase.phase, DATA_PHASE_NAMES[index], 'exact fixed Data phase order required');
    integer(phase.calls, `${phase.phase}.calls`); integer(phase.elapsedMicros, `${phase.phase}.elapsedMicros`);
    if (phase.calls === 0) assert.equal(phase.elapsedMicros, 0, 'an unobserved phase cannot have elapsed time');
  }
  // Only these authenticated twelve fixture paths require these known scopes
  // on success. Early product failures legitimately finish before any phase.
  const image = entry?.kind === 'image' && /^B04-(palette|bgra8|bc1|bc2|bc3)-(png|webp)$/.test(entry.id);
  const physical = entry?.kind === 'sql' && entry.id === 'B04-sql-correctness'
    || entry?.kind === 'content' && entry.id === 'B04-blte-17MiB';
  if (successful && (image || physical)) {
    const required = image ? DATA_PHASE_NAMES : ['admission', 'prepare_inclusive', 'payload_inclusive', 'source_capture_metadata_inclusive', 'source_close'];
    for (const name of required) assert(value.phases.find(phase => phase.phase === name).calls > 0, `${name}: required successful fixture scope was not observed`);
  }
  return value;
}
