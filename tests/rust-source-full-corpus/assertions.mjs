import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';

export const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');
export function corpusDigest(rows) {
  const hash = createHash('sha256');
  for (const row of [...rows].sort((a, b) => Buffer.compare(Buffer.from(a.path), Buffer.from(b.path)))) {
    const size = Buffer.alloc(8); size.writeBigUInt64LE(BigInt(row.bytes));
    hash.update(row.path).update('\0').update(row.sha256).update('\0').update(size);
  }
  return hash.digest('hex');
}
export function assertCorpus(rows, oracle) {
  assert.equal(rows.length, oracle.allFiles, 'all tracked file count');
  assert.equal(new Set(rows.map(row => row.path)).size, rows.length, 'duplicate source path');
  assert.equal(rows.reduce((n, row) => n + row.bytes, 0), oracle.allBytes, 'all tracked bytes');
  assert.equal(corpusDigest(rows), oracle.allPathSHA256SizeDigest, 'every original file path, hash and size');
  const documents = rows.filter(row => /\.(lua|xml|toc)$/i.test(row.path));
  assert.equal(documents.length, oracle.documents, 'complete document count');
  assert.equal(documents.reduce((n, row) => n + row.bytes, 0), oracle.documentBytes, 'complete document bytes');
  assert.equal(corpusDigest(documents), oracle.documentPathSHA256SizeDigest, 'every source document identity');
  for (const [extension, expected] of Object.entries(oracle.counts)) {
    assert.equal(documents.filter(row => row.path.toLowerCase().endsWith(extension)).length, expected, extension);
  }
  return documents;
}
export function assertEnvelope(observation, expectedExit = 0) {
  assert.equal(observation.error, undefined, 'CLI process error');
  assert.equal(observation.exit, expectedExit, 'CLI exit');
  const envelope = observation.envelope;
  assert.equal(envelope?.schema, 'lycheedev.result.v1', 'public envelope schema');
  assert.equal(envelope.ok, expectedExit === 0, 'public envelope ok');
  if (expectedExit === 0) assert(envelope.result !== undefined, 'missing result');
  else assert.equal(typeof envelope.error?.code, 'string', 'structured error');
  return expectedExit === 0 ? envelope.result : envelope.error;
}
export function assertQuery(result, oracle, pin) {
  assert.equal(result.sourceId, oracle.repository, 'query repository');
  assert.equal(result.product, oracle.product, 'query product');
  assert.equal(result.resolvedCommit, oracle.commit, 'query exact commit');
  assert.equal(result.snapshotId, pin, 'query snapshot');
  assert(Array.isArray(result.results), 'query results');
  assert.equal(typeof result.truncated, 'boolean', 'explicit truncation');
  assert.equal(typeof result.complete, 'boolean', 'explicit completion');
  assert.equal(result.coverage?.analyzedDocuments, oracle.documents, 'full query document coverage');
  assert.equal(result.coverage.skippedDocuments, 0, 'no silently omitted source');
  if (result.truncated) assert.equal(result.complete, false, 'truncated query is not complete');
  if (result.coverage.complete === false) assert.equal(result.complete, false, 'partial coverage is not complete');
  return result;
}
export function assertAnchor(result, anchor, oracle, pin) {
  assertQuery(result, oracle, pin);
  const line = anchor.queryLine ?? anchor.line;
  const row = result.results.find(row => row.name === anchor.symbol && row.path === anchor.path && row.line === line);
  assert(row, `exact original declaration missing: ${anchor.symbol} ${anchor.path}:${line}`);
  assert.equal(row.matchedBy, 'exact_symbol', 'exact declaration is identifiable');
  assert.equal(row.contentHash, anchor.sha256, 'declaration original file hash');
  assert.equal(typeof row.symbolId, 'string', 'symbol identity for relations');
  assert(row.excerpt.includes(anchor.declarationLine.trim()), 'readable original declaration excerpt');
  return row;
}
export function assertStaticCall(result, expected) {
  assert(Array.isArray(result.relations), 'relation rows');
  const row = result.relations.find(row => ['source', 'target', 'kind', 'path', 'line'].every(key => row[key] === expected[key]));
  assert(row, `original ${expected.source} -> ${expected.target} call missing at ${expected.path}:${expected.line}`);
  assert.equal(row.state, 'static_candidate', 'literal call is not proved semantic binding');
  assert.deepEqual(row.basis, ['static-extraction'], 'static call evidence basis');
  return row;
}
export function assertInspect(result, anchor, oracle) {
  assert.equal(result.repository, oracle.repository);
  assert.equal(result.commit, oracle.commit);
  for (const field of ['path', 'text', 'totalLines']) assert.equal(result[field], anchor[field], `inspect ${field}`);
  assert.equal(result.firstLine, anchor.line);
  assert.equal(result.lastLine, anchor.line + anchor.count - 1);
  assert.equal(result.blob?.sha256, anchor.sha256, 'whole original file evidence hash');
}
export function assertPageContinuation(first, second, limit) {
  assert.equal(first.results.length, limit);
  assert.equal(first.truncated, true);
  assert.equal(typeof first.nextCursor, 'string');
  assert(first.nextCursor.length > 0);
  assert.equal(second.matchCoverage?.offset, limit, 'cursor advances originating page');
  assert(second.results.length > 0, 'second page contains more results');
  const key = row => row.symbolId || `${row.path}:${row.line}:${row.kind}:${row.name}`;
  const seen = new Set(first.results.map(key));
  assert(second.results.every(row => !seen.has(key(row))), 'no repeated rows across pages');
}
