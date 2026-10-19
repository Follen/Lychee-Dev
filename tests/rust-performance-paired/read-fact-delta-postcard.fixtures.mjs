// Synthetic boundary fixtures only. No compiler, product or external process.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import {join, dirname, relative} from 'node:path';
import {sha256, readFactDelta} from './read-fact-delta.mjs';
import {sourceDigest, hashFile} from '../../tools/rust-build.mjs';
import {splitIndexEvidence} from '../../tools/rust-source-pack-evidence.mjs';
import {captureGoCustody, openGoCustody} from './source-pack-custody.mjs';
import {POSTCARD_LAYOUT, EXPORT_TEST, EXPORT_ARGS, EXPORT_BUILD_ARGS, TOOLCHAIN_OWNER, postcardToolchainPolicy, postcardCompilerObservation, postcardFactKey, postcardFactPath, preparePostcardExport, runPostcardExport, postcardIndexIdentity} from './read-fact-delta-postcard.mjs';
export const put = (file, raw) => { fs.mkdirSync(dirname(file), {recursive: true}); fs.writeFileSync(file, raw); };
export const putJSON = (file, value) => put(file, JSON.stringify(value));
export const proof = () => ({schema: 'lycheedev.owned-process.v1', exitCode: 0, rootReaped: true, treeReaped: true, handlesClosed: true, toolIdentityLocksClosed: true, outputReadersComplete: true, activeProcesses: 0});
export const compilerText = `rustc 1.99.0 (111111111 2026-10-01)\nbinary: rustc\ncommit-hash: ${'1'.repeat(40)}\ncommit-date: 2026-10-01\nhost: x86_64-pc-windows-msvc\nrelease: 1.99.0\nLLVM version: 23.1.0\n`;
export const compilerPolicy = {schema: 'lycheedev.rust-toolchain.v1', rustc: '1.99.0', cargo: '1.99.0', host: 'x86_64-pc-windows-msvc', target: 'x86_64-pc-windows-msvc', profile: 'release', locked: true};
const goJSON = value => JSON.stringify(value).replace(/</g, '\\u003c').replace(/>/g, '\\u003e').replace(/&/g, '\\u0026').replace(/\u2028/g, '\\u2028').replace(/\u2029/g, '\\u2029');
export function emptyFrame(input) {
  // Real frozen Rust empty golden (private_wire_empty_golden_*): five explicit
  // zero sequence counts. Nonempty arrays below test the exporter boundary and
  // are deliberately synthetic; they do not claim native decoder execution.
  const payload = Buffer.from([0, 0, 0, 0, 0]), frame = Buffer.alloc(76 + payload.length);
  frame.write('LYCFCT13'); frame.writeUInt32LE(payload.length, 8); Buffer.from(input, 'hex').copy(frame, 12); Buffer.from(sha256(payload), 'hex').copy(frame, 44); payload.copy(frame, 76); return frame;
}
function sealedExporter(root) {
  for (const directory of ['crates', 'release', '.cargo']) fs.mkdirSync(join(root, directory), {recursive: true});
  put(join(root, 'Cargo.toml'), '[workspace]\n'); put(join(root, 'Cargo.lock'), '# synthetic build provenance fixture\n');
  putJSON(join(root, 'release/tools/rust.json'), compilerPolicy);
  put(join(root, 'crates/lycheedev-source/src/lib.rs'), '// synthetic test identity, never compiled\n');
  const sourceTreeSHA256 = sourceDigest(root), target = join(root, 'target'), executable = join(target, 'debug/deps/lycheedev_source-abcd.exe');
  const cargoArtifact = {reason: 'compiler-artifact', target: {name: 'lycheedev_source', kind: ['lib'], crate_types: ['lib'], src_path: join(root, 'crates/lycheedev-source/src/lib.rs')}, manifest_path: join(root, 'crates/lycheedev-source/Cargo.toml'), profile: {test: true}, executable};
  const stdout = JSON.stringify(cargoArtifact) + '\n' + JSON.stringify({reason: 'build-finished', success: true}) + '\n';
  const id = '11111111-2222-4333-8444-555555555555', evidence = join(root, '.tmp/rust-disk/evidence', id), stem = join(evidence, 'process-aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee');
  const request = {command: 'cargo', args: EXPORT_BUILD_ARGS, cwd: root, cargoBuild: true, stdout: stem + '.stdout.bin', stderr: stem + '.stderr.bin', receipt: stem + '.receipt.json'};
  putJSON(stem + '.request.json', request); putJSON(stem + '.request.json.wrapper.json', {status: 0}); putJSON(request.receipt, proof()); put(request.stdout, stdout); put(request.stderr, '');
  put(join(evidence, 'stdout.bin'), stdout); putJSON(join(evidence, 'command.json'), {command: 'cargo', args: EXPORT_BUILD_ARGS, cwd: root, exit: 0, oneShot: true, inputHashes: {sourceTree: sourceTreeSHA256}, environmentSHA256: 'a'.repeat(64)});
  const command = ['cargo', ...EXPORT_BUILD_ARGS];
  const receipt = {schema: 'lycheedev.disk-owner.v1', id, owner: 'source-postcard-exporter-build', category: 'scratch', directory: join(root, '.tmp/rust-scratch', id), state: 'retired', activePid: null, handlesReaped: true, sealedAt: '2026-10-19T00:00:00Z', command, commandSHA256: sha256(JSON.stringify(command)), inputHashes: {sourceTree: sourceTreeSHA256}};
  const receiptPath = join(evidence, 'receipt.json');
  const seal = () => { receipt.sealedLogs = fs.readdirSync(evidence).filter(n => n !== 'receipt.json').map(n => { const path = join(evidence, n); return {path, original: path, sha256: hashFile(path)}; }); receipt.logHashes = Object.fromEntries(receipt.sealedLogs.map(r => [relative(root, r.path), r.sha256])); putJSON(receiptPath, receipt); }; seal();
  const sourceBinary = join(root, 'artifacts/exporter.exe'); put(sourceBinary, 'synthetic executable identity; never launched');
  const artifact = {schema: 'lycheedev.rust-build-artifact.v1', profile: 'source-lib-test', sourceRoot: root, sourceBinary, binarySHA256: hashFile(sourceBinary), bytes: fs.statSync(sourceBinary).size, sameInvocation: true, command, inputHashes: {sourceTree: sourceTreeSHA256}, environmentSHA256: 'a'.repeat(64), channel: target, buildEvidence: receiptPath, cargoArtifact};
  const compiler = join(root, 'tools-bin/rustc.exe'); put(compiler, 'synthetic compiler identity; never launched');
  const compilerId = '22222222-2222-4333-8444-555555555555', compilerEvidence = join(root, '.tmp/rust-disk/evidence', compilerId), policy = postcardToolchainPolicy(root);
  const observations = ['before-build', 'after-build'].map((label, i) => {
    const runId = `${i ? 'bbbbbbbb' : 'cccccccc'}-bbbb-4ccc-8ddd-eeeeeeeeeeee`, stem = join(compilerEvidence, `process-${runId}`), proofValue = {...proof(), runId, binary: compiler};
    const request = {command: 'rustc', args: ['-Vv'], cwd: root, cargoBuild: false, stdout: stem + '.stdout.bin', stderr: stem + '.stderr.bin', receipt: stem + '.receipt.json'};
    putJSON(stem + '.request.json', request); putJSON(stem + '.request.json.wrapper.json', {status: 0}); putJSON(request.receipt, proofValue); put(request.stdout, compilerText); put(request.stderr, '');
    return {label, runId, identity: postcardCompilerObservation({status: 0, handlesReaped: true, proof: proofValue, stdout: Buffer.from(compilerText), stderr: Buffer.alloc(0)}, policy)};
  });
  const record = {schema: 'lycheedev.source-postcard-toolchain.v1', state: 'verified', handlesReaped: true, sourceRoot: root, sourceTreeSHA256, policy, environmentSHA256: 'b'.repeat(64), observations,
    build: {receipt: artifact.buildEvidence, command: artifact.command, inputHashes: artifact.inputHashes, environmentSHA256: artifact.environmentSHA256, target: artifact.channel}};
  const recordPath = join(compilerEvidence, 'toolchain.json'); putJSON(recordPath, record);
  const compilerCommand = ['rustc', '-Vv', 'before-and-after-source-exporter-build'];
  const compilerReceipt = {...receipt, id: compilerId, owner: TOOLCHAIN_OWNER, directory: join(root, '.tmp/rust-scratch', compilerId), command: compilerCommand, commandSHA256: sha256(JSON.stringify(compilerCommand)), inputHashes: {sourceTree: sourceTreeSHA256, toolchainPolicy: policy.sha256, environment: record.environmentSHA256}};
  const sealCompiler = () => { compilerReceipt.sealedLogs = fs.readdirSync(compilerEvidence).filter(n => n !== 'receipt.json').map(n => { const path = join(compilerEvidence, n); return {path, original: path, sha256: hashFile(path)}; }); compilerReceipt.logHashes = Object.fromEntries(compilerReceipt.sealedLogs.map(r => [relative(root, r.path), r.sha256])); putJSON(join(compilerEvidence, 'receipt.json'), compilerReceipt); };
  sealCompiler(); artifact.toolchain = {record: recordPath, recordSHA256: hashFile(recordPath), receipt: join(compilerEvidence, 'receipt.json'), receiptSHA256: hashFile(join(compilerEvidence, 'receipt.json'))};
  const artifactPath = join(root, 'artifacts/artifact.json'); putJSON(artifactPath, artifact);
  const expected = {artifact: artifactPath, artifactSHA256: hashFile(artifactPath), binarySHA256: artifact.binarySHA256, sourceTreeSHA256};
  return {candidate: {sourceRoot: root, sourceTreeSHA256}, artifact, expected, seal, evidence, sourceBinary, compiler, compilerEvidence, sealCompiler};
}
export async function fixture(t, callback) {
  const root = fs.mkdtempSync(join(os.tmpdir(), 'lycheedev-postcard-')); let succeeded = false;
  try {
    const build = sealedExporter(root), home = join(root, 'rust'), goHome = join(root, 'go'), evidence = join(root, 'evidence'), scratch = join(root, 'scratch'); fs.mkdirSync(evidence); fs.mkdirSync(scratch);
    const documents = ['A.toc', 'B.lua', 'C.xml'].map((path, i) => ({path, sha256: sha256('source-' + i), bytes: 10 + i}));
    const facts = documents.map((_, i) => ({declarations: [{name: 'Name<&>雪\u2028', category: 'function', line: 1, endLine: 128}], relationships: [{from: 'a', to: 'b', category: 'call', confidence: 'literal', line: 2}], loads: [{path: 'A.lua', line: 1, kind: 'script'}, {path: 'A.lua', line: 1, kind: 'script'}, {path: 'B.lua', line: 2, kind: 'script'}], headers: [{key: 'Title', value: String(i), line: 1}], diagnostics: [{message: 'note', line: 3}]}));
    const group = sha256('synthetic authenticated pack'), pack = [Buffer.from('LYCFPK01')]; let offset = 8;
    documents.forEach((doc, i) => { const raw = emptyFrame(doc.sha256); if (i < 2) { const header = Buffer.alloc(68); header.writeUInt32LE(raw.length); Buffer.from(postcardFactKey(doc), 'hex').copy(header, 4); Buffer.from(sha256(raw), 'hex').copy(header, 36); doc.factsLocator = {group, offset, bytes: raw.length, sha256: sha256(raw)}; pack.push(header, raw); offset += header.length + raw.length; } else put(postcardFactPath(home, doc), raw); });
    put(postcardFactPath(home, documents[0]), Buffer.concat(pack));
    const summary = {repository: 'fixture', product: 'retail', commit: '1'.repeat(40), parser: 'source-v1', documents: 3, declarations: 3, relationships: 3, diagnostics: 3};
    const indexRaw = Buffer.from(JSON.stringify({documents, summary})), indexSHA = sha256(indexRaw), indexPath = join(home, 'cache/objects', indexSHA.slice(0, 2), indexSHA.slice(2)); put(indexPath, indexRaw);
    const index = {path: indexPath, bytes: indexRaw.length, sha256: indexSHA, manifest: {schema: 'lycheedev.source-index.rust.v6', identity: postcardIndexIdentity(summary), blob: {bytes: indexRaw.length, sha256: indexSHA}}};
    const records = join(goHome, 'source/v1/facts/snapshots/fixture/records.jsonl'), goRaw = Buffer.from(documents.map(({factsLocator, ...doc}) => JSON.stringify({kind: 'document', ...doc})).join('\n') + '\n'); put(records, goRaw);
    const goManifest = {recordsHash: sha256(goRaw), recordBytes: goRaw.length, summary}; putJSON(join(dirname(records), 'manifest.json'), goManifest);
    const saveGoFacts = () => documents.forEach((doc, i) => { const mode = doc.path.slice(-4).startsWith('.') ? doc.path.slice(-4) : '.lua', key = sha256(`source-v1\0${mode}\0${doc.sha256}`); put(join(goHome, `source/v1/facts/files/source-v1/${key}.json`), goJSON({schema: 'lycheedev.source-file-facts.v2', parser: 'source-v1', mode, inputSHA256: doc.sha256, factsSHA256: sha256(goJSON(facts[i])), facts: facts[i]})); });
    saveGoFacts();
    let calls = 0;
    const execute = (binary, args, options) => {
      calls++; assert.equal(binary, build.sourceBinary); assert.deepEqual(args, EXPORT_ARGS); assert.equal(options.cargoBuild, false);
      const manifest = JSON.parse(fs.readFileSync(options.env.LYCHEEDEV_SOURCE_FACTS_EXPORT_INPUT));
      assert.equal(manifest.records.length, 3); const rows = manifest.records.map((r, ordinal) => ({schema: 'lycheedev.source-facts-export-record.v1', ordinal, path: r.path, inputSHA256: r.inputSHA256, recordSHA256: r.recordSHA256, recordBytes: r.recordBytes, facts: structuredClone(facts[ordinal])}));
      f.mutateRows?.(rows); const output = Buffer.from(rows.map(row => JSON.stringify(row)).join('\n') + '\n'); put(options.env.LYCHEEDEV_SOURCE_FACTS_EXPORT_OUTPUT, output);
      const summary = {schema: 'lycheedev.source-facts-export-summary.v1', codec: 'postcard-1.1.3', layout: 'LYCFCT13', manifestSHA256: options.env.LYCHEEDEV_SOURCE_FACTS_EXPORT_SHA256, manifestBytes: Number(options.env.LYCHEEDEV_SOURCE_FACTS_EXPORT_BYTES), records: manifest.records.length, recordBytes: manifest.records.reduce((n, r) => n + r.recordBytes, 0), outputBytes: output.length, outputSHA256: sha256(output)};
      f.mutateSummary?.(summary); f.afterExecute?.();
      return {status: 0, handlesReaped: true, proof: {...proof(), binary, ...f.proofOverrides}, stdout: Buffer.from(`\nrunning 1 test\ntest ${EXPORT_TEST} ... ${JSON.stringify(summary)}\nok\n\ntest result: ok. 1 passed; 0 failed; 0 ignored; 1 filtered out; finished in 0.00s\n`), stderr: Buffer.alloc(0)};
    };
    const f = {root, home, goHome, records, evidence, scratch, documents, facts, index, build, execute, saveGoFacts, calls: () => calls,
      prepare: () => preparePostcardExport(root, home, index, join(scratch, 'input')),
      run(preparation) { return runPostcardExport(root, build.expected, build.candidate, preparation, join(scratch, 'output.jsonl'), evidence, {execute}); },
      read(token) { return readFactDelta(goHome, home, records, POSTCARD_LAYOUT, index.path, index.sha256, undefined, {captureSources: true, retainIndex: true, postcardExport: token}); },
      async custody() { const directory = join(root, 'go-evidence'); fs.mkdirSync(directory); const goIndex = {path: records, bytes: goRaw.length, sha256: sha256(goRaw), manifest: goManifest}; splitIndexEvidence(root, {path: records, bytes: goRaw.length, sha256: sha256(goRaw)}, join(directory, 'go-index-parts')); const reference = await captureGoCustody(goHome, goIndex, directory, root); const opened = openGoCustody(reference); opened.reference = reference; return opened; },
    };
    await callback(f); succeeded = true;
  } finally { if (succeeded) fs.rmSync(root, {recursive: true, force: true}); else t.diagnostic(`Failed Postcard fixture retained: ${root}`); }
}

