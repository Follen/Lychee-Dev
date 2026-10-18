// Explicit preparation only; the actual Skill workflow never compiles implicitly.
import assert from 'node:assert/strict';
import {lstatSync, readFileSync, writeFileSync} from 'node:fs';
import {dirname, join, resolve} from 'node:path';
import {hashFile, runCargo} from '../../tools/rust-build.mjs';
import {contained} from '../../tools/rust-disk.mjs';

export const EXPORTER_ARGS = ['test', '--locked', '--offline', '-j', '1', '-p', 'lycheedev-data', '--test', 'catalog_navigation_parity', '--test', 'skill_data_fixture', '--no-run', '--message-format=json'];
export const EXPORTER_TARGETS = ['catalog_navigation_parity', 'skill_data_fixture'];
export function prepareDataExporters(root) {
  assert.equal(process.platform, 'win32'); assert.equal(process.arch, 'x64');
  root = resolve(root);
  return runCargo(root, EXPORTER_ARGS, {owner: 'skill-data-exporters', oneShot: true, onSuccess(invocation) {
    const artifacts = invocation.stdout.split(/\r?\n/).filter(Boolean).map(line => JSON.parse(line));
    const executables = EXPORTER_TARGETS.map(name => {
      const rows = artifacts.filter(row => row.reason === 'compiler-artifact' && row.target?.name === name && row.profile?.test === true && row.executable);
      assert.equal(rows.length, 1, `exact exporter compiler artifact ${name}`);
      const row = rows[0], expectedSource = join(root, 'crates/lycheedev-data/tests', name + '.rs');
      assert.deepEqual(row.target.kind, ['test']); assert.equal(resolve(row.target.src_path), expectedSource);
      const path = contained(invocation.target, resolve(row.executable));
      return {name, path, source: expectedSource, bytes: lstatSync(path).size, sha256: hashFile(path, 128 << 20)};
    });
    const evidence = dirname(invocation.buildEvidence), path = join(evidence, 'skill-data-exporters.json');
    const manifest = {schema: 'lycheedev.skill-data-exporters.v2', sourceTree: invocation.inputHashes.sourceTree, target: invocation.target, buildEvidence: invocation.buildEvidence, executables};
    writeFileSync(path, JSON.stringify(manifest, null, 2) + '\n', {flag: 'wx'});
    // Bind this fresh manifest into this invocation's command before runCargo
    // seals it. Never modify a historical receipt or adopt an old test EXE.
    const commandPath = join(evidence, 'command.json'), command = JSON.parse(readFileSync(commandPath));
    command.skillDataExporters = {path, sha256: hashFile(path)};
    writeFileSync(commandPath, JSON.stringify(command, null, 2) + '\n');
    return {buildEvidence: invocation.buildEvidence, manifest: path, manifestSHA256: hashFile(path), executables};
  }});
}
