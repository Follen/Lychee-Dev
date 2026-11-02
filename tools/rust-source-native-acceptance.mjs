// No builds, game, publish, arbitrary artifact or per-child ownership exemptions.
import assert from 'node:assert/strict';
import {existsSync, lstatSync, readdirSync, writeFileSync} from 'node:fs';
import {join, resolve} from 'node:path';
import {verifyArtifact, sourceDigest, hashFile, ownedUserEnvironment} from './rust-build.mjs';
import {beginScratch, contained} from './rust-disk.mjs';
import {collectOwnedMemory} from './rust-owned-memory.mjs';
import {ownedSpawnSync} from './rust-owned-process.mjs';
import {GO_CONTRACT, inventory, inventoryHash, jsonFile, verifyCandidate} from '../tests/rust-source-native-acceptance/runner-inputs.mjs';
import {groupBudget} from '../tests/rust-source-native-acceptance/assertions.mjs';

const [artifactArg, candidateArg, group = 'full', ...extra] = process.argv.slice(2);
assert(artifactArg && candidateArg && !extra.length, 'usage: node tools/rust-source-native-acceptance.mjs <artifact.json> <candidate-report.json> [full|quota|idle|lifetime]');
const timeout = groupBudget(group);
assert(process.platform === 'win32' && process.arch === 'x64', 'native-acceptance.windows_amd64_required');
const runnerRoot = resolve(import.meta.dirname, '..');
const root = resolve('.'), artifactPath = contained(root, resolve(artifactArg)), candidatePath = contained(root, resolve(candidateArg));
const artifact = verifyArtifact(root, artifactPath), artifactSHA256 = hashFile(artifactPath), tree = sourceDigest(root);
assert.equal(artifact.inputHashes?.sourceTree, tree, 'current source tree and sealed artifact must match');
const candidate = verifyCandidate(root, candidatePath, artifact);
assert.equal(candidate.report.artifactRecordSHA256, artifactSHA256);
const paths = ['tools/rust-source-native-acceptance.mjs', 'tools/rust-owned-process.mjs', 'tools/rust-owned-process.ps1', 'tools/rust-owned-memory.mjs', ...readdirSync(join(runnerRoot, 'tests/rust-source-native-acceptance')).filter(name => /\.(mjs|ps1)$/.test(name)).map(name => `tests/rust-source-native-acceptance/${name}`), 'tests/rust-source-full-corpus/run.mjs', 'tests/rust-source-full-corpus/assertions.mjs', 'tests/rust-source-full-corpus/retail-oracle.json', 'tools/rust-build.mjs', 'tools/rust-disk.mjs', 'tools/rust-emmy-identity.mjs', 'tools/rust-build-auxiliary.json', 'tools/zip.mjs', 'release/rust-disk-policy.json'];
const testInputs = Object.fromEntries(paths.map(path => [path, hashFile(join(runnerRoot, path))]));
const job = beginScratch(root, `source-native-${group}`, {command: process.argv, inputs: {binarySHA256: artifact.binarySHA256, sourceTreeSHA256: tree, artifactSHA256, candidateReportSHA256: candidate.reportSHA256, packageSHA256: candidate.packageSHA256, testInputs}, admissionOptions: {requiredBytes: 1 << 30, scratchReserveBytes: 1 << 30}});
let state = 'failed', handlesReaped = true; const logs = [];
const save = (name, value) => writeFileSync(join(job.evidence, name), JSON.stringify(value, null, 2) + '\n');
try {
  save('candidate-identity.json', {runnerRoot, goContract: GO_CONTRACT, artifact, artifactSHA256, candidate: candidate.report, candidateReportSHA256: candidate.reportSHA256, packageSHA256: candidate.packageSHA256, files: candidate.files, testInputs});
  handlesReaped = false;
  const run = ownedSpawnSync(process.execPath, [join(runnerRoot, 'tests/rust-source-native-acceptance/run.mjs'), artifact.sourceBinary, candidate.packageRoot, job.directory, job.evidence, group], {cwd: root, env: ownedUserEnvironment(job.directory), evidence: job.evidence, timeout, memoryObservation: group === 'full'});
  handlesReaped = run.handlesReaped === true; logs.push(...run.logs);
  const observationPath = join(job.evidence, 'report.json');
  const observation = existsSync(observationPath) ? jsonFile(observationPath) : null;
  const after = verifyArtifact(root, artifactPath);
  const stable = after.binarySHA256 === artifact.binarySHA256 && hashFile(artifactPath) === artifactSHA256 && sourceDigest(root) === tree && hashFile(candidatePath) === candidate.reportSHA256
    && inventoryHash(inventory(candidate.packageRoot)) === candidate.packageSHA256 && Object.entries(testInputs).every(([path, hash]) => hashFile(join(runnerRoot, path)) === hash);
  const passed = stable && handlesReaped && !run.error && run.status === 0 && observation?.state === `passed_native_${group}`;
  const prerequisitePath = join(job.evidence, 'full-corpus', 'report.json');
  let prerequisite;
  // A failed/missing prerequisite remains incomplete memory evidence too.
  try { if (existsSync(prerequisitePath)) prerequisite = jsonFile(prerequisitePath); } catch {}
  const memory = collectOwnedMemory(run, {evidence: job.evidence, commands: [...(prerequisite?.commands ?? []), ...(observation?.commands ?? [])], windows: observation?.memoryWindows, resources: observation?.resources, runState: observation?.state, sourceBinary: artifact.sourceBinary, controllerImage: process.execPath,
    expectedPhases: group === 'full' ? ['cold-index', 'warm-index', 'warm-query', 'full-native-semantic', 'retained-session-cold', 'retained-session-warm', 'retained-session-steady'] : []});
  save('owned-report.json', {schema: 'lycheedev.source-native-acceptance-owned.v1', group, passed, memory, runnerRoot, goContract: GO_CONTRACT, sourceTreeSHA256: tree, binarySHA256: artifact.binarySHA256, artifactSHA256, candidateReportSHA256: candidate.reportSHA256, packageSHA256: candidate.packageSHA256, inputsStable: stable, testInputs,
    process: {exit: run.status, error: run.error?.message, handlesReaped, proof: run.proof}, observation: observationPath,
    scope: group === 'full' ? 'Fresh complete fixed Retail corpus, selected real ordinary native and cross-CLI retained session correctness. Partial native coverage stays partial. No Go/performance/full-product acceptance claim.' : 'Real isolated native IPC lifecycle/quota fixture; this does not establish full Retail semantic acceptance.'});
  state = passed ? 'succeeded' : 'failed'; if (!passed) process.exitCode = 1;
  console.log(JSON.stringify({passed, group, report: join(job.evidence, 'owned-report.json')}, null, 2));
} catch (error) { save('controller-failure.json', {error: error.message, handlesReaped}); throw error; }
finally {
  // Include nested prerequisite/IPC evidence; never reclaim an unproven tree.
  let files = 0, bytes = 0;
  function collect(path, depth = 0) { assert(depth <= 8, 'evidence depth budget'); for (const entry of readdirSync(path, {withFileTypes: true})) {
    const child = join(path, entry.name), stat = lstatSync(child);
    assert(!stat.isSymbolicLink(), 'evidence links forbidden');
    if (stat.isDirectory()) collect(child, depth + 1);
    else { assert(stat.isFile() && ++files <= 10000 && stat.size <= 16 << 20, 'evidence file budget'); bytes += stat.size; assert(bytes <= 512 << 20, 'evidence total budget'); if (child !== job.receipt && !logs.includes(child)) logs.push(child); }
  } }
  collect(job.evidence);
  job.seal({state: handlesReaped ? state : 'interrupted', handlesReaped, logs});
  if (handlesReaped) job.cleanup();
}
