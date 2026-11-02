// Explicit fixture preparation only, outside all benchmark timing. The existing
// Cargo runner owns/reaps each invocation and its ordinary cache lease.
import assert from 'node:assert/strict';
import {existsSync, readFileSync, writeFileSync} from 'node:fs';
import {join, resolve} from 'node:path';
import {performance} from 'node:perf_hooks';
import {beginScratch} from '../../tools/rust-disk.mjs';
import {hashFile, runCargo, sourceDigest} from '../../tools/rust-build.mjs';
import {ownedSpawnSync} from '../../tools/rust-owned-process.mjs';
import {agentEnvironment,preparationEnvironmentPolicy,preparationTimeout} from './agent-environment.mjs';
import {resolvedTool,verifyTool} from './agent-tool-identity.mjs';
import {CONTENT_BYTES, dataCases} from './data-contract.mjs';
import {fixturePreparers, fixturePreparationRecipe, fixtureArgs, digest, loadAgentDataFixture, parse} from './agent-fixture-inputs.mjs';

export {fixturePreparers, fixtureArgs};
export function selectedFixturePreparers(args) {
  assert(args.length === 0 || args.length === 1 && ['--content', '--images', '--catalog', '--csv-numeric'].includes(args[0]), 'optional --content, --images, --catalog or --csv-numeric selects one fixture family');
  return args.length ? [fixturePreparationRecipe(args[0].slice(2))] : fixturePreparers.filter(entry => entry.kind !== 'csv-numeric');
}

