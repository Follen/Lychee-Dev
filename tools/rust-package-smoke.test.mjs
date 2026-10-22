import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdtempSync, mkdirSync, readFileSync, writeFileSync, rmSync, existsSync} from 'node:fs';
import {join} from 'node:path';
import {createHash} from 'node:crypto';
import {tmpdir} from 'node:os';
import {parsePackageOptions, prepareDependencyClosure, assertArtifactSource, assertSourceSyntaxValidation, assertInstalledNativeVersion, packageSmokeEnvironment} from './rust-package-smoke.mjs';
import {sourceDigest} from './rust-build.mjs';
import {assertNpmSmokeVersion} from './ci-release.mjs';

test('release candidate consumes the v1 npm report CLI version envelope without accepting a flat version', () => {
  const report = {schema: 'lycheedev.rust-npm-smoke.v1', version: {
    schema: 'lycheedev.result.v1', ok: true, result: {version: '3.5.0'},
  }};
  assertNpmSmokeVersion(report, '3.5.0');
  assert.throws(() => assertNpmSmokeVersion(report, '3.5.1'), /ci-release.npm_version_mismatch/);
  assert.throws(() => assertNpmSmokeVersion({...report, version: '3.5.0'}, '3.5.0'), /ci-release.npm_version_mismatch/);
  assert.throws(() => assertNpmSmokeVersion({...report, version: undefined}, '3.5.0'), /ci-release.npm_version_mismatch/);
});

test('npm smoke isolates user, Agent and npm state without inherited toolkit overrides', () => {
  const scratch = join(tmpdir(), 'isolated-npm-smoke'), inherited = '/real-user-state';
  const env = packageSmokeEnvironment(scratch, {HOME: inherited, UserProfile: inherited, CODEX_HOME: inherited,
    APPDATA: inherited, LOCALAPPDATA: inherited, TEMP: inherited, TMP: inherited,
    LYCHEEDEV_BIN: inherited, LYCHEEDEV_HOME: inherited, npm_config_prefix: inherited,
    NPM_CONFIG_USERCONFIG: inherited, PATH: 'tools', SystemRoot: 'system'});
  for (const key of ['HOME', 'USERPROFILE', 'CODEX_HOME', 'APPDATA', 'LOCALAPPDATA', 'TEMP', 'TMP', 'LYCHEEDEV_HOME', 'npm_config_cache', 'npm_config_userconfig', 'npm_config_globalconfig']) assert(env[key].startsWith(scratch));
  assert(!Object.values(env).includes(inherited));
  assert.equal(env.LYCHEEDEV_BIN, undefined); assert.equal(env.npm_config_prefix, undefined);
  assert.equal(env.PATH, 'tools'); assert.equal(env.SystemRoot, 'system');
});

