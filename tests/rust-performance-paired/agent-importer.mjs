import {agentEnvironment} from './agent-environment.mjs';
import {parse} from './agent-json.mjs';
// Explicit preparation only; the actual Skill workflow never compiles implicitly.
import assert from 'node:assert/strict';
import {lstatSync, readFileSync, writeFileSync} from 'node:fs';
import {dirname, join, resolve} from 'node:path';
import {hashFile, runCargo} from '../../tools/rust-build.mjs';
import {contained} from '../../tools/rust-disk.mjs';

export const IMPORTER_ARGS = ['test', '--locked', '--offline', '-j', '1', '-p', 'lycheedev-data', '--test', 'agent_fixture_import', '--no-run', '--message-format=json'];
export const IMPORTER_TARGETS = ['agent_fixture_import'];
export function prepareAgentImporter(root) {
  assert.equal(process.platform, 'win32'); assert.equal(process.arch, 'x64');
  root = resolve(root);agentEnvironment(root,process.env);
  return runCargo(root, IMPORTER_ARGS, {owner: 'paired-agent-importer', oneShot: false, isolateUser: true, onSuccess(invocation) {
    const artifacts = invocation.stdout.split(/\r?\n/).filter(Boolean).map(line => parse(line));
    const executables = IMPORTER_TARGETS.map(name => {
      const rows = artifacts.filter(row => row.reason === 'compiler-artifact' && row.target?.name === name && row.profile?.test === true && row.executable);
      assert.equal(rows.length, 1, `exact exporter compiler artifact ${name}`);
      const row = rows[0], expectedSource = join(root, 'crates/lycheedev-data/tests', name + '.rs');
      assert.deepEqual(row.target.kind, ['test']); assert.equal(resolve(row.target.src_path), expectedSource);
      const path = contained(invocation.target, resolve(row.executable));
      return {name, path, source: expectedSource, bytes: lstatSync(path).size, sha256: hashFile(path, 128 << 20)};
    });
    const evidence = dirname(invocation.buildEvidence), path = join(evidence, 'paired-agent-importer.json');
    const manifest = {schema: 'lycheedev.paired-agent-importer.recovered.v1', sourceTree: invocation.inputHashes.sourceTree, target: invocation.target, buildEvidence: invocation.buildEvidence, executables};
    writeFileSync(path, JSON.stringify(manifest, null, 2) + '\n', {flag: 'wx'});
    // Bind this fresh manifest into this invocation's command before runCargo
    // seals it. Never modify a historical receipt or adopt an old test EXE.
    const commandPath = join(evidence, 'command.json'), command = parse(readFileSync(commandPath));
    command.agentImporter = {path, sha256: hashFile(path)};
    writeFileSync(commandPath, JSON.stringify(command, null, 2) + '\n');
    return {buildEvidence: invocation.buildEvidence, manifest: path, manifestSHA256: hashFile(path), executables};
  }});
}

import {bounded} from './agent-fixture-inputs.mjs';
const parseFile=path=>parse(bounded(path,4<<20));
export function loadAgentImporter(root, receiptPath, sourceTree) {
  const receipt = parseFile(contained(root, resolve(receiptPath))), evidence = dirname(receiptPath);
  assert.equal(receipt.schema, 'lycheedev.disk-owner.v1'); assert.equal(receipt.owner, 'paired-agent-importer');
  assert(['succeeded', 'retired'].includes(receipt.state)); assert.equal(receipt.handlesReaped, true);
  assert.equal(receipt.inputHashes?.sourceTree, sourceTree);
  function sealed(name) {
    const file = contained(evidence, join(evidence, name)), row = receipt.sealedLogs.find(row => row.path === file);
    assert(row, `missing sealed exporter ${name}`); assert.equal(hashFile(file), row.sha256); return file;
  }
  const command = parseFile(sealed('command.json'));
  assert.equal(command.cwd, root); assert.equal(command.channel, join(root, 'target')); assert.equal(command.target, join(root, 'target'));
  assert.deepEqual(receipt.command, ['cargo', ...IMPORTER_ARGS]);
  assert.equal(command.command, 'cargo'); assert.deepEqual(command.args, IMPORTER_ARGS); assert.equal(command.exit, 0);
  assert.equal(command.error, undefined); assert.equal(command.oneShot, false); assert.equal(command.inputHashes.sourceTree, sourceTree);
  const manifestPath = contained(evidence, command.agentImporter.path);
  assert.equal(hashFile(manifestPath), command.agentImporter.sha256);
  const manifest = parseFile(manifestPath);
  assert.equal(manifest.schema, 'lycheedev.paired-agent-importer.recovered.v1'); assert.equal(manifest.buildEvidence, receiptPath);
  assert.equal(manifest.sourceTree, sourceTree); assert.equal(manifest.target, command.target);
  const artifacts = bounded(sealed('stdout.bin'), 8 << 20).toString().split(/\r?\n/).filter(Boolean).map(line => parse(line));
  assert.deepEqual(manifest.executables.map(row => row.name), IMPORTER_TARGETS);
  for (const row of manifest.executables) {
    const compiled = artifacts.filter(item => item.reason === 'compiler-artifact' && item.target?.name === row.name && item.profile?.test && item.executable);
    assert.equal(compiled.length, 1); assert.deepEqual(compiled[0].target.kind, ['test']);
    assert.equal(row.source, join(root, 'crates/lycheedev-data/tests', row.name + '.rs'));
    assert.equal(resolve(compiled[0].target.src_path), row.source); assert.equal(resolve(compiled[0].executable), row.path);
    contained(command.target, row.path); assert.equal(lstatSync(row.path).size, row.bytes); assert.equal(hashFile(row.path, 128 << 20), row.sha256);
  }
  return {receiptPath, receiptSHA256: hashFile(receiptPath), manifestPath, manifestSHA256: hashFile(manifestPath), ...manifest};
}

