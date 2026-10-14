// Checker fixtures are not product evidence. Real acceptance supplies the fixed
// corpus manifest and actual public CLI/native-session observations.
import assert from 'node:assert/strict';
import {assertCorpus, corpusDigest, sha256} from '../rust-source-full-corpus/assertions.mjs';
import {assertBoundedCoverage, assertNativeEvidence} from '../rust-skill-usage/assertions.mjs';

export const GROUPS = Object.freeze({full: 1200000, quota: 600000, idle: 150000, lifetime: 720000});
export function groupBudget(group) { assert(Object.hasOwn(GROUPS, group), 'unknown acceptance group'); return GROUPS[group]; }
export function assertFullNative(result, oracle, manifest, callsite, {mode = 'one_shot', commit = oracle.commit} = {}) {
  assertBoundedCoverage(result);
  assert.equal(result.commit, commit, 'fixed source identity');
  assert.equal(result.repository, oracle.repository); assert.equal(result.product, oracle.product);
  const native = result.nativeResearch;
  assert.equal(native?.schema, 'lycheedev.source-native-research.v1');
  assert.equal(native.analysis?.engine, 'native_common_facts');
  assert.equal(result.semanticRuntime?.engine, 'native-common-source');
  assert.equal(native.entryEvidence?.sourceExecution, false);
  assert.equal(native.analysis.sourceExecution, false);
  assert.match(native.inputIdentity, /^[a-f0-9]{64}$/);
  assert.equal(native.analysis.inputIdentity, native.inputIdentity);
  assert.deepEqual(result.semanticAnalysis, native.analysis);
  assert.equal(result.coverage.semantic, native.analysis.state);
  assert(['partial', 'complete'].includes(native.analysis.state));
  if (native.analysis.state === 'partial') assert.equal(result.complete, false);
  assert.equal(native.relations, undefined, 'native rows must respect public pagination');
  const files = new Map(manifest.files.map(row => [row.path, row]));
  const expected = manifest.xmlMappings;
  assert.equal(expected?.schema, 'lycheedev.independent-xml-map-oracle.v1', 'independent full XML inventory required');
  assert.equal(expected.xmlDocuments, oracle.counts['.xml']);
  assert.equal(mappingDigest(expected.maps), expected.sha256, 'independent mapping oracle integrity');
  assert(Array.isArray(native.sourceMaps), 'complete XML mapping inventory');
  assert.equal(new Set(native.sourceMaps.map(row => row.path)).size, native.sourceMaps.length);
  for (const map of native.sourceMaps) {
    const original = files.get(map.xmlPath); assert(original, 'XML map original exists in verified corpus');
    assert(map.xmlPath.toLowerCase().endsWith('.xml'));
    assert.equal(map.xmlSHA256, original.sha256); assert.equal(map.xmlBytes, original.bytes);
  }
  const ordered = [...native.sourceMaps].sort((a, b) => Buffer.compare(Buffer.from(a.path), Buffer.from(b.path)));
  assert.equal(mappingDigest(ordered), expected.sha256, 'every independently reconstructed XML mapping and coordinate');
  assert.deepEqual(ordered, expected.maps, 'complete independent XML mapping inventory');
  assert.equal(native.analysis.sourceFiles, oracle.counts['.lua'] + expected.maps.length, 'every Lua input plus independently counted XML inputs');
  assert.deepEqual([...native.loadEvidence.map(row => row.manifest)].sort(), manifest.files.filter(row => row.path.toLowerCase().endsWith('.toc')).map(row => row.path).sort(), 'every fixed TOC supplied to native load planning');
  assert.equal(native.environmentManifest?.identity.commit, commit);
  assert.equal(native.environmentManifest.identity.repository, oracle.repository);
  assert.equal(native.environmentManifest.identity.client, oracle.product);
  assert.equal(native.environmentManifest.documents, manifest.files.filter(row => /\/blizzard_apidocumentationgenerated\/.*\.lua$/i.test(row.path)).length);
  assert.equal(native.semanticPlan?.plan?.schema, 'lycheedev.native-semantic-plan.v1');
  const plan = native.semanticPlan.plan;
  assertExecutionAccounting(native);
  if (mode === 'one_shot') {
    assert.deepEqual(native.lifecycle, {owner: 'source_owned_process', retired: true, mode: 'one_shot'});
    if (callsite) assertNativeEvidence(result, oracle, callsite);
  }
  else {
    assert.equal(mode, 'session'); assert.equal(native.lifecycle, undefined);
    assert(Number.isInteger(native.nativeSession?.workerPid) && native.nativeSession.workerPid > 0);
    assert.equal(native.nativeSession.admittedSemanticBatches, plan.batch_sizes.length);
    assert(native.nativeSession.batches >= plan.batch_sizes.length && native.nativeSession.batches <= 256);
    assert.equal(native.nativeSession.analysisRetired, native.nativeSession.batches === 256);
    if (callsite) {
    const row = result.relations.find(row => row.basis?.includes('native_common_source_facts') && ['path', 'line', 'kind', 'target'].every(key => row[key] === callsite[key]));
    assert(row, 'exact native original callsite'); assert.deepEqual(row.site, callsite.site);
    assert(['resolved', 'static_candidate'].includes(row.state));
    assert(typeof row.proof === 'string' && row.proof.length > 0);
    assert(['known', 'known_absent', 'unavailable', 'not_applicable', 'selector_required'].includes(row.lexical?.state));
    assert(Array.isArray(row.observations));
    for (const observation of row.observations) {
      assert(typeof observation.proof === 'string' && observation.proof.length > 0);
      assert.equal(typeof observation.occurrence?.kind, 'string');
      assert(['known', 'known_absent', 'unavailable', 'not_applicable', 'selector_required'].includes(observation.selected?.state));
    }
    if (row.state === 'resolved') assert(row.observations.length > 0 && row.observations.every(observation => observation.selected?.state === 'known'));
    }
  }
  return native;
}
export function assertExecutionAccounting(native) {
  const planned = native.semanticPlan, plan = planned?.plan, execution = native.semanticExecution, receipt = execution?.receipt;
  assert.equal(execution?.schema, 'lycheedev.native-semantic-execution.v1');
  assert(Array.isArray(plan.operations) && plan.operations.length <= 256);
  const batches = Array.from({length: Math.ceil(plan.operations.length / 128)}, (_, index) => Math.min(128, plan.operations.length - index * 128));
  assert.deepEqual(plan.batch_sizes, batches, 'actual logical operation batch boundaries');
  assert(Array.isArray(plan.unavailable) && plan.unavailable.every(reason => typeof reason === 'string'));
  assert.equal(receipt?.plan_identity, semanticPlanDigest(plan));
  assert.equal(receipt.planned_batches, batches.length);
  for (const field of ['completed_batches', 'scopes', 'unavailable_scopes', 'uncertain_answers', 'unexecuted_operations']) assert(Number.isSafeInteger(receipt[field]) && receipt[field] >= 0, `bounded receipt ${field}`);
  assert(Array.isArray(execution.answers), 'actual scope answers required for execution accounting');
  let scopes = 0, unavailable = 0, uncertain = 0, unexecuted = 0, partialResult = false;
  const completed = batches.map(() => true);
  // Recompute ResearchExecution::scope/finish from every actual answer. Receipt
  // counters are outputs to verify, never the source of completeness credit.
  for (const scope of execution.answers) {
    if (scope.state === 'unavailable') {
      assert.equal(scope.operationsExecuted, false); assert.equal(scope.operations, undefined); unavailable++; continue;
    }
    assert(scope.scope && typeof scope.scope === 'object' && !Array.isArray(scope.scope));
    assert(Array.isArray(scope.operations) && scope.operations.length === plan.operations.length, 'one ordered answer for every logical operation'); scopes++;
    for (const [index, answer] of scope.operations.entries()) {
      const operation = plan.operations[index], batch = Math.floor(index / 128);
      assert.equal(answer.operation, index); assert.equal(answer.batch, batch); assert.equal(answer.kind, operation.kind);
      assert.equal(answer.operationIdentity, semanticOperationDigest(operation));
      assert.equal(typeof answer.executed, 'boolean'); assert.equal(typeof answer.state, 'string');
      if (!answer.executed) { unexecuted++; completed[batch] = false; }
      if (!['known', 'known_declaration'].includes(answer.state)) uncertain++;
      if (answer.result?.analysis) {
        assert(['complete', 'partial'].includes(answer.result.analysis.state), 'actual answer analysis state');
        if (answer.result.analysis.state === 'partial') partialResult = true;
      }
    }
  }
  assert.equal(receipt.scopes, scopes); assert.equal(receipt.unavailable_scopes, unavailable);
  assert.equal(receipt.uncertain_answers, uncertain); assert.equal(receipt.unexecuted_operations, unexecuted);
  assert.equal(receipt.completed_batches, scopes > 0 && unavailable === 0 ? completed.filter(Boolean).length : 0, 'completed batches require every operation in every admitted scope');
  assert.equal(planned.resourceAccounting.executedSemanticBatches, receipt.completed_batches);
  assert.equal(planned.state, unavailable || unexecuted ? 'execution_incomplete' : 'executed');
  if (plan.unavailable.length || unavailable || uncertain || unexecuted || partialResult) assert.equal(native.analysis.state, 'partial', 'incomplete plan, scope or actual answer cannot claim complete analysis');
  return {scopes, unavailable, uncertain, unexecuted, completedBatches: receipt.completed_batches};
}
export function assertReuse(first, next, {changed = false} = {}) {
  assert.equal(next.nativeSession.workerPid, first.nativeSession.workerPid, 'same retained owner');
  assert.equal(next.nativeSession.snapshotTransfer, changed ? 'full' : 'identity_only');
  assert.equal(next.analysis.cacheHit, !changed);
  assert.equal(next.nativeSession.cacheHit, !changed);
  if (changed) assert.notEqual(next.inputIdentity, first.inputIdentity);
  else assert.equal(next.inputIdentity, first.inputIdentity);
}
export function assertMaterial(material, manifest, oracle, commit = oracle.commit) {
  assert.equal(material.schema, 'lycheedev.source-research-material.v3');
  assert.equal(material.snapshot?.source?.exactCommit, commit);
  assert.equal(material.snapshot.source.repository, oracle.repository); assert.equal(material.snapshot.source.product, oracle.product);
  const snapshot = material.snapshot, rows = [];
  for (const [kind, collection] of [['lua', snapshot.files], ['load', snapshot.load_documents]]) {
    assert(collection && typeof collection === 'object' && !Array.isArray(collection));
    for (const [path, text] of Object.entries(collection)) {
      assert.equal(typeof text, 'string'); assert.equal(/\.lua$/i.test(path), kind === 'lua');
      assert(/\.(lua|xml|toc)$/i.test(path));
      rows.push({path, bytes: Buffer.byteLength(text), sha256: sha256(Buffer.from(text))});
    }
  }
  assert.equal(rows.length, oracle.documents); assert.equal(new Set(rows.map(row => row.path)).size, rows.length);
  assert.equal(corpusDigest(rows), oracle.documentPathSHA256SizeDigest, 'actual persisted native material contains every unchanged original document');
  const original = new Map(manifest.files.map(row => [row.path, row]));
  assert.deepEqual(Object.keys(snapshot.environment_documents).sort(), manifest.files.filter(row => /\/blizzard_apidocumentationgenerated\/.*\.lua$/i.test(row.path)).map(row => row.path).sort(), 'all independent API originals');
  for (const [path, text] of Object.entries(snapshot.environment_documents)) {
    assert(/\/blizzard_apidocumentationgenerated\/.*\.lua$/i.test(path));
    assert.equal(sha256(Buffer.from(text)), original.get(path)?.sha256);
  }
  assert.equal(Object.keys(snapshot.environment_documents).length, snapshot.environment_manifest.documents);
  return {sourceDocuments: rows.length, sourceBytes: rows.reduce((n, row) => n + row.bytes, 0), documentDigest: corpusDigest(rows), environmentDocuments: Object.keys(snapshot.environment_documents).length, source: snapshot.source};
}
export function assertPrerequisite(report, manifest, oracle) {
  assert.equal(report.state, 'passed_selected_static_cases');
  assert.equal(report.oracle.commit, oracle.commit); assert.equal(report.oracle.archiveSHA256, oracle.archiveSHA256);
  assert.equal(manifest.commit, oracle.commit); assertCorpus(manifest.files, oracle);
  assert(report.checks.length >= 20 && report.checks.every(row => row.state === 'passed'));
  assert(report.commands.length >= 25 && report.commands.every(row => !row.error && row.signal === null));
  assert.match(report.pin, /^PIN-[a-f0-9]{64}$/); return report.pin;
}
export function fixturePin(source) {
  // Rust PinnedSet/SourcePin serialization order, not JSON object-key sorting.
  const pin = {schema: 'lycheedev.selection.v1', id: '', source: {repository: source.repository, product: source.product, requestedRef: source.requestedRef, exactCommit: source.exactCommit, parserRevision: source.parserRevision}};
  pin.id = `PIN-${sha256(Buffer.from(JSON.stringify(pin).replaceAll('&', '\\u0026').replaceAll('<', '\\u003c').replaceAll('>', '\\u003e').replaceAll('\u2028', '\\u2028').replaceAll('\u2029', '\\u2029')))}`;
  return pin;
}

