// Explicit fixture preparation only, outside all benchmark timing. The existing
// Cargo runner owns/reaps each invocation and its ordinary cache lease.
import assert from 'node:assert/strict';
import {existsSync, readFileSync, writeFileSync} from 'node:fs';
import {join, resolve} from 'node:path';
import {performance} from 'node:perf_hooks';
import {beginScratch} from '../../tools/rust-disk.mjs';
import {hashFile, runCargo, sourceDigest} from '../../tools/rust-build.mjs';
import {ownedSpawnSync} from '../../tools/rust-owned-process.mjs';
import {CONTENT_BYTES, dataCases} from './data-contract.mjs';
import {CSV_CARGO_LOCK_SHA256, NUMERIC_LAYOUT_SOURCE_SHA256, digest, loadDataFixture, parse} from './data-inputs.mjs';

export const fixturePreparers = Object.freeze([
  {kind: 'catalog', target: 'catalog_navigation_parity', test: 'prepare_catalog_public_fixture', variable: 'LYCHEEDEV_CATALOG_FIXTURE'},
  {kind: 'content', target: 'content_budget_parity', test: 'prepare_compressed_content_public_fixture', variable: 'LYCHEEDEV_CONTENT_BUDGET_FIXTURE'},
  {kind: 'images', target: 'image_export_business', test: 'prepare_large_image_export_public_fixture', variable: 'LYCHEEDEV_IMAGE_EXPORT_FIXTURE'},
]);
export const numericCSVPreparer = Object.freeze({kind: 'csv-numeric', target: 'catalog_navigation_parity', test: 'prepare_numeric_csv_public_fixture', variable: 'LYCHEEDEV_NUMERIC_CSV_FIXTURE'});
export function selectedFixturePreparers(args) {
  assert(args.length === 0 || args.length === 1 && args[0] === '--csv-numeric', 'only optional --csv-numeric is supported');
  return args.length ? [numericCSVPreparer] : fixturePreparers;
}
export const fixtureArgs = entry => ['test', '--locked', '--offline', '-j', '1', '-p', 'lycheedev-data', '--test', entry.target, entry.test, '--', '--ignored', '--exact', '--nocapture', '--test-threads=1'];

function main() {
  assert(process.platform === 'win32', 'usage (Windows, fixture-producer root): node tests/rust-performance-paired/prepare-data.mjs [--csv-numeric]');
  const selected = selectedFixturePreparers(process.argv.slice(2));
  const root = resolve('.'), started = performance.now();
  const paths = ['tests/rust-performance-paired/prepare-data.mjs', 'tests/rust-performance-paired/data-inputs.mjs', 'tests/rust-performance-paired/data-contract.mjs',
    'tests/rust-performance-paired/inputs.mjs', 'tests/rust-performance-paired/contract.mjs', 'tests/rust-source-full-corpus/assertions.mjs', 'tests/rust-parity/describe-contract.mjs', 'tests/rust-parity/oracle.mjs',
    'tools/rust-build.mjs', 'tools/rust-disk.mjs', 'tools/rust-emmy-identity.mjs', 'tools/rust-owned-process.mjs', 'tools/rust-owned-process.ps1', 'tools/rust-build-auxiliary.json', 'tools/zip.mjs', 'release/rust-disk-policy.json'];
  const inputs = {sourceTree: sourceDigest(root), files: Object.fromEntries(paths.map(path => [path, hashFile(join(root, path))])), contentBytes: CONTENT_BYTES};
  if (selected[0] === numericCSVPreparer) {
    inputs.numericLayout = {sourceSHA256: hashFile(join(root, 'crates/lycheedev-core/src/public_json.rs')), cargoLockSHA256: hashFile(join(root, 'Cargo.lock'))};
    assert.deepEqual(inputs.numericLayout, {sourceSHA256: NUMERIC_LAYOUT_SOURCE_SHA256, cargoLockSHA256: CSV_CARGO_LOCK_SHA256}, 'reviewed unchanged numeric ABI source');
  }
  const job = beginScratch(root, 'paired-data-fixtures', {command: process.argv, inputs, retention: 'evidence', admissionOptions: {requiredBytes: 1 << 30, scratchReserveBytes: 256 << 20}});
  const reportPath = join(job.evidence, 'fixture-preparation.json');
  const report = {schema: 'lycheedev.paired-data-preparation.v1', state: 'running', scope: 'synthetic fixture construction only; no product comparison or performance result',
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
        const invocation = runCargo(root, row.args, {owner: `paired-data-fixture-${entry.kind}`, oneShot: true, environment,
          execute(binary, args, options) {
            row.launched = true; handlesReaped = false; row.buildEvidence = join(options.evidence, 'receipt.json'); save();
            const run = ownedSpawnSync(binary, args, options); handlesReaped = run.handlesReaped === true;
            row.process = {exit: run.status, signal: run.signal, error: run.error?.message, handlesReaped, proof: run.proof, logs: run.logs,
              stdoutSHA256: digest(run.stdout || Buffer.alloc(0)), stderrSHA256: digest(run.stderr || Buffer.alloc(0))}; save();
            return run;
          }});
        assert.equal(invocation.inputHashes.sourceTree, inputs.sourceTree);
        row.invocation = invocation; delete row.invocation.stdout; delete row.invocation.stderr;
        assert(row.launched && row.process.exit === 0 && !row.process.error && handlesReaped);
        const fixture = loadDataFixture(destination, entry.kind); if (entry.kind === 'images') dataCases(fixture.images);
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
    assert.equal(sourceDigest(root), inputs.sourceTree);
    for (const [path, sha] of Object.entries(inputs.files)) assert.equal(hashFile(join(root, path)), sha, 'fixture preparation tools changed');
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
