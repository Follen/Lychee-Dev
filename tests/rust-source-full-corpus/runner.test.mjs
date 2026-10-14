// Runner fault tests only. Synthetic assertions are not product/corpus passes.
import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {assertAnchor, assertCorpus, assertEnvelope, assertInspect, assertPageContinuation, assertQuery, assertStaticCall, corpusDigest} from './assertions.mjs';
const oracle = JSON.parse(readFileSync(new URL('./retail-oracle.json', import.meta.url)));
const anchor = oracle.anchors[0], pin = 'PIN-test';
function query() { return {sourceId: oracle.repository, product: oracle.product, resolvedCommit: oracle.commit, snapshotId: pin, results: [{name: anchor.symbol, path: anchor.path, line: anchor.line, matchedBy: 'exact_symbol', contentHash: anchor.sha256, symbolId: 'SYM-test', excerpt: anchor.declarationLine}], complete: true, truncated: false, coverage: {analyzedDocuments: oracle.documents, skippedDocuments: 0, complete: true}}; }
test('pinned independent oracle preserves exact full-corpus identity', () => {
  assert.equal(oracle.commit, '31c7f7b9cc79e56c986b365c06a6afbcf3c9177b');
  assert.equal(oracle.archiveSHA256, 'd4d6e468636d6205d94b912fd96ac469b66ae606e6422d7e3849afbff7a376df');
  assert.equal(oracle.documents, 4036); assert.equal(oracle.documentBytes, 32269051);
  assert.equal(oracle.documentPathSHA256SizeDigest, '88d68f2c4bdb352245cbcb53cd41ec906b294e2ea599ee6374e00be169be7928');
  assert.equal(anchor.line, 97); assert.equal(oracle.anchors[1].line, 2);
});
test('corpus digest is order-independent but detects path, byte count and content changes', () => {
  const rows = [{path: 'a.lua', bytes: 1, sha256: 'a'.repeat(64)}, {path: 'b.xml', bytes: 2, sha256: 'b'.repeat(64)}];
  assert.equal(corpusDigest(rows), corpusDigest([...rows].reverse()));
  for (const key of ['path', 'bytes', 'sha256']) { const changed = structuredClone(rows); changed[0][key] = key === 'bytes' ? 3 : `${changed[0][key]}x`; assert.notEqual(corpusDigest(rows), corpusDigest(changed)); }
});
test('small fixture cannot impersonate full corpus', () => assert.throws(() => assertCorpus([{path: anchor.path, bytes: 1, sha256: anchor.sha256}], oracle), /file count/));
test('public envelope requires matching exit/schema/ok and structured errors', () => {
  assertEnvelope({exit: 0, envelope: {schema: 'lycheedev.result.v1', ok: true, result: {}}});
  for (const value of [{exit: 1}, {exit: 0, error: 'timeout'}, {exit: 0, envelope: {ok: true, result: {}}}]) assert.throws(() => assertEnvelope(value));
  assertEnvelope({exit: 2, envelope: {schema: 'lycheedev.result.v1', ok: false, error: {code: 'bad'}}}, 2);
  assert.throws(() => assertEnvelope({exit: 2, envelope: {schema: 'lycheedev.result.v1', ok: false}}, 2));
});
test('query cannot retarget the pin or reduce coverage silently', () => {
  assertQuery(query(), oracle, pin);
  for (const [key, value] of [['resolvedCommit', '0'.repeat(40)], ['snapshotId', 'PIN-other'], ['product', 'classic']]) { const q = query(); q[key] = value; assert.throws(() => assertQuery(q, oracle, pin)); }
  const partial = query(); partial.coverage.analyzedDocuments--; assert.throws(() => assertQuery(partial, oracle, pin));
});
test('truncation and partial coverage cannot be called complete', () => {
  for (const mutate of [q => { q.truncated = true; }, q => { q.coverage.complete = false; }]) { const q = query(); mutate(q); assert.throws(() => assertQuery(q, oracle, pin)); q.complete = false; assertQuery(q, oracle, pin); }
});
test('anchor checks reject wrong original line, file hash and unreadable excerpt', () => {
  assertAnchor(query(), anchor, oracle, pin);
  for (const [key, value] of [['line', 98], ['contentHash', '0'.repeat(64)], ['excerpt', 'wrong source']]) { const q = query(); q.results[0][key] = value; assert.throws(() => assertAnchor(q, anchor, oracle, pin)); }
});
test('generated API query requires exact table-start line, independent of inspect Name line', () => {
  const anchor = oracle.anchors[2], q = query();
  q.results = [{name: anchor.symbol, path: anchor.path, line: anchor.queryLine, matchedBy: 'exact_symbol', contentHash: anchor.sha256, symbolId: 'SYM-api', excerpt: anchor.declarationLine}];
  assertAnchor(q, anchor, oracle, pin);
  q.results[0].line = anchor.line; assert.throws(() => assertAnchor(q, anchor, oracle, pin), /declaration missing/);
});
test('static call requires exact caller and call kind, never write or unrelated source', () => {
  const row = {...oracle.staticCall, state: 'static_candidate', basis: ['static-extraction']};
  assertStaticCall({relations: [row]}, oracle.staticCall);
  for (const [key, value] of [['kind', 'write'], ['source', 'UnrelatedCaller'], ['line', 156], ['state', 'resolved']]) {
    assert.throws(() => assertStaticCall({relations: [{...row, [key]: value}]}, oracle.staticCall));
  }
});
test('original inspect preserves exact bytes and whole-file identity', () => {
  const result = {repository: oracle.repository, commit: oracle.commit, ...anchor, firstLine: anchor.line, lastLine: anchor.line + anchor.count - 1, blob: {sha256: anchor.sha256}};
  assertInspect(result, anchor, oracle); assert.throws(() => assertInspect({...result, text: anchor.text + '\n'}, anchor, oracle));
});
test('continuation requires more rows, offset and disjoint identities', () => {
  const first = {results: [{symbolId: 'a'}, {symbolId: 'b'}], truncated: true, nextCursor: 'token'};
  const second = {results: [{symbolId: 'c'}], matchCoverage: {offset: 2}};
  assertPageContinuation(first, second, 2);
  assert.throws(() => assertPageContinuation(first, {...second, results: [{symbolId: 'a'}]}, 2));
  assert.throws(() => assertPageContinuation(first, {...second, matchCoverage: {offset: 0}}, 2));
});