export function assertApiType(native, anchor) {
  const operations = native.semanticExecution.answers.flatMap(scope => scope.operations ?? []);
  const typed = operations.filter(operation => operation.kind === 'type' && operation.result?.bindingSelection === 'authenticated_environment_declaration');
  assert(typed.length > 0, 'real independent API type inference required');
  for (const answer of typed) {
    assert.equal(answer.executed, true); assert.equal(answer.state, 'known');
    assert.equal(answer.result.sourceExecution, false);
    assert(answer.result.coverage?.some(row => row.state === 'bound'), 'bound semantic model evidence');
    const target = answer.result.target;
    assert.equal(target.site.file, '@native/api-definitions.lua');
    assert.equal(target.file_sha256, native.environmentManifest.definitionsSHA256);
    assert.deepEqual(target.material.manifest, native.environmentManifest);
    assert.equal(target.material.mapping.callable, anchor.symbol);
    assert.equal(target.material.mapping.sourcePath, anchor.path);
    assert.equal(target.material.mapping.sourceLine, anchor.queryLine ?? anchor.line, 'exact original API declaration table line');
    assert(typeof answer.result.type === 'string' && answer.result.type.length > 0 && answer.result.type.toLowerCase() !== 'unknown');
  }
  return typed;
}

// Independent acceptance-side byte tokenizer for the fixed Ui/Bindings corpus.
// It does not consume returned sourceMaps or invoke the production extractor.
// Unsupported structural forms fail instead of silently dropping scripts.
// This fixed-corpus mapping reconstructor is not a general XML validator.
export function rebuildXmlInventory(originals, manifest) {
  const maps = [];
  for (const file of manifest.files.filter(row => /\.xml$/i.test(row.path))) {
    const raw = originals.get(file.path); assert(Buffer.isBuffer(raw), `missing original XML: ${file.path}`);
    assert.equal(raw.length, file.bytes); assert.equal(sha256(raw), file.sha256);
    maps.push(...extractXmlMaps(file.path, raw));
  }
  maps.sort((a, b) => Buffer.compare(Buffer.from(a.path), Buffer.from(b.path)));
  return {schema: 'lycheedev.independent-xml-map-oracle.v1', xmlDocuments: manifest.files.filter(row => /\.xml$/i.test(row.path)).length, maps, sha256: mappingDigest(maps)};
}
export function mappingDigest(maps) {
  const canonical = value => Array.isArray(value) ? value.map(canonical) : value && typeof value === 'object' ? Object.fromEntries(Object.keys(value).sort().map(key => [key, canonical(value[key])])) : value;
  return sha256(Buffer.from(JSON.stringify(canonical(maps))));
}
export function extractXmlMaps(path, raw) {
  assert(Buffer.isBuffer(raw) && raw.length <= 32 << 20, 'independent XML byte budget');
  // Fatal decoding prevents a replacement character from hiding offset drift.
  new TextDecoder('utf-8', {fatal: true}).decode(raw);
  const originalSHA256 = sha256(raw), maps = [], stack = []; let cursor = raw.subarray(0, 3).equals(Buffer.from([239, 187, 191])) ? 3 : 0, body, roots = 0, profile;
  const ascii = (start, end) => raw.subarray(start, end).toString('utf8');
  const append = (text, span, origin, anchor) => {
    if (!text.length) return;
    const start = body.length, end = start + Buffer.byteLength(text); assert(end <= 16 << 20, 'independent decoded XML budget');
    const last = body.segments.at(-1);
    if (origin === 'Exact' && last?.origin === 'Exact' && last.output.end === start && last.raw.end === span.start) { last.output.end = end; last.raw.end = span.end; }
    else { const segment = {output: {start, end}, raw: span, origin}; if (anchor) segment.anchor = anchor; body.segments.push(segment); }
    assert(body.segments.length <= 200000, 'independent XML map segment budget'); body.parts.push(text); body.length = end;
  };
  const physical = (start, end) => {
    let at = start;
    for (let i = start; i < end; i++) if (raw[i] === 13) {
      append(ascii(at, i), {start: at, end: i}, 'Exact'); const stop = i + (raw[i + 1] === 10 && i + 1 < end ? 2 : 1);
      append('\n', {start: i, end: stop}, 'NewlineNormalization'); i = stop - 1; at = stop;
    }
    append(ascii(at, end), {start: at, end}, 'Exact');
  };
  const entity = (start, end) => {
    const value = ascii(start + 1, end - 1), names = {amp: '&', lt: '<', gt: '>', apos: "'", quot: '"'};
    let decoded = names[value];
    if (!decoded && /^#(?:[0-9]+|x[0-9a-fA-F]+)$/.test(value)) {
      const point = value[1] === 'x' ? Number.parseInt(value.slice(2), 16) : Number(value.slice(1));
      assert(Number.isInteger(point) && point <= 0x10ffff && ![0xfffe, 0xffff].includes(point) && !(point >= 0xd800 && point <= 0xdfff) && (point >= 32 || [9, 10, 13].includes(point)), 'independent XML entity scalar');
      decoded = String.fromCodePoint(point);
    }
    assert(decoded, 'independent XML unknown entity'); append(decoded, {start, end}, 'Entity');
  };
  while (cursor < raw.length) {
    assert(stack.length <= 256 && maps.length <= 10000, 'independent XML structural budget');
    if (raw[cursor] !== 60) {
      let end = raw.indexOf(60, cursor); if (end < 0) end = raw.length;
      if (body) {
        let at = cursor;
        while (at < end) { const amp = raw.indexOf(38, at); if (amp < 0 || amp >= end) { physical(at, end); break; } physical(at, amp); const semi = raw.indexOf(59, amp); assert(semi >= 0 && semi < end, 'independent XML unterminated entity'); entity(amp, semi + 1); at = semi + 1; }
      } else if (!stack.length) assert(/^\s*$/.test(ascii(cursor, end)), 'independent XML outside-root text');
      cursor = end; continue;
    }
    if (ascii(cursor, cursor + 4) === '<!--') { const end = raw.indexOf('-->', cursor + 4); assert(end >= 0, 'independent XML comment'); cursor = end + 3; continue; }
    if (ascii(cursor, cursor + 9) === '<![CDATA[') { const end = raw.indexOf(']]>', cursor + 9); assert(end >= 0 && stack.length, 'independent XML CDATA'); if (body) physical(cursor + 9, end); cursor = end + 3; continue; }
    if (ascii(cursor, cursor + 5) === '<?xml') { const end = raw.indexOf('?>', cursor + 5); assert(end >= 0 && roots === 0, 'independent XML declaration'); cursor = end + 2; continue; }
    assert(raw[cursor + 1] !== 33 && raw[cursor + 1] !== 63, 'independent XML DTD/PI unsupported');
    let end = cursor + 1, quote = 0;
    for (; end < raw.length; end++) { const c = raw[end]; if (quote) { if (c === quote) quote = 0; } else if (c === 34 || c === 39) quote = c; else if (c === 62) break; }
    assert(end < raw.length && !quote, 'independent XML unterminated tag'); end++;
    const text = ascii(cursor, end), closing = text.startsWith('</'), empty = /\/\s*>$/.test(text);
    const match = /^<\/?([A-Za-z_][A-Za-z0-9_.-]*)(?=[\s/>])/.exec(text); assert(match, 'independent XML supported tag name'); const tag = match[1];
    if (closing) {
      assert.equal(stack.pop(), tag, 'independent XML balanced tag');
      if (body) {
        assert.equal(body.tag, tag); const decoded = body.parts.join('');
        if (body.kind !== 'binding' || decoded.length) {
          const decodedBytes = Buffer.from(decoded), decodedSegments = body.segments, kind = body.kind, start = body.start, prefix = kind === 'handler' ? 'return function(self, ...)\n' : kind === 'binding' ? 'return function(...)\n' : '';
          body = {...body, parts: [], segments: [], length: 0};
          if (prefix) append(prefix, null, 'Synthetic', {start, end: start});
          for (const segment of decodedSegments) append(decodedBytes.subarray(segment.output.start, segment.output.end).toString('utf8'), segment.raw, segment.origin, segment.anchor);
          if (prefix) append('\nend\n', null, 'Synthetic', {start: cursor, end});
          const compiler = Buffer.from(body.parts.join(''));
          maps.push({rules: 'xml-segmented-original-utf8-v1', path: `${path}/__inline_${maps.length}.lua`, xmlPath: path, xmlSHA256: originalSHA256, xmlBytes: raw.length, compilerSHA256: sha256(compiler), compilerBytes: compiler.length, segments: body.segments, element: {start, end}, wrapper: 'syntax_only_closure', implicitHandlerParameters: kind === 'handler' ? 'self:xml_object; remaining:unknown' : 'unknown', byteOffsets: 'original_utf8_including_bom', positionEncoding: 'utf-16', bomColumnPolicy: 'encoding_marker_not_displayed'});
        }
        body = undefined;
      }
    } else {
      assert(!body, 'independent XML nested script markup');
      if (!stack.length) { assert.equal(++roots, 1); assert(['Ui', 'Bindings'].includes(tag)); profile = tag; }
      const kind = profile === 'Bindings' ? tag === 'Binding' && stack.length === 1 ? 'binding' : null : tag === 'Script' ? 'top_level' : stack.at(-1) === 'Scripts' ? 'handler' : null;
      if (kind && !empty) body = {kind, tag, start: cursor, parts: [], segments: [], length: 0};
      if (!empty) stack.push(tag);
    }
    cursor = end;
  }
  assert.equal(roots, 1); assert.equal(stack.length, 0); assert.equal(body, undefined); return maps;
}

