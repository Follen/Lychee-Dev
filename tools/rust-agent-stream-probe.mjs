// Private implementation for the two rust-stream-* entrypoints only.
// Reconstruction identity is deliberately distinct from retired stream tools.
// Product children are created exclusively inside the existing owned Job. The
// final owner receipt, never child.kill / a PID / an exit event, certifies reap.
import assert from 'node:assert/strict';
import {createHash, randomUUID} from 'node:crypto';
import {closeSync, lstatSync, mkdirSync, openSync, readFileSync, readSync, readdirSync, writeFileSync} from 'node:fs';
import {join, resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import {AgentClient, AGENT_PROTOCOL, assertSafeContext, decodeAgentFrame, parseAgentJson} from './rust-agent-client.mjs';
import {ownedSpawnSync} from './rust-owned-process.mjs';

const IMPLEMENTATION = 'agent-stream-consumer-reconstruction-20261020';
const OUTER_MS = 600000;
const CONTROLLER_MS = 540000;
const REQUEST_MS = 1000;
const EXIT_MS = 15000;
const MAX_RAW = 32 << 20;
const OWNER_ENV = 'LYCHEEDEV_AGENT_STREAM_PROBE_OWNER';
const SELF = fileURLToPath(import.meta.url);
const MODES = ['closed-consumer', 'backpressure-deadline'];
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const errorText = error => error?.message ?? String(error);
const save = (path, value) => writeFileSync(path, JSON.stringify(value, null, 2) + '\n');
function bounded(path, maximum = MAX_RAW) {
  const stat = lstatSync(path);
  assert(stat.isFile() && !stat.isSymbolicLink() && stat.size <= maximum, 'agent-stream.file_bound');
  return readFileSync(path);
}
const json = path => parseAgentJson(bounded(path));
function fileHash(path) {
  const stat = lstatSync(path), maximum = 512 << 20;
  assert(stat.isFile() && !stat.isSymbolicLink() && stat.size <= maximum, 'agent-stream.identity_file_bound');
  const file = openSync(path, 'r'), digest = createHash('sha256'), chunk = Buffer.alloc(65536);
  let total = 0;
  try {
    for (;;) {
      const count = readSync(file, chunk, 0, chunk.length, null);
      if (!count) break;
      total += count; assert(total <= maximum, 'agent-stream.identity_file_bound');
      digest.update(chunk.subarray(0, count));
    }
    assert.equal(total, stat.size, 'agent-stream.identity_file_changed');
    return digest.digest('hex');
  } finally { closeSync(file); }
}
const query = () => ({kind: 'plan', plan: {op: 'limit',
  input: {op: 'scan', table: {name: 'Map', mode: 'static'}}, offset: 0, count: 1184}});
function agentArgs(config) {
  return ['data', 'agent', 'serve', '--snapshot', config.pin, '--installation', config.installation,
    '--offline', '--format', 'jsonl', '--home', config.home, '--timeout-seconds', '510'];
}
function sourceInputs() {
  return Object.fromEntries(['rust-stream-pipe.mjs', 'rust-stream-deadline.mjs',
    'rust-agent-stream-probe.mjs', 'rust-agent-client.mjs', 'rust-owned-process.mjs',
    'rust-owned-process.ps1'].map(name => [name, fileHash(join(import.meta.dirname, name))]));
}
function archive(config, name, bytes) {
  const path = join(config.out, name);
  writeFileSync(path, bytes, {flag: 'wx'});
  return {name, bytes: bytes.length, sha256: hash(bytes)};
}
function archived(config, item) {
  assert(item && /^[a-z0-9-]+\.(json|jsonl|bin)$/.test(item.name), 'agent-stream.archive_name');
  const bytes = bounded(join(config.out, item.name));
  assert.equal(bytes.length, item.bytes); assert.equal(hash(bytes), item.sha256);
  return bytes;
}
function assertPage(page, rows, offset, limit) {
  assertSafeContext({offset: page.offset, totalRows: page.totalRows, complete: page.complete, columns: page.columns});
  assert.equal(page.offset, offset); assert.equal(page.totalRows, rows); assert.equal(page.complete, true);
  assert(Array.isArray(page.columns) && page.columns.length > 0 && page.columns.every(name => typeof name === 'string'));
  assert(Array.isArray(page.rows)); assert.equal(page.rows.length, Math.min(limit, rows - offset));
  for (const row of page.rows) assert(Array.isArray(row) && row.length === page.columns.length);
}
function assertQuery(result) {
  assertSafeContext(result.provenance); assertSafeContext(result.result);
  assert.equal(result.complete, true); assert.deepEqual(result.provenance.request, query());
  assert.equal(result.provenance.complete, true);
  const sources = result.provenance.sources;
  assert(Array.isArray(sources) && sources.length === 1);
  assert.equal(sources[0].sourceMode, 'static'); assert.equal(sources[0].complete, true);
  assert.equal(sources[0].sourceIdentity.table.name, 'Map');
  assert.match(result.result.handle, /^RES-/);
  assert(Number.isSafeInteger(result.result.rows) && result.result.rows > 0 && result.result.rows <= 1184,
    'agent-stream.nonempty_real_map_required');
}

// Replay every archived baseline frame. Neither a controller's "passed" string
// nor fixture-sized/empty output can substitute for the actual typed pipeline.
function preflight(config, observation) {
  const records = observation.frames;
  assert(Array.isArray(records) && records.length >= 3);
  const frames = records.map((row, index) => {
    assert.equal(row.request.id, index + 1); assert.equal(row.request.protocol, AGENT_PROTOCOL);
    assertSafeContext(row.request);
    const bytes = archived(config, row.raw), frame = decodeAgentFrame(bytes, row.request.id);
    assert.equal(frame.ok, true, JSON.stringify(frame.error));
    return frame.result;
  });
  assert.deepEqual(records[0].request, {protocol: AGENT_PROTOCOL, id: 1, timeout_ms: 120000,
    task: 'query', query: query()});
  assertQuery(frames[0]);
  const handle = frames[0].result.handle, rows = frames[0].result.rows;
  let offset = 0, largest;
  for (let index = 1; index < records.length; index += 2) {
    const pageRequest = records[index].request, exportRecord = records[index + 1];
    assert(exportRecord, 'agent-stream.export_after_every_page');
    const limit = Math.min(1000, rows - offset);
    assert.deepEqual(pageRequest, {protocol: AGENT_PROTOCOL, id: index + 1, timeout_ms: 30000,
      task: 'page', handle, offset, limit});
    assert.deepEqual(exportRecord.request, {protocol: AGENT_PROTOCOL, id: index + 2, timeout_ms: 30000,
      task: 'export', handle, offset, limit, format: 'csv'});
    assertPage(frames[index], rows, offset, limit);
    assert.equal(frames[index + 1].format, 'csv');
    assert.equal(typeof frames[index + 1].text, 'string'); assert(frames[index + 1].text.length > 0);
    assert.deepEqual(frames[index + 1].page, frames[index]);
    if (!largest || exportRecord.raw.bytes > largest.raw.bytes) largest = exportRecord;
    offset += limit;
  }
  assert.equal(offset, rows, 'agent-stream.all_requested_rows_paged_and_exported');
  return {handle, rows, largest};
}
function expectedExport(config, baseline, requestId) {
  const raw = archived(config, baseline.raw);
  // Change only the response's envelope identity, preserving every exact
  // numeric token, string, CSV byte and source row from the successful export.
  const prefix = Buffer.from(`{"protocol":"${AGENT_PROTOCOL}","id":${baseline.request.id},`);
  assert(raw.subarray(0, prefix.length).equals(prefix), 'agent-stream.response_envelope_order');
  return Buffer.concat([Buffer.from(`{"protocol":"${AGENT_PROTOCOL}","id":${requestId},`),
    raw.subarray(prefix.length), Buffer.from('\n')]);
}
function diagnostics(config, observation) {
  const raw = archived(config, observation.stderr);
  const text = new TextDecoder('utf-8', {fatal: true}).decode(raw);
  const lines = text.split('\n').filter(line => line.trim());
  return lines.flatMap(line => {
    // Worker-side errors may precede the structured supervisor diagnostic.
    try {
      const value = parseAgentJson(line); assertSafeContext(value);
      return value.protocol === 'lycheedev.data-agent-supervisor.v1' ? [value] : [];
    } catch { return []; }
  });
}
function assertOutcome(config, observation) {
  assert.equal(observation.implementation, IMPLEMENTATION);
  assert.equal(observation.runId, config.runId); assert.equal(observation.mode, config.mode);
  assert.equal(observation.independentWatchdog, false);
  assert.equal(observation.binarySHA256After, config.binarySHA256);
  assert.deepEqual(observation.args, agentArgs(config));
  const baseline = preflight(config, observation), stress = observation.stress;
  const expectedRequest = {...baseline.largest.request, id: observation.frames.length + 1,
    timeout_ms: config.mode === 'closed-consumer' ? 10000 : REQUEST_MS};
  assert.deepEqual(stress.request, expectedRequest);
  assert(stress.stdinKeptOpenUntilExit, 'agent-stream.no_eof_cancellation_substitute');
  assert.equal(observation.process.signal, null); assert.equal(observation.process.error, undefined);
  const terminal = diagnostics(config, observation);
  assert.equal(terminal.length, 1, 'agent-stream.terminal_supervisor_diagnostic_required');
  assert.equal(terminal[0].ok, false);
  const error = terminal[0].error;
  assert.equal(error.details.sessionInvalidated, true);
  assert.equal(Object.hasOwn(error.details, 'cleanup'), false, 'agent-stream.cleanup_not_pending');
  if (config.mode === 'closed-consumer') {
    assert.equal(stress.consumerClosedBeforeExport, true);
    assert.equal(observation.process.code, 5); assert.equal(error.exit_code, 5);
    assert.equal(error.code, 'data.session.worker_exited');
    assert(error.details.outstandingRequestIds.includes(stress.request.id));
    assert.equal(archived(config, observation.stressStdout).length, 0);
  } else {
    assert.equal(observation.process.code, 7); assert.equal(error.exit_code, 7);
    assert.equal(error.code, 'command.deadline'); assert.equal(error.details.requestId, stress.request.id);
    assert.equal(error.details.replayed, false);
    assert.equal(stress.pausedBeforeRequest, true);
    assert.equal(stress.consumerBytesBeforeExit, 0);
    assert(stress.exitedWhilePaused && stress.exitAtMs < stress.resumedAtMs);
    assert(stress.exitAtMs - stress.sentAtMs >= REQUEST_MS, 'agent-stream.deadline_did_not_elapse');
    const partial = archived(config, observation.stressStdout), expected = expectedExport(config, baseline.largest, stress.request.id);
    // A real prefix proves export serialization reached the unread pipe. A
    // complete frame, no bytes, unrelated error or short paused read is no pass.
    assert(partial.length > 0 && partial.length < expected.length, 'agent-stream.backpressure_not_triggered');
    assert.equal(partial.includes(10), false, 'agent-stream.complete_frame_not_deadline_proof');
    assert(expected.subarray(0, partial.length).equals(partial), 'agent-stream.partial_export_mismatch');
  }
  return {rows: baseline.rows, pageAndExportPairs: (observation.frames.length - 1) / 2,
    baselineExportBytes: baseline.largest.raw.bytes, terminalCode: error.code};
}

export function runOwnedAgentStream(mode, argv) {
  assert(MODES.includes(mode));
  assert.equal(argv.length, 5, 'usage: <exact-binary> <task-home> <pin> <read-only-installation> <new-report-directory>');
  const [binaryArg, homeArg, pin, installationArg, outArg] = argv;
  assert.match(pin, /^PIN-[a-f0-9]{64}$/);
  assert.equal(process.platform, 'win32', 'agent-stream.owned_windows_job_required');
  const binary = resolve(binaryArg), home = resolve(homeArg), installation = resolve(installationArg), out = resolve(outArg);
  const binarySHA256 = fileHash(binary);
  assert(lstatSync(home).isDirectory()); assert(lstatSync(installation).isDirectory());
  mkdirSync(out); const evidence = join(out, 'owned'); mkdirSync(evidence);
  const config = {schema: 'lycheedev.agent-stream-configuration.reconstructed.v1', implementation: IMPLEMENTATION,
    runId: randomUUID(), mode, binary, binarySHA256, home, pin, installation, out, evidence,
    cwd: process.cwd(), coordinatorPid: process.pid, node: process.execPath, nodeSHA256: fileHash(process.execPath),
    outerTimeoutMs: OUTER_MS, controllerTimeoutMs: CONTROLLER_MS, inputs: sourceInputs()};
  const configPath = join(out, 'configuration.json'); save(configPath, config);
  const configurationSHA256 = fileHash(configPath);
  let run, observation, failure, detail;
  try {
    run = ownedSpawnSync(process.execPath, [SELF, '--owned-controller', configPath], {
      cwd: config.cwd, env: {...process.env, [OWNER_ENV]: config.runId},
      evidence, timeout: OUTER_MS, orphanMs: 10000});
    assert.equal(fileHash(configPath), configurationSHA256, 'agent-stream.configuration_changed');
    assert.equal(fileHash(binary), binarySHA256, 'agent-stream.binary_changed');
    assert.equal(fileHash(process.execPath), config.nodeSHA256, 'agent-stream.node_changed');
    assert.deepEqual(sourceInputs(), config.inputs, 'agent-stream.runner_changed');
    observation = json(join(out, 'observation.json'));
    assert.equal(run.handlesReaped, true, 'agent-stream.tree_reap_unverified');
    assert.equal(run.proof?.forced, false, 'agent-stream.forced_cleanup_is_not_a_pass');
    assert.equal(run.proof?.deadlineHit, false, 'agent-stream.outer_deadline_is_not_product_deadline');
    assert.equal(run.error, undefined, errorText(run.error));
    assert.equal(run.status, 0, 'agent-stream.controller_failed');
    assert.equal(observation.owner.runId, run.proof.runId);
    assert.equal(observation.controllerPid, run.proof.pid);
    detail = assertOutcome(config, observation);
  } catch (error) { failure = errorText(error); }
  const report = {schema: 'lycheedev.agent-stream-owned.reconstructed.v1', implementation: IMPLEMENTATION,
    state: failure ? (run?.handlesReaped ? 'failed_or_not_triggered' : 'cleanup_unverified') : 'passed',
    runId: config.runId, mode, configuration: configPath, configurationSHA256,
    binary: {path: binary, sha256: binarySHA256}, home, pin, installation,
    outerTimeoutMs: OUTER_MS, observation: join(out, 'observation.json'), detail, failure,
    controllerFailure: observation?.failure,
    process: {exit: run?.status, error: run?.error && errorText(run.error), handlesReaped: run?.handlesReaped === true,
      proof: run?.proof, logs: run?.logs ?? []}};
  save(join(out, 'report.json'), report);
  console.log(JSON.stringify({state: report.state, mode, failure, report: join(out, 'report.json')}));
  if (failure) process.exitCode = 1;
}

function assertOwnedController(config, configPath) {
  assert.equal(config.schema, 'lycheedev.agent-stream-configuration.reconstructed.v1');
  assert.equal(config.outerTimeoutMs, OUTER_MS); assert.equal(config.controllerTimeoutMs, CONTROLLER_MS);
  assert.equal(config.implementation, IMPLEMENTATION); assert(MODES.includes(config.mode));
  assert.equal(process.platform, 'win32'); assert.equal(process.env[OWNER_ENV], config.runId);
  assert.equal(config.node, process.execPath); assert.equal(config.nodeSHA256, fileHash(process.execPath));
  assert.equal(config.binarySHA256, fileHash(config.binary)); assert.deepEqual(config.inputs, sourceInputs());
  const requests = readdirSync(config.evidence).filter(name => /^process-[0-9a-f-]+\.request\.json$/.test(name));
  assert.equal(requests.length, 1, 'agent-stream.single_owned_controller_required');
  const request = json(join(config.evidence, requests[0])), root = json(request.rootIdentity);
  assert.equal(request.schema, 'lycheedev.owned-request.v2'); assert.equal(request.coordinatorPid, config.coordinatorPid);
  assert.equal(request.command, process.execPath); assert.deepEqual(request.args, [SELF, '--owned-controller', configPath]);
  assert.equal(request.timeoutMs, OUTER_MS); assert.equal(request.cwd, resolve(config.cwd));
  assert.equal(root.schema, 'lycheedev.owned-root.v1'); assert.equal(root.runId, request.runId);
  assert.equal(root.pid, process.pid); assert.equal(root.jobAssignedAtCreation, true);
  return {runId: root.runId, rootIdentity: request.rootIdentity, request: join(config.evidence, requests[0]),
    jobAssignedAtCreation: true, terminalReapProof: 'pending_outer_owner_receipt'};
}
async function within(promise, milliseconds, code) {
  let timer;
  try {
    return await Promise.race([promise, new Promise((_, reject) => {
      timer = setTimeout(() => reject(Error(code)), milliseconds);
    })]);
  } finally { clearTimeout(timer); }
}
async function controller(configPath) {
  const config = json(configPath), owner = assertOwnedController(config, configPath);
  const started = performance.now(), deadline = started + CONTROLLER_MS;
  const observation = {schema: 'lycheedev.agent-stream-observation.reconstructed.v1', implementation: IMPLEMENTATION,
    runId: config.runId, mode: config.mode, controllerPid: process.pid, owner,
    args: agentArgs(config), independentWatchdog: false, frames: [], state: 'running'};
  const persist = () => save(join(config.out, 'observation.json'), observation);
  let client, timer, stressBytes = 0, detachedConsumer = false;
  const stressChunks = [];
  const exitUnconfirmed = error => {
    observation.state = 'failed'; observation.failure = errorText(error); observation.independentWatchdog = true;
    observation.cleanup = 'outer_owned_job_must_terminate_and_reap_descendants';
    if (client && !observation.stderr) observation.stderr = archive(config, 'agent-stderr.jsonl', Buffer.concat(client.stderr));
    if (detachedConsumer && !observation.stressStdout) observation.stressStdout = archive(config, 'stress-stdout.bin', Buffer.concat(stressChunks));
    persist();
    // Deliberately leave tree custody with the owner. This is a failed probe,
    // never a child.kill cleanup claim or an invitation to launch a new child.
    process.exit(1);
  };
  timer = setTimeout(() => exitUnconfirmed(Error('agent-stream.controller_deadline')), CONTROLLER_MS);
  persist();
  try {
    client = new AgentClient(config.binary, observation.args, {cwd: config.cwd, env: {...process.env}, deadline,
      onFrame: row => {
        observation.frames.push({request: row.request,
          raw: archive(config, `frame-${row.request.id}.json`, row.raw)});
        persist();
      }});
    observation.agentPid = client.child.pid;
    const rootExit = new Promise(resolveExit => client.child.once('exit', (code, signal) => {
      const atMs = performance.now() - started;
      if (observation.stress?.pausedBeforeRequest) {
        // Observe at the exit event itself: Node may automatically resume
        // unread stdio after this event to finish closing ChildProcess handles.
        observation.stress.exitedWhilePaused = client.child.stdout.isPaused();
        observation.stress.consumerBytesBeforeExit = stressBytes;
        observation.stress.exitAtMs = atMs;
        observation.stress.resumedAtMs ??= performance.now() - started;
        client.child.stdout.resume();
      }
      resolveExit({code, signal, atMs});
    }));
    const result = await client.success({task: 'query', query: query()}, {timeoutMs: 120000});
    assertQuery(result);
    for (let offset = 0; offset < result.result.rows; offset += 1000) {
      const limit = Math.min(1000, result.result.rows - offset), handle = result.result.handle;
      await client.success({task: 'page', handle, offset, limit}, {timeoutMs: 30000});
      await client.success({task: 'export', handle, offset, limit, format: 'csv'}, {timeoutMs: 30000});
    }
    const baseline = preflight(config, observation);
    assert.equal(client.pending, null); assert.equal(client.bytes.length, 0);
    const request = {...baseline.largest.request, id: client.nextId++,
      timeout_ms: config.mode === 'closed-consumer' ? 10000 : REQUEST_MS};
    assertSafeContext(request);
    // AgentClient owns the baseline only. The final response intentionally is
    // not a request to that normal complete-frame consumer.
    client.child.stdout.pause(); client.child.stdout.removeAllListeners('data');
    detachedConsumer = true;
    client.child.stdout.on('data', chunk => {
      stressBytes += chunk.length;
      if (stressBytes > MAX_RAW) exitUnconfirmed(Error('agent-stream.stdout_bound'));
      stressChunks.push(Buffer.from(chunk));
    });
    observation.stress = {request, stdinKeptOpenUntilExit: false};
    if (config.mode === 'closed-consumer') {
      await within(new Promise(ok => {
        client.child.stdout.once('close', ok); client.child.stdout.destroy();
      }), EXIT_MS, 'agent-stream.consumer_close_deadline');
      observation.stress.consumerClosedBeforeExport = true;
    } else {
      assert(client.child.stdout.isPaused());
      observation.stress.pausedBeforeRequest = true;
    }
    observation.stress.sentAtMs = performance.now() - started; persist();
    await within(new Promise((ok, fail) => client.child.stdin.write(JSON.stringify(request) + '\n',
      error => error ? fail(error) : ok())), EXIT_MS, 'agent-stream.input_write_deadline');
    let notTriggered = false, exited;
    try {
      exited = await within(rootExit, EXIT_MS, 'agent-stream.exit_not_observed');
    } catch (error) {
      if (config.mode !== 'backpressure-deadline') throw error;
      // A small real export may fit the pipe and complete normally. Do not
      // replace its data or lower a guardrail to manufacture a deadline. Drain
      // and close this same session, then retain the explicit non-pass.
      notTriggered = true;
      observation.stress.resumedAtMs = performance.now() - started;
      observation.stress.notTriggered = 'no_supervisor_exit_within_observation_window';
      client.child.stdout.resume();
      const close = {protocol: AGENT_PROTOCOL, id: client.nextId++, timeout_ms: 10000, task: 'close'};
      observation.stress.cleanupRequest = close; persist();
      await within(new Promise((ok, fail) => client.child.stdin.end(JSON.stringify(close) + '\n',
        error => error ? fail(error) : ok())), EXIT_MS, 'agent-stream.cleanup_write_deadline');
      exited = await within(rootExit, EXIT_MS, 'agent-stream.cleanup_exit_deadline');
    }
    observation.stress.exitAtMs = exited.atMs;
    observation.stress.stdinKeptOpenUntilExit = !client.child.stdin.writableEnded && !client.failed;
    observation.process = await within(client.exit, EXIT_MS, 'agent-stream.output_drain_deadline');
    observation.stressStdout = archive(config, 'stress-stdout.bin', Buffer.concat(stressChunks));
    observation.stderr = archive(config, 'agent-stderr.jsonl', Buffer.concat(client.stderr));
    observation.binarySHA256After = fileHash(config.binary);
    if (notTriggered) throw Error('agent-stream.backpressure_not_triggered');
    observation.detail = assertOutcome(config, observation);
    observation.state = 'observed_expected_failure_pending_outer_reap'; persist();
    clearTimeout(timer);
  } catch (error) {
    observation.state = 'failed'; observation.failure = errorText(error);
    if (client && !client.closed && !detachedConsumer && !client.pending && !client.failed) {
      // A rejected baseline query is not a reason to abandon a healthy worker.
      try { observation.baselineCleanup = await within(client.close(), EXIT_MS, 'agent-stream.baseline_close_deadline'); }
      catch (cleanup) { observation.baselineCleanup = {error: errorText(cleanup)}; }
    }
    if (client?.closed) {
      if (!observation.stderr) observation.stderr = archive(config, 'agent-stderr.jsonl', Buffer.concat(client.stderr));
      observation.process ??= await client.exit;
      observation.binarySHA256After = fileHash(config.binary);
      persist(); clearTimeout(timer); process.exitCode = 1;
    } else exitUnconfirmed(error);
  }
}

if (process.argv[1] && resolve(process.argv[1]) === SELF) {
  assert.equal(process.argv[2], '--owned-controller', 'agent-stream.private_controller');
  assert.equal(process.argv.length, 4, 'agent-stream.private_controller_arguments');
  await controller(resolve(process.argv[3]));
}
