// Dedicated B01 describe and first fixed-Retail index/query comparison only.
import assert from 'node:assert/strict';
import {isDeepStrictEqual} from 'node:util';
import {assertAnchor, assertEnvelope} from '../rust-source-full-corpus/assertions.mjs';

export const GO_COMMIT = '29bdbdaff5f064ec212ff2064380b1ccac652eda';
export const GO_VERSION = '3.2.4';
// Reviewed B01 describe differences between the external fixed Go 3.2.4
// reference and the current product. The current command directory was
// intentionally reorganized: commands only present in the reference are dropped,
// commands added by the current product are not compared, and the named
// summary/flag substitutions are the only row-level normalization allowed.
// Everything else (formats, development, remaining paths and row fields) must
// match exactly; an undeclared extra field or row fails the comparison.
export const APPROVED_ENGINE_DIFFERENCES = Object.freeze({
  resultKeys: Object.freeze(["constraints","defaults","effects","examples","mutuallyExclusive","oneOfRequired","readOnly","required"]),
  describeResultKeys: Object.freeze({
    base: Object.freeze(["commands","development","formats"]),
    addedByRust: Object.freeze(["discovery","exitCodes","products","result","schema"]),
  }),
  commandKeys: Object.freeze(["path","summary","mutates","arguments","flags"]),
  goOnlyCommands: Object.freeze(["data creature display","data creature model","data db2","data db2 foreign-key","data db2 schema","data db2 search","data db2 stream","data decor get","data decor list","data encounter get","data item geosets","data item get","data item models","data item textures","data spell auras","data spell info","data spell summons","data sql","live abandon","live cancel"]),
  rustOnlyCommands: Object.freeze(["cli search","data agent schema","data agent serve","data cache prepare","schema"]),
  summaryChanges: Object.freeze([
    {
      "path": "cache prune",
      "actual": "Reclaim ordinary cache toward --target-bytes <n> (zero reclaims all ordinary cache), capped by --max-objects <n>; --uncommitted also reclaims staging files; --dry-run reports without deleting; protected and in-use objects are always skipped; --data-derived selects rebuildable Data objects and generations only (not downloads, shared blobs or CAP), with a remaining-byte target and distinct object-group attempt cap; staging cannot be combined",
      "expected": "Reclaim ordinary cache toward --target-bytes <n> (zero reclaims all ordinary cache), capped by --max-objects <n>; --uncommitted also reclaims staging files; --dry-run reports without deleting; protected and in-use objects are always skipped"
    },
    {
      "path": "doctor",
      "actual": "Aggregate read-only workspace, data, embedded Emmy worker smoke and source checks; when a live target or CON is selected, include the same checks as live doctor; exit 3 means an unhealthy check",
      "expected": "Aggregate read-only workspace, data, LuaLS and source checks; when a live target or CON is selected, include the same checks as live doctor; exit 3 means an unhealthy check"
    },
    {
      "path": "source batch",
      "actual": "Run 1-32 query/refs/context requests from a JSON file against one fixed snapshot, sharing preparation and a request-scoped native Emmy session; each item retains its own cursor and coverage; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins",
      "expected": "Run 1-32 query/refs/context requests from a JSON file against one fixed snapshot, sharing preparation and a request-scoped LuaLS session; each item retains its own cursor and coverage; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins"
    },
    {
      "path": "source session close",
      "actual": "Retire the verified opt-in native Source session and release its worktree lease",
      "expected": "Retire the verified opt-in LuaLS broker and release its worktree lease"
    },
    {
      "path": "source session status",
      "actual": "Inspect the opt-in native Source session for this home without starting it",
      "expected": "Inspect the opt-in LuaLS broker for this home without starting it"
    },
    {
      "path": "source validate",
      "actual": "Validate one pinned addon closure with --path/--toc/--snapshot, or a fixed multi-client --matrix <config.json>; add --semantic for native Emmy analysis using fixed client API metadata; report unresolved coverage; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins",
      "expected": "Validate one pinned addon closure with --path/--toc/--snapshot, or a fixed multi-client --matrix <config.json>; add --semantic for bundled LuaLS using fixed client API metadata; report unresolved coverage; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins"
    }
  ].map(Object.freeze)),
  flagChanges: Object.freeze([
    {
      "path": "cache prune",
      "actual": [
        "--home <root>",
        "--format text|json|jsonl",
        "--target-bytes <n>",
        "--max-objects <n>",
        "--uncommitted",
        "--dry-run",
        "--data-derived"
      ],
      "expected": [
        "--home <root>",
        "--format text|json|jsonl",
        "--target-bytes <n>",
        "--max-objects <n>",
        "--uncommitted",
        "--dry-run"
      ]
    },
    {
      "path": "cache status",
      "actual": [
        "--data",
        "--home <root>",
        "--format text|json|jsonl"
      ],
      "expected": [
        "--home <root>",
        "--format text|json|jsonl"
      ]
    },
    {
      "path": "data hotfix",
      "actual": [
        "--home <root>",
        "--format text|json|jsonl",
        "--project <directory>",
        "--source <wago|dbcache|raidbots>",
        "--snapshot <pin>",
        "--dbcache <DBCache.bin>",
        "--raidbots <DBCache.bin>",
        "--from <cache-capture-or-time>",
        "--to <time>",
        "--product <retail|classic|classic-era|titan|forever-asia-beta|forever-cn-beta|anniversary>",
        "--build <full-build>",
        "--region <us|eu|cn|kr|tw>",
        "--locale <locale>",
        "--table <name>",
        "--table-hash <8-hex>",
        "--record <n>",
        "--push <n>",
        "--status <0-255>",
        "--region-id <n>",
        "--search <text>",
        "--latest",
        "--scan (dbcache: complete bounded scan; --cursor resumes its capture)",
        "--offline",
        "--limit <1-200>",
        "--cursor <opaque>",
        "--page <n>",
        "--after-index <n>",
        "--max-pages <n>",
        "--max-requests <n>",
        "--max-bytes <n>",
        "--encoding csv",
        "--output <file>"
      ],
      "expected": [
        "--home <root>",
        "--format text|json|jsonl",
        "--project <directory>",
        "--source <wago|dbcache|raidbots>",
        "--snapshot <pin>",
        "--dbcache <DBCache.bin>",
        "--raidbots <DBCache.bin>",
        "--from <cache-capture-or-time>",
        "--to <time>",
        "--product <retail|classic|classic-era|titan|forever-asia-beta|forever-cn-beta|anniversary> (legacy: forever)",
        "--build <full-build>",
        "--region <us|eu|cn|kr|tw>",
        "--locale <locale>",
        "--table <name>",
        "--table-hash <8-hex>",
        "--record <n>",
        "--push <n>",
        "--status <0-255>",
        "--region-id <n>",
        "--search <text>",
        "--latest",
        "--scan (dbcache: complete bounded scan; --cursor resumes its capture)",
        "--offline",
        "--limit <1-200>",
        "--cursor <opaque>",
        "--page <n>",
        "--after-index <n>",
        "--max-pages <n>",
        "--max-requests <n>",
        "--max-bytes <n>",
        "--encoding csv",
        "--output <file>"
      ]
    },
    {
      "path": "live bugs",
      "actual": [
        "--home <root>",
        "--format text|json|jsonl",
        "--session <session-id>",
        "--request <idempotency-key>",
        "--count <1-100>",
        "--project <directory>",
        "--wait-seconds <1-600> (default 120)"
      ],
      "expected": [
        "--home <root>",
        "--format text|json|jsonl",
        "--session <session-id>",
        "--request <idempotency-key>",
        "--count <1-100>",
        "--project <directory>",
        "--wait-seconds <1-600> (default 120)",
        "--no-cache"
      ]
    },
    {
      "path": "live connect",
      "actual": [
        "--home <root>",
        "--format text|json|jsonl",
        "--snapshot <pin>",
        "--character <name>",
        "--realm <realm>",
        "--pid <pid>",
        "--installation <client>",
        "--session <session-id>",
        "--project <directory>",
        "--wait-seconds <1-600> (default 120)"
      ],
      "expected": [
        "--home <root>",
        "--format text|json|jsonl",
        "--snapshot <pin>",
        "--character <name>",
        "--realm <realm>",
        "--pid <pid>",
        "--installation <client>",
        "--session <session-id>",
        "--project <directory>",
        "--wait-seconds <1-600> (default 120)",
        "--no-cache"
      ]
    },
    {
      "path": "live disconnect",
      "actual": [
        "--format text|json|jsonl",
        "--project <directory>",
        "--wait-seconds <1-600> (default 120)"
      ],
      "expected": [
        "--format text|json|jsonl",
        "--project <directory>",
        "--wait-seconds <1-600> (default 120)",
        "--no-cache"
      ]
    },
    {
      "path": "live doctor",
      "actual": [
        "--format text|json|jsonl",
        "--installation <client>",
        "--pid <pid>",
        "--session <CON-id>",
        "--project <directory>",
        "--fix",
        "--release <distribution-root>",
        "--output <upgrade-archive>",
        "--request <idempotency-key>",
        "--wait-seconds <1-600> (default 30)"
      ],
      "expected": [
        "--format text|json|jsonl",
        "--installation <client>",
        "--pid <pid>",
        "--session <CON-id>",
        "--project <directory>",
        "--fix",
        "--release <distribution-root>",
        "--output <upgrade-archive>",
        "--request <idempotency-key>",
        "--wait-seconds <1-600> (default 30)",
        "--no-cache"
      ]
    },
    {
      "path": "live execute",
      "actual": [
        "--home <root>",
        "--format text|json|jsonl",
        "--file <lua-file>",
        "--session <session-id>",
        "--probe <name-or-revision>",
        "--request <idempotency-key>",
        "--budget-seconds <1-120>",
        "--project <directory>",
        "--wait-seconds <1-600> (default 120)",
        "--policy <opaque|observation> (default opaque)"
      ],
      "expected": [
        "--home <root>",
        "--format text|json|jsonl",
        "--file <lua-file>",
        "--session <session-id>",
        "--probe <name-or-revision>",
        "--request <idempotency-key>",
        "--budget-seconds <1-120>",
        "--project <directory>",
        "--wait-seconds <1-600> (default 120)",
        "--no-cache",
        "--policy <opaque|observation> (default opaque)"
      ]
    },
    {
      "path": "live instances",
      "actual": [
        "--home <root>",
        "--format text|json|jsonl",
        "--installation <client-or-game-root>"
      ],
      "expected": [
        "--home <root>",
        "--format text|json|jsonl",
        "--installation <client-or-game-root>",
        "--passive"
      ]
    },
    {
      "path": "live reload",
      "actual": [
        "--home <root>",
        "--format text|json|jsonl",
        "--session <session-id>",
        "--request <idempotency-key>",
        "--project <directory>",
        "--wait-seconds <1-600> (default 120)"
      ],
      "expected": [
        "--home <root>",
        "--format text|json|jsonl",
        "--session <session-id>",
        "--request <idempotency-key>",
        "--project <directory>",
        "--wait-seconds <1-600> (default 120)",
        "--no-cache"
      ]
    },
    {
      "path": "live reload fallback",
      "actual": [
        "--home <root>",
        "--format text|json|jsonl",
        "--installation <client>",
        "--pid <pid>",
        "--request <idempotency-key>",
        "--project <directory>",
        "--wait-seconds <1-600> (default 120)"
      ],
      "expected": [
        "--home <root>",
        "--format text|json|jsonl",
        "--installation <client>",
        "--pid <pid>",
        "--request <idempotency-key>",
        "--project <directory>",
        "--wait-seconds <1-600> (default 120)",
        "--no-cache"
      ]
    },
    {
      "path": "live resume",
      "actual": [
        "--home <root>",
        "--format text|json|jsonl",
        "--project <directory>",
        "--wait-seconds <1-600> (default 120)"
      ],
      "expected": [
        "--home <root>",
        "--format text|json|jsonl",
        "--project <directory>",
        "--wait-seconds <1-600> (default 120)",
        "--no-cache"
      ]
    },
    {
      "path": "target add",
      "actual": [
        "--home <root>",
        "--format text|json|jsonl",
        "--product <retail|classic|classic-era|titan|forever-asia-beta|forever-cn-beta|anniversary>",
        "--region <us|eu|cn|kr|tw>",
        "--locale <locale>",
        "--build <full-build>",
        "--installation <client-directory>",
        "--definitions <commit-or-full-ref>",
        "--remote",
        "--replace"
      ],
      "expected": [
        "--home <root>",
        "--format text|json|jsonl",
        "--product <retail|classic|classic-era|titan|forever-asia-beta|forever-cn-beta|anniversary> (legacy: forever)",
        "--region <us|eu|cn|kr|tw>",
        "--locale <locale>",
        "--build <full-build>",
        "--installation <client-directory>",
        "--definitions <commit-or-full-ref>",
        "--remote",
        "--replace"
      ]
    },
    {
      "path": "target resolve",
      "actual": [
        "--home <root>",
        "--format text|json|jsonl",
        "--file <selection.json>",
        "--target <name>",
        "--installation <game-root-or-client>",
        "--product <retail|classic|classic-era|titan|forever-asia-beta|forever-cn-beta|anniversary>",
        "--build <full-build>",
        "--definitions <commit-or-full-ref>",
        "--region <us|eu|cn|kr|tw>",
        "--locale <locale>",
        "--from <parent-pin>",
        "--offline"
      ],
      "expected": [
        "--home <root>",
        "--format text|json|jsonl",
        "--file <selection.json>",
        "--target <name>",
        "--installation <game-root-or-client>",
        "--product <retail|classic|classic-era|titan|forever-asia-beta|forever-cn-beta|anniversary> (legacy: forever)",
        "--build <full-build>",
        "--definitions <commit-or-full-ref>",
        "--region <us|eu|cn|kr|tw>",
        "--locale <locale>",
        "--from <parent-pin>",
        "--offline"
      ]
    }
  ].map(Object.freeze)),
});
export const APPROVED_ENGINE_SUMMARIES = APPROVED_ENGINE_DIFFERENCES.summaryChanges;
export const REPETITIONS = 20;
export const TARGET_RATIO = 0.50;
export const CASES = Object.freeze([
  {id: 'B01-describe', args: ['describe'], performance: false},
  {id: 'B02-retail-index', args: ['source', 'index'], performance: true},
  {id: 'B02-retail-query', args: ['source', 'query', 'GetActionButtonForID', '--mode', 'precise', '--topic', 'lua', '--limit', '10'], performance: true},
]);
export const orderFor = iteration => iteration % 2 === 0 ? ['go', 'rust'] : ['rust', 'go'];
export function elapsed(proof) {
  assert.equal(proof?.schema, 'lycheedev.owned-process.v1');
  for (const key of ['rootReaped', 'treeReaped', 'handlesClosed', 'toolIdentityLocksClosed', 'outputReadersComplete']) assert.equal(proof[key], true, key);
  assert.equal(proof.activeProcesses, 0); assert.equal(proof.forced, false);
  assert(!proof.error && !proof.deadlineHit && !proof.outputLimit, 'failed process cannot yield accepted timing');
  const t = proof.timing;
  assert.equal(t?.schema, 'lycheedev.owned-timing.v1', 'actual CLI clock required');
  assert(Number.isSafeInteger(t.frequency) && t.frequency > 0);
  for (const key of ['startTicks', 'rootExitTicks', 'treeReapedTicks']) assert.match(t[key], /^[1-9]\d*$/);
  const start = BigInt(t.startTicks), root = BigInt(t.rootExitTicks), end = BigInt(t.treeReapedTicks);
  assert(end >= root && root >= start && end > start, 'ordered monotonic timing required');
  const millis = Number(end - start) * 1000 / t.frequency;
  assert(Number.isFinite(millis) && millis >= 1000 / t.frequency * 100, 'timing window under 100 clock ticks; enlarge equivalent work before acceptance');
  return {controllableMillis: millis, rootObservedMillis: Number(root - start) * 1000 / t.frequency, reapAndDrainMillis: Number(end - root) * 1000 / t.frequency, resolutionMillis: 1000 / t.frequency};
}