export function assertContextSnippets(result, originals, manifest, {maxBytes, maxLines, limit, anchor, depth = 0}) {
  assert(Array.isArray(result.snippets) && result.snippets.length > 0 && result.snippets.length <= limit, 'bounded nonempty context snippets');
  if (depth === 0) assert.equal(result.snippets.length, 1, 'depth-zero definition snippet only');
  const rows = new Map(manifest.files.map(row => [row.path, row])); let bytes = 0, lines = 0; const seen = new Set();
  for (const snippet of result.snippets) {
    const original = originals.get(snippet.path), file = rows.get(snippet.path); assert(file && Buffer.isBuffer(original), 'snippet authenticated original path');
    assert.equal(original.length, file.bytes); assert.equal(sha256(original), file.sha256); assert.equal(snippet.contentHash, file.sha256, 'snippet whole-file hash');
    const {firstLine: first, lastLine: last} = snippet; assert(Number.isInteger(first) && Number.isInteger(last) && first > 0 && last >= first, 'snippet line bounds');
    const rawLines = original.toString('utf8').replaceAll('\r\n', '\n').split('\n'); assert(last <= rawLines.length, 'snippet lines inside original');
    const expected = rawLines.slice(first - 1, last).map((text, i) => `${first + i}: ${text.replace(/\r+$/, '')}`).join('\n');
    assert.equal(snippet.text, expected, 'exact numbered original context text');
    const key = `${snippet.path}\0${first}\0${last}`; assert(!seen.has(key), 'duplicate snippet range'); seen.add(key);
    bytes += Buffer.byteLength(snippet.text); lines += last - first + 1;
  }
  assert(bytes <= maxBytes && lines <= maxLines, 'context total byte/line budgets');
  const anchorLines = anchor.text.trimEnd().split('\n');
  const definitionLast = anchor.line + anchorLines.length - 1;
  assert(result.snippets.some(row => row.path === anchor.path && row.firstLine === anchor.line && row.lastLine === definitionLast && row.basis === 'definition'), 'exact independently fixed definition range present');
  return {snippets: result.snippets.length, bytes, lines, exactOriginals: true};
}