test('artifact and assembly-only flags are never mistaken for home or pin', () => {
  const parsed = parsePackageOptions(['candidate', 'luac', '--artifact', 'sealed/artifact.json', '--assembly-only']);
  assert.deepEqual(parsed, {outArg: 'candidate', luacArg: 'luac', homeArg: undefined, pinArg: undefined, artifactPath: 'sealed/artifact.json', dependencyClosurePath: undefined, profile: 'debug', assemblyOnly: true});
  const reordered = parsePackageOptions(['--profile', 'release', 'candidate', '--artifact', 'sealed/artifact.json', 'luac', '--assembly-only']);
  assert.equal(reordered.profile, 'release'); assert.equal(reordered.luacArg, 'luac'); assert.equal(reordered.homeArg, undefined);
});
test('positional and named validation inputs identify the same current task', () => {
  const positional = parsePackageOptions(['candidate', 'luac', 'home', 'PIN-exact', '--profile', 'release']);
  const named = parsePackageOptions(['candidate', 'luac', '--home', 'home', '--pin', 'PIN-exact', '--profile', 'release']);
  assert.deepEqual(positional, named);
  assert.equal(named.assemblyOnly, false); assert.equal(named.profile, 'release');
});
test('ambiguous, missing, surplus and ignored package inputs fail before work', () => {
  for (const args of [
    ['out', 'luac', '--dependency-closure'],
    ['out', 'luac', '--dependency-closure', 'one', '--dependency-closure', 'two'],
    ['out', 'luac', '--artifact'], ['out', 'luac', '--profile', '--artifact', 'record'],
    ['out', 'luac', '--profile', 'symbols'], ['out', 'luac', '--unknown'],
    ['out', 'luac', '--luals-archive', 'luals.zip'],
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
test('single-addon smoke requires staticValid and never substitutes matrix valid', () => {
  const compilerSHA256 = 'a'.repeat(64), syntaxCompilation = {passed: true, compilerSHA256};
  assertSourceSyntaxValidation({ok: true, result: {staticValid: true, syntaxCompilation}}, compilerSHA256);
  for (const response of [
    {ok: true, result: {valid: true, syntaxCompilation}},
    {ok: true, result: {staticValid: false, valid: true, syntaxCompilation}},
    {ok: false, result: {staticValid: true, syntaxCompilation}},
    {ok: true, result: {staticValid: true, syntaxCompilation: {...syntaxCompilation, passed: false}}},
    {ok: true, result: {staticValid: true, syntaxCompilation: {...syntaxCompilation, compilerSHA256: 'b'.repeat(64)}}},
  ]) assert.throws(() => assertSourceSyntaxValidation(response, compilerSHA256), /candidate.source_syntax_validation_failed/);
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

test('installed native version must match the authenticated package version', () => {
  const expectedVersion = '3.5.0';
  const valid = {ok: true, result: {platform: 'windows/amd64', rust: true, version: expectedVersion}};
  assertInstalledNativeVersion(valid, expectedVersion);
  for (const version of ['3.1.4', '3.5.1', '', undefined, 3.5]) {
    assert.throws(() => assertInstalledNativeVersion({...valid, result: {...valid.result, version}}, expectedVersion), /candidate\.version_mismatch/);
  }
  assert.throws(() => assertInstalledNativeVersion({...valid, ok: false}, expectedVersion), /candidate\.version_mismatch/);
  for (const expected of ['', undefined]) assert.throws(() => assertInstalledNativeVersion(valid, expected), /candidate\.version_mismatch/);
  assert.throws(() => assertInstalledNativeVersion({...valid, result: {...valid.result, platform: 'linux/amd64'}}, expectedVersion), /candidate\.not_rust/);
  assert.throws(() => assertInstalledNativeVersion({...valid, result: {...valid.result, rust: false}}, expectedVersion), /candidate\.not_rust/);
  assertInstalledNativeVersion(valid, expectedVersion);
});

function closureFixture(t) {
  const root = mkdtempSync(join(tmpdir(), 'lycheedev-package-closure-'));
  t.after(() => rmSync(root, {recursive: true}));
  writeFileSync(join(root, 'Cargo.lock'), '# fixed closure input\n');
  const source = join(root, 'input'); mkdirSync(source);
  const closure = {schema: 'lycheedev.rust-dependencies.v1', target: 'x86_64-pc-windows-msvc',
    cargoLockSHA256: createHash('sha256').update(readFileSync(join(root, 'Cargo.lock'))).digest('hex'),
    metadataEvidence: 'original-build-evidence', packages: [{name: 'fixture', licenseFiles: [{text: 'Original license'}]}]};
  const bytes = Buffer.from(JSON.stringify(closure, null, 2) + '\r\n');
  const notices = Buffer.from('Original notices\r\n许可证\r\n');
  writeFileSync(join(source, 'dependency-closure.json'), bytes);
  writeFileSync(join(source, 'THIRD_PARTY_NOTICES'), notices);
  return {root, source, closure, bytes, notices};
}

test('explicit dependency closure is a named input and copies original bytes without generation', t => {
  const parsed = parsePackageOptions(['out', 'luac', '--dependency-closure', 'input', '--artifact', 'record']);
  assert.equal(parsed.dependencyClosurePath, 'input'); assert.equal(parsed.homeArg, undefined);
  const {root, source, closure, bytes, notices} = closureFixture(t), output = join(root, 'output');
  const result = prepareDependencyClosure(root, output, parsed.dependencyClosurePath, () => assert.fail('explicit input must not run Cargo inventory'));
  assert.deepEqual(result, closure);
  for (const [name, expected] of [['dependency-closure.json', bytes], ['THIRD_PARTY_NOTICES', notices]]) {
    assert.deepEqual(readFileSync(join(output, name)), expected);
    assert.deepEqual(readFileSync(join(source, name)), expected, 'build closure remains unchanged');
  }
});

test('missing or mismatched explicit closures fail without generating or staging replacements', t => {
  const {root, source, closure} = closureFixture(t);
  let generated = 0;
  const generate = () => { generated++; assert.fail('explicit input must not fall back'); };
  for (const [field, value, expected] of [
    ['schema', 'lycheedev.rust-dependencies.v0', /dependency_closure_schema/],
    ['target', 'aarch64-pc-windows-msvc', /dependency_closure_target/],
    ['cargoLockSHA256', 'f'.repeat(64), /stale_dependency_closure/],
  ]) {
    writeFileSync(join(source, 'dependency-closure.json'), JSON.stringify({...closure, [field]: value}));
    const output = join(root, field);
    assert.throws(() => prepareDependencyClosure(root, output, source, generate), expected);
    assert.equal(existsSync(output), false);
  }
  writeFileSync(join(source, 'dependency-closure.json'), JSON.stringify(closure));
  rmSync(join(source, 'THIRD_PARTY_NOTICES'));
  assert.throws(() => prepareDependencyClosure(root, join(root, 'missing-notices'), source, generate), /ENOENT/);
  assert.throws(() => prepareDependencyClosure(root, join(root, 'missing-directory'), join(root, 'absent'), generate), /ENOENT/);
  assert.equal(generated, 0);
});

test('default local and build preparation still generates the dependency inventory once', t => {
  const {root, bytes, notices, closure} = closureFixture(t), output = join(root, 'generated');
  let generated = 0;
  const result = prepareDependencyClosure(root, output, undefined, directory => {
    generated++; assert.equal(directory, output); mkdirSync(directory);
    writeFileSync(join(directory, 'dependency-closure.json'), bytes);
    writeFileSync(join(directory, 'THIRD_PARTY_NOTICES'), notices);
  });
  assert.equal(generated, 1); assert.deepEqual(result, closure);
});