// Optional attribution only. Old receipts remain valid for elapsed()/acceptance.
// Never subtract these phases or root CPU from the original end-to-end clock.
export function startupBreakdown(proof, {required = false} = {}) {
  const d = proof?.diagnostics;
  if (d === undefined) { assert(!required, 'startup diagnostics missing'); return null; }
  const total = elapsed(proof), t = proof.timing;
  assert.equal(d?.schema, 'lycheedev.owned-startup-diagnostics.v1');
  assert(!d.error, 'startup diagnostic observation failed');
  const names = ['createReturnTicks', 'rootPersistBeginTicks', 'rootPersistEndTicks', 'observerSetupEndTicks', 'resumeBeforeTicks', 'resumeAfterTicks'];
  for (const name of names) assert.match(d[name], /^[1-9]\d*$/, name);
  const ticks = [BigInt(t.startTicks), ...names.map(name => BigInt(d[name])), BigInt(t.rootExitTicks), BigInt(t.treeReapedTicks)];
  for (let i = 1; i < ticks.length; i++) assert(ticks[i] >= ticks[i - 1], 'startup diagnostic clock order');
  assert.equal(d.rootCpuObserved, true, 'root process CPU observation missing');
  for (const name of ['rootKernel100ns', 'rootUser100ns']) assert.match(d[name], /^(0|[1-9]\d*)$/, name);
  const millis = (start, end) => Number(end - start) * 1000 / t.frequency;
  const phaseNames = ['createProcessMillis', 'identityBeforePersistenceMillis', 'rootIdentityDurabilityMillis', 'outputAndObserverSetupMillis', 'beforeResumeGapMillis', 'resumeCallMillis', 'resumedToObservedRootExitMillis', 'rootExitToReapAndDrainMillis'];
  return {
    schema: 'lycheedev.startup-breakdown.v1', ...total,
    preResumeMillis: millis(ticks[0], ticks[5]),
    resumeThroughReapAndDrainMillis: millis(ticks[5], ticks[8]),
    phases: Object.fromEntries(phaseNames.map((name, i) => [name, millis(ticks[i], ticks[i + 1])])),
    rootCpu: {kernelMillis: Number(BigInt(d.rootKernel100ns)) / 10000, userMillis: Number(BigInt(d.rootUser100ns)) / 10000,
      scope: 'GetProcessTimes on the original reaped root handle only; not descendants or owner/observer CPU'},
    qualification: 'Diagnostic attribution only; all original elapsed time remains included. Resume-through-reap contains scheduling, loader/runtime, product work, cleanup and output drain; it is not pure CPU or a performance pass.',
  };
}

