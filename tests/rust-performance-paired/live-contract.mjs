// Strict bounded evidence contract. No process launch, clocks, or fabricated
// timings: accepts only complete samples and reports diagnostic distributions.
import assert from 'node:assert/strict';
import {ORACLE} from '../rust-parity/formal-lua-3.2.3/assertions.mjs';
export const RESULT_STRING_BYTES = Object.freeze([256, 16384, 131072]);
export const HISTORY_EVENTS = Object.freeze([1, 255, 256]);
export const REPEATS = 3;
export const SCOPES = Object.freeze(['fixture_create', 'journal_acquire', 'history_seed', 'history_verify',
  'journal_load_at_history', 'history_state_assert', 'journal_checkpoint_at_history', 'checkpoint_verify',
  'journal_append_after_checkpoint', 'peer_startup', 'driver_construct', 'driver_connect', 'driver_prepare',
  'driver_run', 'terminal_correctness', 'driver_completed_retry', 'driver_disconnect', 'state_snapshot',
  'state_serialize', 'state_deserialize', 'state_roundtrip_assert', 'codec_fixture_read', 'codec_decode',
  'codec_correctness', 'terminal_projection_borrowed', 'terminal_serialize_borrowed', 'journal_checked_close',
  'terminal_projection_owned', 'terminal_serialize_owned', 'projection_exact_assert', 'peer_checked_cleanup',
  'journal_failure_cleanup', 'input_identity_recheck']);
const CHECKS = ['exactReport', 'wireRoundtrip', 'checksumSequence', 'checkpointBoundary', 'persistBeforeRelease',
  'noReplay', 'sixOrderedInputs', 'ownedReportPointerCapacity', 'ownedBorrowedExactWire', 'malformedCodecRejected'];
