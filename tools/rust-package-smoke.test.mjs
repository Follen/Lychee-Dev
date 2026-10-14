import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdtempSync, mkdirSync, writeFileSync, rmSync} from 'node:fs';
import {join} from 'node:path';
import {tmpdir} from 'node:os';
import {parsePackageOptions, assertArtifactSource} from './rust-package-smoke.mjs';
import {sourceDigest} from './rust-build.mjs';

test('artifact and assembly-only flags are never mistaken for home, pin, or LuaLS input', () => {
  const parsed = parsePackageOptions(['candidate', 'luac', '--artifact', 'sealed/artifact.json', '--assembly-only']);
  assert.deepEqual(parsed, {outArg: 'candidate', luacArg: 'luac', homeArg: undefined, pinArg: undefined, lualsArg: undefined, artifactPath: 'sealed/artifact.json', profile: 'debug', assemblyOnly: true});
  const reordered = parsePackageOptions(['--profile', 'release', 'candidate', '--artifact', 'sealed/artifact.json', 'luac', '--assembly-only']);
  assert.equal(reordered.profile, 'release'); assert.equal(reordered.luacArg, 'luac'); assert.equal(reordered.homeArg, undefined);
});
test('old positional and explicit named validation inputs remain independently usable', () => {
  const positional = parsePackageOptions(['candidate', 'luac', 'home', 'PIN-exact', 'luals.zip', '--profile', 'release']);
  const named = parsePackageOptions(['candidate', 'luac', '--home', 'home', '--pin', 'PIN-exact', '--luals-archive', 'luals.zip', '--profile', 'release']);
  assert.deepEqual(positional, named);
  assert.equal(named.assemblyOnly, false); assert.equal(named.profile, 'release');
});
test('ambiguous, missing, surplus and ignored package inputs fail before work', () => {
  for (const args of [
    ['out', 'luac', '--artifact'], ['out', 'luac', '--profile', '--artifact', 'record'],
    ['out', 'luac', '--profile', 'symbols'], ['out', 'luac', '--unknown'],
    ['out', 'luac', '--assembly-only', '--assembly-only'],
    ['out', 'luac', '--artifact', 'one', '--artifact', 'two'],
    ['out', 'luac', 'home'], ['out', 'luac', '--pin', 'PIN'],
    ['out', 'luac', 'home', 'PIN', '--home', 'other'],
    ['out', 'luac', 'home', 'PIN', '--assembly-only'],
    ['out', 'luac', 'home', 'PIN', 'luals.zip', 'surplus'],
  ]) assert.throws(() => parsePackageOptions(args), /candidate\.|usage:/);
});
test('artifact admission requires exact source digest and compatible build profile', () => {
  const digest = 'a'.repeat(64), artifact = {profile: 'dev', inputHashes: {sourceTree: digest}};
  assertArtifactSource(artifact, digest, 'debug');
  assert.throws(() => assertArtifactSource(artifact, 'b'.repeat(64), 'debug'), /artifact_source_mismatch/);
  assert.throws(() => assertArtifactSource({profile: 'dev'}, digest, 'debug'), /artifact_source_mismatch/);
  assert.throws(() => assertArtifactSource(artifact, digest, 'release'), /artifact_profile_mismatch/);
  assertArtifactSource({...artifact, profile: 'symbols'}, digest, 'release');
  assert.throws(() => assertArtifactSource(artifact, 'latest', 'debug'), /artifact_source_mismatch/);
});
test('a different luac pin invalidates an otherwise unchanged sealed CLI', t => {
  const root = mkdtempSync(join(tmpdir(), 'lycheedev-candidate-source-')); t.after(() => rmSync(root, {recursive: true}));
  for (const path of ['crates', 'release/tools', '.cargo']) mkdirSync(join(root, path), {recursive: true});
  writeFileSync(join(root, 'Cargo.toml'), '[workspace]\n'); writeFileSync(join(root, 'Cargo.lock'), '# fixed fixture\n');
  const pin = join(root, 'release/tools/luac.json'); writeFileSync(pin, JSON.stringify({sha256: 'old'}));
  const artifact = {profile: 'dev', inputHashes: {sourceTree: sourceDigest(root)}};
  assertArtifactSource(artifact, sourceDigest(root), 'debug');
  writeFileSync(pin, JSON.stringify({sha256: 'new'}));
  assert.throws(() => assertArtifactSource(artifact, sourceDigest(root), 'debug'), /artifact_source_mismatch/);
});