function fields(value, names, optional = []) {
  const result = {};
  for (const name of names) { assert(Object.hasOwn(value, name) || optional.includes(name), `missing public field ${name}`); if (Object.hasOwn(value, name)) result[name] = value[name]; }
  return result;
}
function normalizeDescribe(result, engine) {
  assert(Array.isArray(result.commands) && result.commands.length > 0);
  const diffs = APPROVED_ENGINE_DIFFERENCES;
  const changes = [], rows = structuredClone(result.commands);
  assert.equal(new Set(rows.map(row => row.path)).size, rows.length, 'duplicate commands');
  // Declared engine changes are substituted only in the direction recorded in
  // the reviewed table; an undeclared difference in a shared row is not normalized.
  if (engine === 'rust') {
    for (const row of rows) {
      const summary = diffs.summaryChanges.find(entry => entry.path === row.path && entry.actual === row.summary);
      if (summary) { changes.push(summary); row.summary = summary.expected; }
      const flags = diffs.flagChanges.find(entry => entry.path === row.path && isDeepStrictEqual(entry.actual, row.flags));
      if (flags) { changes.push(flags); row.flags = flags.expected; }
    }
  }
  const dropped = new Set(engine === 'go' ? diffs.goOnlyCommands : diffs.rustOnlyCommands);
  const extras = {result: Object.keys(result).filter(key => !diffs.describeResultKeys.base.includes(key)),
    command: [...new Set(rows.flatMap(row => Object.keys(row)))].filter(key => !diffs.commandKeys.includes(key))};
  for (const key of extras.result) assert(diffs.describeResultKeys.addedByRust.includes(key), `undeclared describe result field ${key}`);
  for (const key of extras.command) assert(diffs.resultKeys.includes(key), `undeclared describe command field ${key}`);
  if (engine === 'go') {
    assert.equal(extras.result.length, 0, 'fixed reference reports fields outside the reviewed base set');
    assert.equal(extras.command.length, 0, 'fixed reference reports row fields outside the reviewed base set');
  }
  const shared = rows.filter(row => !dropped.has(row.path));
  // The fixed reference predates the current command directory: declared
  // reference-only commands are dropped and current-only commands are not
  // compared. Every remaining row keeps the base contract fields in order.
  const projected = shared.map(row => Object.fromEntries(diffs.commandKeys.map(key => [key, row[key]])));
  return {value: {development: result.development, formats: result.formats, commands: projected}, changes,
    droppedEngineOnlyCommands: [...dropped]};
}
export function semantic(caseId, envelope, engine, oracle, pin) {
  const result = assertEnvelope({exit: 0, envelope});
  assert.equal(envelope.error, null, 'successful envelope cannot hide an error');
  if (caseId === 'B01-describe') return normalizeDescribe(result, engine);
  if (caseId === 'B02-retail-index') {
    assert.equal(result.repository, oracle.repository); assert.equal(result.product, oracle.product); assert.equal(result.commit, oracle.commit);
    assert.equal(result.documents, oracle.documents); assert.equal(result.skippedDocuments ?? 0, 0); assert.equal(result.complete, true);
    assert.equal(result.schema, 'lycheedev.source-map.v2'); assert.equal(result.storage, 'file-cache'); assert.equal(result.parser, 'source-v1');
    return {value: {...fields(result, ['schema', 'storage', 'parser', 'repository', 'product', 'commit', 'documents', 'declarations', 'relationships', 'assets', 'diagnostics', 'complete']), skippedDocuments: result.skippedDocuments ?? 0, diagnosticSample: result.diagnosticSample ?? []}, changes: []};
  }
  assert.equal(caseId, 'B02-retail-query');
  assertAnchor(result, oracle.anchors[0], oracle, pin);
  assert.equal(result.complete, true); assert.equal(result.truncated, false); assert.equal(result.coverage.complete, true);
  // Project the fixed Go 3.2.4 public result, never Rust's Emmy-only provenance,
  // checker counters or matchCoverage. Keep all rows, ordering, scores, IDs,
  // excerpts, original-byte hashes, diagnostics, cursors and completeness.
  const value = fields(result, ['sourceId', 'product', 'requestedRef', 'matchedTag', 'resolvedCommit', 'snapshotId', 'complete', 'truncated'], ['requestedRef']);
  value.results = result.results.map(row => fields(row, ['symbolId', 'kind', 'name', 'path', 'matchedBy', 'role', 'confidence', 'excerpt', 'contentHash', 'line', 'score', 'scoreParts'], ['symbolId', 'name', 'confidence', 'scoreParts']));
  value.relations = (result.relations ?? []).map(row => fields(row, ['source', 'target', 'kind', 'confidence', 'path', 'line'], ['source']));
  value.suggestions = result.suggestions ?? []; value.nextCursor = result.nextCursor ?? null;
  value.coverage = {...fields(result.coverage, ['analyzedDocuments', 'skippedDocuments', 'diagnostics', 'complete']), diagnosticSample: result.coverage.diagnosticSample ?? []};
  return {value, changes: []};
}
export function compareSamples(caseId, go, rust, oracle, pin) {
  const expected = semantic(caseId, go, 'go', oracle, pin), actual = semantic(caseId, rust, 'rust', oracle, pin);
  assert(isDeepStrictEqual(expected.value, actual.value), 'fixed Go public semantic fields differ; both complete raw outputs are retained');
  return {resultEquivalent: true, approvedEngineSummaryChanges: actual.changes};
}
export function statistics(values) {
  assert(values.length > 0 && values.every(v => Number.isFinite(v) && v > 0));
  const sorted = [...values].sort((a, b) => a - b), middle = Math.floor(sorted.length / 2);
  return {count: sorted.length, median: sorted.length % 2 ? sorted[middle] : (sorted[middle - 1] + sorted[middle]) / 2, p95: sorted[Math.ceil(sorted.length * .95) - 1], min: sorted[0], max: sorted.at(-1)};
}
export function summarize(pairs) {
  return CASES.flatMap(({id, performance}) => ['cold', 'warm'].map(cacheMode => {
    const rows = pairs.filter(row => row.caseId === id && row.cacheMode === cacheMode);
    const iterations = new Set(rows.map(r => r.iteration));
    const identities = new Set(rows.flatMap(row => [row.go?.home, row.rust?.home]));
    const valid = rows.length === REPETITIONS && iterations.size === REPETITIONS && [...iterations].every(n => Number.isInteger(n) && n >= 0 && n < REPETITIONS)
      && identities.size === REPETITIONS * 2 && !identities.has(undefined) && rows.every(row => {
        const cold = cacheMode === 'cold' ? row : pairs.find(p => p.caseId === id && p.iteration === row.iteration && p.cacheMode === 'cold');
        return row.resultEquivalent === true && cold?.resultEquivalent === true && row.go?.valid === true && row.rust?.valid === true
          && orderFor(row.iteration).join() === row.order?.join()
          && (cacheMode !== 'warm' || id === 'B01-describe' || ['go', 'rust'].every(engine => row[engine].warmReuse?.verified === true))
          && ['go', 'rust'].every(engine => row[engine].home === cold[engine]?.home && row[engine].binary === cold[engine]?.binary && row[engine].snapshot === cold[engine]?.snapshot);
      });
    const allMeasured = rows.length > 0 && rows.every(row => ['go', 'rust'].every(engine => Number.isFinite(row[engine]?.controllableMillis) && row[engine].controllableMillis > 0));
    const timings = allMeasured ? Object.fromEntries(['go', 'rust'].map(engine => [engine, statistics(rows.map(row => row[engine].controllableMillis))])) : null;
    const ratio = timings ? timings.rust.median / timings.go.median : null;
    return {caseId: id, cacheMode, requiredPairs: REPETITIONS, attemptedPairs: rows.length, correctAndComplete: valid, timings, medianRatio: ratio, targetRatio: performance ? TARGET_RATIO : null, performanceAcceptanceEvaluated: performance, state: !valid ? 'failed_correctness_or_missing_samples' : !performance ? 'passed_correctness_only' : ratio <= TARGET_RATIO ? 'passed_selected_path' : 'failed_performance'};
  }));
}
export function selectedPathsPassed(groups) {
  return groups.length === CASES.length * 2 && groups.every(row => {
    const entry = CASES.find(entry => entry.id === row.caseId);
    return entry && row.correctAndComplete && row.state === (entry.performance ? 'passed_selected_path' : 'passed_correctness_only');
  });
}