function main() {
  assert(process.platform === 'win32', 'usage (Windows, fixture-producer root): node tests/rust-performance-paired/agent-prepare-fixtures.mjs [--content | --images | --catalog | --csv-numeric]');
  const selected = selectedFixturePreparers(process.argv.slice(2));
  const root = resolve('.'), started = performance.now();
  agentEnvironment(root,process.env);const cargoIdentity=resolvedTool('cargo');
  const paths = ['tests/rust-performance-paired/agent-prepare-fixtures.mjs', 'tests/rust-performance-paired/agent-fixture-inputs.mjs', 'tests/rust-performance-paired/agent-json.mjs', 'tests/rust-performance-paired/data-contract.mjs',
    'tests/rust-performance-paired/inputs.mjs', 'tests/rust-performance-paired/contract.mjs', 'tests/rust-source-full-corpus/assertions.mjs',
    'tests/rust-performance-paired/agent-custody.mjs','tests/rust-performance-paired/agent-environment.mjs','tests/rust-performance-paired/agent-tool-identity.mjs','tools/rust-owned-memory.mjs','tools/rust-build.mjs', 'tools/rust-disk.mjs', 'tools/rust-emmy-identity.mjs', 'tools/rust-owned-process.mjs', 'tools/rust-owned-process.ps1', 'tools/rust-build-auxiliary.json', 'tools/zip.mjs', 'release/rust-disk-policy.json'];
  const inputs = {environmentPolicy:preparationEnvironmentPolicy,environmentOverrides:[],cargoIdentity,sourceTree: sourceDigest(root), files: Object.fromEntries(paths.map(path => [path, hashFile(join(root, path))])), contentBytes: CONTENT_BYTES};
  if (selected.some(entry => entry.kind === 'csv-numeric')) {
    inputs.numericLayout = {sourceSHA256: hashFile(join(root, 'crates/lycheedev-core/src/public_json.rs')), cargoLockSHA256: hashFile(join(root, 'Cargo.lock'))};
  }
  const job = beginScratch(root, 'paired-data-fixtures', {command: process.argv, inputs, retention: 'evidence', admissionOptions: {requiredBytes: 1 << 30, scratchReserveBytes: 256 << 20}});
  const reportPath = join(job.evidence, 'fixture-preparation.json');
  const report = {schema: 'lycheedev.paired-agent-inputs.recovered.v1', state: 'running', scope: 'synthetic fixture construction only; no product comparison or performance result',
    owner: job.receipt, directory: job.directory, inputs, calls: [], fixtures: {}, fixtureIdentity: {}, buildCacheOwnership: 'each original runCargo receipt/lease is authoritative; this parent never releases or cleans a Cargo channel'};
  const save = () => writeFileSync(reportPath, JSON.stringify(report, null, 2) + '\n');
  let handlesReaped = true;
  save();
  try {
    for (const entry of selected) {
      const destination = join(job.directory, entry.kind); assert(!existsSync(destination));
      const environment = {[entry.variable]: destination, LYCHEEDEV_CONTENT_BUDGET_BYTES: String(CONTENT_BYTES)};
      const row = {kind: entry.kind, args: fixtureArgs(entry), destination, environment, launched: false}; report.calls.push(row); save();
      const began = performance.now();
      try {
        const remainingMs=Math.floor(600000-(performance.now()-started));assert(remainingMs>=1000,'fixture preparation group deadline');
        const invocation = runCargo(root, row.args, {owner: `paired-data-fixture-${entry.kind}`, oneShot: false, isolateUser: true, environment,
          execute(binary, args, options) {
            agentEnvironment(root,process.env);verifyTool(cargoIdentity);options={...options,timeout:preparationTimeout(started,performance.now(),options.timeout)};row.actualTimeoutMs=options.timeout;
            row.launched = true; handlesReaped = false; row.buildEvidence = join(options.evidence, 'receipt.json'); save();
            const run = ownedSpawnSync(binary, args, options); handlesReaped = run.handlesReaped === true;
            // Return post-run validation failures to runCargo with the original
            // reap proof, so it can close this failed invocation's own lease.
            try { verifyTool(cargoIdentity);assert.equal(run.proof?.binary,cargoIdentity.path); }
            catch (error) { run.error ??= error; }
            row.process = {exit: run.status, signal: run.signal, error: run.error?.message, handlesReaped, proof: run.proof, logs: run.logs,
              stdoutSHA256: digest(run.stdout || Buffer.alloc(0)), stderrSHA256: digest(run.stderr || Buffer.alloc(0))}; save();
            return run;
          }});
        assert.equal(invocation.inputHashes.sourceTree, inputs.sourceTree);
        row.invocation = invocation; delete row.invocation.stdout; delete row.invocation.stderr;
        assert(row.launched && row.process.exit === 0 && !row.process.error && handlesReaped);
        const fixture = loadAgentDataFixture(destination, entry.kind); if (entry.kind === 'images') dataCases(fixture.images);
        report.fixtures[entry.kind] = destination; report.fixtureIdentity[entry.kind] = fixture.receipt;
      } catch (error) { row.failure = error.message.slice(0, 4096); throw error; }
      finally {
        row.wallMillis = performance.now() - began;
        if (row.buildEvidence && existsSync(row.buildEvidence)) {
          row.cargoReceipt = parse(readFileSync(row.buildEvidence)); row.cargoReceiptSHA256 = hashFile(row.buildEvidence);
          // Do not infer descendant retirement from this parent's own exit.
          if (row.cargoReceipt.handlesReaped !== true) handlesReaped = false;
        }
        save();
      }
    }
    agentEnvironment(root,process.env);verifyTool(cargoIdentity);assert.equal(sourceDigest(root), inputs.sourceTree);
    for (const [path, sha] of Object.entries(inputs.files)) assert.equal(hashFile(join(root, path)), sha, 'fixture preparation tools changed');
    assert(performance.now()-started<600000,'fixture preparation total deadline');
    assert(report.calls.every(row => row.cargoReceipt?.handlesReaped === true && ['succeeded', 'retired'].includes(row.cargoReceipt.state)));
    report.state = 'prepared_synthetic_inputs';
  } catch (error) { report.state = 'failed'; report.blocker = error.message.slice(0, 4096); process.exitCode = 1; }
  finally {
    report.handlesReaped = handlesReaped; report.wallMillis = performance.now() - started; save();
    job.seal({state: !handlesReaped ? 'interrupted' : report.state === 'prepared_synthetic_inputs' ? 'succeeded' : 'failed', handlesReaped, logs: [reportPath]});
    // All successful and partial inputs remain in this evidence-retained owner.
    // No scratch/cache cleanup, lease recovery or process scanning occurs here.
  }
  console.log(JSON.stringify({state: report.state, report: reportPath, owner: job.receipt, fixtures: report.fixtures}, null, 2));
}
if (process.argv[1] && resolve(process.argv[1]) === resolve(import.meta.filename)) main();
