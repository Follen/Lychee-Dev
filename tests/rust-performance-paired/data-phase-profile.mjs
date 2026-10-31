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
export const DATA_DETAIL_NAMES = Object.freeze([
  'encoded_identity_hash', 'decoded_cache_open', 'decoded_cache_lookup',
  'decoded_cache_verify_inclusive', 'decoded_stream_publish_inclusive',
  'encoded_chunk_read', 'encoded_chunk_md5', 'zlib_inflate',
  'whole_content_key_hash', 'decoded_cache_commit',
]);
export const DATA_PUBLISHER_NAMES = Object.freeze([
  'source_to_spool_inclusive', 'spool_to_shard_inclusive', 'final_verify_inclusive',
  'stage_create', 'private_spool_read_ready', 'final_stage_seal', 'digest_lock',
  'shard_open', 'rename', 'winner_open', 'checked_close',
]);
export const DATA_SQL_NAMES = Object.freeze(['request_compile_admit', 'execute_resolved_inclusive', 'source_prepare_inclusive', 'source_verify_inclusive', 'result_projection', 'capture_serialize', 'capture_publish_metadata_inclusive', 'generation_open_inclusive', 'generation_read_admission', 'generation_manifest_authenticate', 'generation_object_open_authenticate_inclusive', 'generation_directory_validate_inclusive', 'generation_cursor_batch_decode', 'generation_cursor_advance_inclusive', 'generation_prepare_inclusive', 'generation_import_encode_inclusive', 'generation_object_commit', 'generation_manifest_publish']);
export const DATA_CLI_PROFILE_VARIABLE = 'LYCHEEDEV_DATA_CLI_PROFILE';
export const DATA_CLI_TIMING = 'wall-clock from instrumented main entry; excludes pre-main startup and this CLI profile emission; dispatch includes nested Data phases and Data profile emission; external elapsed remainder is unattributed, not startup or CPU time';
export const DATA_PHASE_TIMING = 'wall-clock; phases may nest or overlap across workers; not additive; budget lifetime and phases are diagnostics, not accepted command total or CPU time';
export const DATA_CAPTURE_TIMING = 'capture_metadata_inclusive includes capture and metadata work; metadata open/write/close are not separately observed';
export function dataPhaseProfile(stderr) {
  assert(Buffer.isBuffer(stderr), 'Data phase stderr must retain original bytes');
  assert(stderr.length > 0 && stderr.length <= 16384, 'missing or oversized Data phase profile');
  const text = new TextDecoder('utf-8', {fatal: true, ignoreBOM: true}).decode(stderr);
  const line = text.endsWith('\r\n') ? text.slice(0, -2) : text.endsWith('\n') ? text.slice(0, -1) : text;
  assert(line.startsWith('{') && line.endsWith('}') && !/[\r\n]/.test(line), 'expected exactly one Data phase JSON record; unknown stderr rejected');
  const value = JSON.parse(line);
  // The recorder emits compact JSON. Round-trip identity also rejects duplicate
  // keys, lossy numbers, BOMs, and additional non-profile stderr bytes.
  assert.equal(JSON.stringify(value), line, 'Data phase record must be compact, exact JSON without duplicate keys');
  assert.equal(value.schema, 'lycheedev.data-query-phases.v5', 'complete v5 Data profile required');
  assert.deepEqual(Object.keys(value).sort(), ['captureTiming', 'details', 'elapsedMicros', 'phases', 'publisher', 'schema', 'sql', 'timing']);
  assert.equal(value.timing, DATA_PHASE_TIMING); assert.equal(value.captureTiming, DATA_CAPTURE_TIMING);
  const integer = (value, label) => assert(Number.isSafeInteger(value) && value >= 0, `${label}: nonnegative exact integer required`);
  integer(value.elapsedMicros, 'budget elapsedMicros');
  function counters(values, names) {
    assert(Array.isArray(values)); assert.equal(values.length, names.length);
    for (const [index, phase] of values.entries()) {
      assert(phase && !Array.isArray(phase) && typeof phase === 'object');
      assert.deepEqual(Object.keys(phase).sort(), ['calls', 'elapsedMicros', 'phase']);
      assert.equal(phase.phase, names[index], 'exact fixed Data phase order required');
      integer(phase.calls, `${phase.phase}.calls`); integer(phase.elapsedMicros, `${phase.phase}.elapsedMicros`);
      if (phase.calls === 0) assert.equal(phase.elapsedMicros, 0, 'an unobserved phase cannot have elapsed time');
    }
  }
  counters(value.phases, DATA_PHASE_NAMES);
  counters(value.details, DATA_DETAIL_NAMES);
  counters(value.publisher, DATA_PUBLISHER_NAMES);
  counters(value.sql, DATA_SQL_NAMES);
  return value;
}

// CLI record is deliberately independent of the query budget recorder. The
// generation runner alone opts into both and validates their two-line framing.
export function dataCliProfile(stderr) {
  assert(Buffer.isBuffer(stderr) && stderr.length > 0 && stderr.length <= 4096);
  const text = new TextDecoder('utf-8', {fatal: true, ignoreBOM: true}).decode(stderr);
  const line = text.endsWith('\r\n') ? text.slice(0, -2) : text.endsWith('\n') ? text.slice(0, -1) : text;
  assert(line.startsWith('{') && line.endsWith('}') && !/[\r\n]/.test(line));
  const value = JSON.parse(line);
  assert.equal(JSON.stringify(value), line, 'exact compact CLI profile required');
  assert.deepEqual(Object.keys(value).sort(), ['command', 'dispatchMicros', 'mainElapsedMicros', 'mainEntryToDispatchMicros', 'renderMicros', 'renderSucceeded', 'schema', 'timing']);
  assert.equal(value.schema, 'lycheedev.data-cli-phases.v1');
  assert.equal(value.timing, DATA_CLI_TIMING);
  assert(['data sql', 'data cache prepare'].includes(value.command));
  assert.equal(typeof value.renderSucceeded, 'boolean');
  for (const key of ['dispatchMicros', 'mainElapsedMicros', 'mainEntryToDispatchMicros', 'renderMicros']) {
    assert(Number.isSafeInteger(value[key]) && value[key] >= 0, `${key}: nonnegative exact integer required`);
  }
  // Individual truncation to micros loses at most two micros across 3 segments.
  const total = BigInt(value.dispatchMicros) + BigInt(value.mainEntryToDispatchMicros) + BigInt(value.renderMicros);
  assert(BigInt(value.mainElapsedMicros) >= total && BigInt(value.mainElapsedMicros) - total <= 2n);
  return value;
}