export function assertNestedControllerRetired(observation, nested) {
  // A failed nested controller may have killed only its direct CLI while a
  // retained worker/grandchild is still alive. Only the outer Job can reap it.
  const uncertain = observation.exit !== 0 || observation.error || observation.signal !== null || !nested
    || !Array.isArray(nested.commands) || nested.commands.some(row => row.error || row.signal !== null)
    || nested.fatalProcessFailure === true;
  if (uncertain) { const error = new Error('nested controller process failure; outer Job retirement required'); error.fatalProcessFailure = true; throw error; }
}

// Match the declared Rust struct/enum field order for wire hashes. Nested
// serde_json::Value maps are sorted, unlike their containing Rust structs.
const canonicalValue = value => Array.isArray(value) ? value.map(canonicalValue) : value && typeof value === 'object' ? Object.fromEntries(Object.keys(value).sort().map(key => [key, canonicalValue(value[key])])) : value;
function wireOperation(operation) {
  const target = operation.target;
  return {kind: operation.kind, target: {site: {file: target.site.file, start: target.site.start, end: target.site.end}, file_sha256: target.file_sha256, position: canonicalValue(target.position), material: canonicalValue(target.material)}};
}
export const semanticOperationDigest = operation => sha256(Buffer.from(JSON.stringify(wireOperation(operation))));
export function semanticPlanDigest(plan) {
  return sha256(Buffer.from(JSON.stringify({schema: plan.schema, source_identity: plan.source_identity, query_identity: plan.query_identity, operations: plan.operations.map(wireOperation), batch_sizes: plan.batch_sizes, unavailable: plan.unavailable, outgoing_truncated: plan.outgoing_truncated, scanner_diagnostics: canonicalValue(plan.scanner_diagnostics), scanner_diagnostics_omitted: plan.scanner_diagnostics_omitted})));
}
export function assertExecutedContext(native, originals, anchor) {
  assertExecutionAccounting(native);
  const raw = originals.get(anchor.path); assert(Buffer.isBuffer(raw)); assert.equal(sha256(raw), anchor.sha256);
  let lineStart = 0;
  for (let line = 1; line < anchor.line; line++) { lineStart = raw.indexOf(10, lineStart) + 1; assert(lineStart > 0, 'original declaration line'); }
  const lineEnd = raw.indexOf(10, lineStart), line = raw.subarray(lineStart, lineEnd < 0 ? raw.length : lineEnd), name = Buffer.from(anchor.symbol);
  const startInLine = line.indexOf(name); assert(startInLine >= 0 && line.lastIndexOf(name) === startInLine, 'unique original declaration spelling');
  const site = {file: anchor.path, start: lineStart + startInLine, end: lineStart + startInLine + name.length};
  const position = offset => {
    const prefix = raw.subarray(0, offset).toString('utf8').replace(/^\uFEFF/, '').replaceAll('\r\n', '\n').replaceAll('\r', '\n').split('\n');
    return {line: prefix.length - 1, character: prefix.at(-1).length};
  };
  const range = {start: position(site.start), end: position(site.end)}, plan = native.semanticPlan.plan, execution = native.semanticExecution;
  assert.equal(plan.source_identity, native.inputIdentity, 'plan belongs to the actual native input');
  assert.equal(execution.receipt.plan_identity, semanticPlanDigest(plan), 'execution receipt binds complete actual plan');
  assert(execution.receipt.scopes > 0, 'at least one actual execution scope');
  const references = plan.operations.map((operation, index) => ({operation, index})).filter(({operation}) => operation.kind === 'references' && operation.target?.site?.file === anchor.path);
  assert.equal(references.length, 1, 'one selected-definition reference operation');
  const {operation, index} = references[0], target = operation.target;
  assert.deepEqual(target.site, site, 'references target is exact original declaration token');
  assert.equal(target.file_sha256, anchor.sha256); assert.deepEqual(target.material, {kind: 'source'});
  assert.equal(target.position.path, anchor.path); assert.deepEqual(target.position.bytes, {start: site.start, end: site.end});
  assert.deepEqual(target.position.range, range); assert.deepEqual(target.position.coveringRange, range); assert.equal(target.position.positionAccuracy, 'utf16-character');
  const identity = semanticOperationDigest(operation), matches = [];
  for (const scope of execution.answers ?? []) for (const answer of scope.operations ?? []) {
    if (answer.operation !== index || answer.kind !== 'references') continue;
    assert.equal(answer.operationIdentity, identity, 'executed answer binds exact original operation');
    assert.equal(answer.batch, Math.floor(index / 128));
    if (answer.executed !== true) continue;
    assert(['known', 'partial'].includes(answer.state));
    assert.match(scope.scope?.snapshot, /^[a-f0-9]{64}$/);
    assert(native.loadEvidence.some(row => row.manifest === scope.scope?.manifest), 'actual scope belongs to admitted load plan');
    assert.match(answer.result?.relationsSHA256, /^[a-f0-9]{64}$/); assert(Number.isInteger(answer.result.relationCount) && answer.result.relationCount > 0);
    for (const field of ['engine', 'inputIdentity', 'sourceFiles', 'sourceExecution']) assert.deepEqual(answer.result.analysis?.[field], native.analysis[field], `actual reference result ${field}`);
    matches.push({operation: index, operationIdentity: identity, batch: answer.batch, state: answer.state, manifest: scope.scope.manifest, snapshot: scope.scope.snapshot, relationCount: answer.result.relationCount});
  }
  assert(matches.length > 0, 'context needs an actually executed, identity-bound references answer, not plan/metadata alone');
  return {site, planIdentity: execution.receipt.plan_identity, executedReferences: matches};
}