const integer = (value, maximum = Number.MAX_SAFE_INTEGER) => {
  assert(Number.isSafeInteger(value) && value >= 0 && value <= maximum, 'live-offline.nonnegative_bounded_integer'); return value;
};
const sum = values => values.reduce((total, value) => integer(total + integer(value)), 0);
const distribution = values => {
  const sorted = values.toSorted((a, b) => a - b);
  assert.equal(sorted.length, REPEATS);
  return {minimum: sorted[0], median: sorted[1], maximum: sorted[2], samples: sorted.length};
};
export function verifyPerformanceReport(report) {
  assert.equal(report.schema, 'lycheedev.live-offline-performance.v1');
  assert.equal(report.oracle, ORACLE); assert.equal(report.state, 'passed');
  assert(Array.isArray(report.samples)); assert.equal(report.samples.length, 27);
  for (const key of ['scope', 'timing', 'memory']) assert(typeof report[key] === 'string' && report[key].length < 2048);
  assert.match(report.scope, /Rust-only/); assert.match(report.scope, /no game latency/i);
  assert.equal(integer(report.totalWallNanos), sum([report.authenticationWallNanos, report.sampleWallNanos, report.betweenSamplesAndEvidenceWallNanos]));
  assert.equal(report.sampleWallNanos, sum(report.samples.map(row => row.sampleWallNanos)));
  const names = [], hashes = new Map(), groups = new Map();
  for (const row of report.samples) {
    assert.equal(row.schema, 'lycheedev.live-offline-sample.v1');
    const bytes = row.input.resultStringBytes, history = row.input.historyEvents;
    assert(RESULT_STRING_BYTES.includes(bytes)); assert(HISTORY_EVENTS.includes(history)); integer(row.repeat, REPEATS - 1);
    names.push(`string-${bytes}-events-${history}-repeat-${row.repeat}`);
    assert.deepEqual(row.scopes.map(scope => scope.name), SCOPES);
    assert.equal(integer(row.sampleWallNanos), sum([row.unattributedWallNanos, ...row.scopes.map(scope => scope.wallNanos)]));
    assert(row.sampleWallNanos > 0);
    assert.deepEqual(row.completion, {bodyPanicked: false, peerCleanup: {Ok: null}, journalCleanup: 'Ok(())', verified: true});
    assert.deepEqual(row.checks, Object.fromEntries(CHECKS.map(key => [key, true])));
    const body = row.body;
    assert.equal(body.requestedResultStringBytes, bytes);
    assert(integer(body.bodyPayloadBytes, 512 << 10) > bytes);
    assert.match(body.bodySHA256, /^[a-f0-9]{64}$/);
    if (hashes.has(bytes)) assert.equal(hashes.get(bytes), body.bodySHA256, 'live-offline.same_input_body_changed');
    else hashes.set(bytes, body.bodySHA256);
    assert.equal(row.history.inputEvents, history);
    assert(integer(row.history.inputLogBytes, 64 << 20) > 0);
    assert.equal(row.history.afterCheckpointEvents, history === 256 ? 1 : history);
    assert.equal(row.history.appendInputEvents, row.history.afterCheckpointEvents);
    assert(integer(row.history.afterCheckpointLogBytes, 64 << 20) > 0);
    if (history !== 256) assert.equal(row.history.afterCheckpointLogBytes, row.history.inputLogBytes);
    else assert(row.history.afterCheckpointLogBytes < row.history.inputLogBytes);
    const partition = row.driverPartitionNanos;
    assert.equal(integer(partition.total), sum([partition.journal, partition.backendIncludingPeerExchange, partition.driverAndMeasurementRemainder]));
    assert.equal(partition.total, sum(row.scopes.filter(scope => scope.name.startsWith('driver_')).map(scope => scope.wallNanos)));
    for (const [kind, expected] of [['journal', partition.journal], ['backend', partition.backendIncludingPeerExchange]]) {
      const entries = Object.values(row.calls[kind]); assert(entries.length > 0 && entries.length <= 7);
      for (const count of entries) { assert(integer(count.calls, 1024) > 0); integer(count.wallNanos); }
      // Count serializes camelCase; reject accidental alternate schema.
      assert.equal(expected, sum(entries.map(count => count.wallNanos)));
    }
    assert.equal(row.calls.journal.write_result.calls, 1);
    assert.equal(row.calls.backend.input.calls, 6); assert.equal(row.calls.backend.publish.calls, 6);
    assert.equal(row.nestedPeerExchange.calls, 6);
    assert(integer(row.nestedPeerExchange.wallNanos) <= partition.backendIncludingPeerExchange);
    assert.equal(row.nestedPeerExchange.includedIn, 'driverPartitionNanos.backendIncludingPeerExchange');
    assert.match(row.nestedPeerExchange.meaning, /not pure wait or game latency/);
    const peerRecords = row.nestedPeerRecords;
    assert(integer(peerRecords.decodeCalls, 65536) > 0);
    assert(integer(peerRecords.decodeWallNanos) <= partition.backendIncludingPeerExchange);
    assert(integer(peerRecords.readBytes, 512 << 20) >= body.bodyPayloadBytes);
    assert(integer(peerRecords.payloadCopyBytes, 512 << 20) >= body.bodyPayloadBytes);
    assert(peerRecords.payloadCopyBytes <= peerRecords.readBytes);
    assert.equal(peerRecords.includedIn, 'driverPartitionNanos.backendIncludingPeerExchange');
    assert.match(peerRecords.scope, /not native process-memory IO/);
    const memory = row.ownedBytes;
    for (const key of ['totalAllocatorCalls', 'allocatorRequestedBytes', 'peakRSS']) assert.equal(memory[key], null);
    assert.equal(memory.snapshotArtifactCount, 2);
    assert(integer(memory.snapshotArtifactCopyBytes) > body.bodyPayloadBytes);
    assert(integer(memory.snapshotArtifactCapacityBytes) >= memory.snapshotArtifactCopyBytes);
    assert(integer(memory.codecPayloadCopyBytes) >= body.bodyPayloadBytes);
    assert(integer(memory.codecPayloadCapacityBytes) >= memory.codecPayloadCopyBytes);
    assert(integer(memory.codecPayloadVectors, 64) > 0);
    assert.equal(memory.ownedProjectionReportCopyBytes, 0);
    assert(integer(memory.ownedProjectionReportCapacityBytes) >= body.bodyPayloadBytes);
    assert.equal(memory.borrowedProjectionReportCopyBytes, body.bodyPayloadBytes);
    assert(integer(memory.stateSerializedBytes, 2 << 20) > body.bodyPayloadBytes);
    assert(integer(memory.terminalSerializedBytes, 2 << 20) > body.bodyPayloadBytes);
    const key = `${bytes}/${history}`;
    if (!groups.has(key)) groups.set(key, []); groups.get(key).push(row);
  }
  assert.equal(new Set(names).size, 27, 'live-offline.duplicate_or_missing_sample');
  const summaries = [];
  for (const bytes of RESULT_STRING_BYTES) for (const history of HISTORY_EVENTS) {
    const rows = groups.get(`${bytes}/${history}`); assert.equal(rows?.length, REPEATS);
    assert.deepEqual(rows.map(row => row.repeat).toSorted(), [0, 1, 2]);
    summaries.push({resultStringBytes: bytes, bodyPayloadBytes: rows[0].body.bodyPayloadBytes, historyEvents: history,
      sampleWallNanos: distribution(rows.map(row => row.sampleWallNanos)),
      driverWallNanos: distribution(rows.map(row => row.driverPartitionNanos.total)),
      scopes: Object.fromEntries(SCOPES.map((name, index) => [name, distribution(rows.map(row => row.scopes[index].wallNanos))]))});
  }
  return {samples: 27, caseNames: names.toSorted(), groups: summaries, acceptance: 'diagnostic_only', goHostComparison: 'not_available'};
}
