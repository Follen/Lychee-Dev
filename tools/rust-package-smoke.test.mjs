import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdtempSync, mkdirSync, readFileSync, writeFileSync, rmSync, existsSync, renameSync, readdirSync} from 'node:fs';
import {join} from 'node:path';
import {createHash} from 'node:crypto';
import {tmpdir} from 'node:os';
import {gzipSync} from 'node:zlib';
import * as packageTools from './rust-package-smoke.mjs';
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

test('explicit package producer requires sealed CLI and closure before creating output', async t => {
  const root = mkdtempSync(join(tmpdir(), 'lycheedev-produce-')); t.after(() => rmSync(root, {recursive: true}));
  for (const args of [['out', 'luac'], ['out', 'luac', '--artifact', 'record'], ['out', 'luac', '--dependency-closure', 'closure']]) {
    await assert.rejects(() => packageTools.packageProduce(args, root, {execute: () => assert.fail('must not execute')}), /candidate.explicit_package_inputs_required/);
    assert.equal(existsSync(join(root, 'out')), false);
  }
});

function packedFixture(t) {
  const root = mkdtempSync(join(tmpdir(), 'lycheedev-packed-')); t.after(() => rmSync(root, {recursive: true}));
  for (const path of ['crates', 'release', '.cargo', 'producer']) mkdirSync(join(root, path));
  writeFileSync(join(root, 'Cargo.toml'), '[workspace]\n'); writeFileSync(join(root, 'Cargo.lock'), '# fixed\n');
  const sha = bytes => createHash('sha256').update(bytes).digest('hex');
  const cli = Buffer.from('fixture CLI bytes'), compiler = Buffer.from('fixture compiler bytes');
  const resources = [{path: 'tool/luac/luac.exe', bytes: compiler.length, sha256: sha(compiler)}];
  const commit = 'a'.repeat(40), packageVersion = '3.5.0';
  const release = Buffer.from(JSON.stringify({schema: 'lycheedev.release.v1', version: packageVersion, commit, runtime: 'rust',
    binaries: {'windows-amd64': {path: 'native/windows-amd64/lycheedev.exe', bytes: cli.length, sha256: sha(cli)}}, resources}));
  const files = {'package.json': Buffer.from(JSON.stringify({name: 'lycheedev', version: packageVersion})), 'release.json': release,
    'native/windows-amd64/lycheedev.exe': cli, 'payload/tool/luac/luac.exe': compiler,
    'payload/skill/scripts/lycheedev.mjs': Buffer.from('// launcher fixture')};
  const tgz = join(root, 'producer/candidate.tgz'); writeFileSync(tgz, tarball(files));
  const report = {schema: 'lycheedev.rust-npm-package.v1', state: 'packed', profile: 'release', developmentOnly: true, packageVersion,
    commit, sourceTreeSHA256: sourceDigest(root), binarySHA256: sha(cli), releaseManifestSHA256: sha(release), resourceInventorySHA256: sha(Buffer.from(JSON.stringify(resources))),
    compilerSHA256: sha(compiler), tgz: 'candidate.tgz', tgzPathBase: 'report', tgzSHA256: sha(readFileSync(tgz)), npmSmoke: 'not_run', validation: {state: 'not_run'}};
  const reportPath = join(root, 'producer/report.json'); writeFileSync(reportPath, JSON.stringify(report));
  const calls = [];
  const execute = (command, args) => {
    calls.push({command, args});
    let stdout = '';
    if (command === 'git') stdout = args[0] === 'status' ? '' : commit;
    else if (args.includes('install')) {
      const installed = join(args[args.indexOf('--prefix') + 1], 'node_modules/lycheedev');
      for (const [path, bytes] of Object.entries(files)) { const destination = join(installed, path); mkdirSync(join(destination, '..'), {recursive: true}); writeFileSync(destination, bytes); }
    } else if (args.includes('version')) stdout = JSON.stringify({ok: true, result: {version: packageVersion, rust: true, platform: 'windows/amd64'}});
    else if (!args.includes('-v') && !args.includes('-p')) assert.fail(`unexpected command: ${command} ${args.join(' ')}`);
    return {status: 0, handlesReaped: true, stdout: Buffer.from(stdout), stderr: Buffer.alloc(0), logs: []};
  };
  return {root, reportPath, report, tgz, files, calls, execute};
}

function tarball(files) {
  // A real gzip/tar fixture; injected npm processes never execute product code.
  const chunks = [];
  for (const [name, bytes] of Object.entries(files)) {
    const header = Buffer.alloc(512); header.write(`package/${name}`); header.write('0000644\0', 100); header.write('0000000\0', 108); header.write('0000000\0', 116);
    header.write(bytes.length.toString(8).padStart(11, '0') + '\0', 124); header.write('00000000000\0', 136); header.fill(32, 148, 156); header[156] = 48; header.write('ustar\0', 257); header.write('00', 263);
    header.write([...header].reduce((n, value) => n + value, 0).toString(8).padStart(6, '0') + '\0 ', 148);
    chunks.push(header, bytes, Buffer.alloc((512 - bytes.length % 512) % 512));
  }
  return gzipSync(Buffer.concat([...chunks, Buffer.alloc(1024)]));
}

test('package validation consumes the original TGZ without assembly, pack or Cargo', async t => {
  const {root, reportPath, report, tgz, calls, execute} = packedFixture(t);
  const before = readFileSync(tgz), reportBytes = readFileSync(reportPath);
  const result = await packageTools.packageValidate(['validated', '--package-report', reportPath], root, {execute});
  assert.equal(result.schema, 'lycheedev.rust-npm-smoke.v1'); assert.equal(result.assemblyOnly, false);
  assert.equal(result.state, 'partial_passed'); assert.equal(result.validation.state, 'not_run'); assert.equal(result.npmSmoke, 'passed');
  assert.equal(result.tgzSHA256, report.tgzSHA256); assert.equal(join(root, result.tgz), tgz); assert.equal(result.tgzPathBase, 'workspace');
  assert.equal(result.packageReportSHA256, createHash('sha256').update(reportBytes).digest('hex'));
  assert.deepEqual(readFileSync(tgz), before); assert.deepEqual(readFileSync(reportPath), reportBytes);
  const npmCalls = calls.filter(call => call.args.some(arg => arg.endsWith('npm-cli.js')));
  assert.equal(npmCalls.length, 1); assert(npmCalls[0].args.includes('install')); assert(npmCalls[0].args.includes(tgz));
  assert(calls.some(call => call.args[0]?.includes('payload') && call.args.includes('version')), 'executes installed Skill launcher');
  assert.equal(existsSync(join(root, 'validated/package')), false, 'no package assembly tree');
});

test('package validation rejects changed TGZ or stale producer identity before install', async t => {
  for (const defect of ['tgz', 'schema', 'state', 'sourceTreeSHA256', 'commit', 'tgzPathBase']) {
    const f = packedFixture(t);
    if (defect === 'tgz') writeFileSync(f.tgz, 'changed archive');
    else { f.report[defect] = 'invalid'; writeFileSync(f.reportPath, JSON.stringify(f.report)); }
    await assert.rejects(() => packageTools.packageValidate(['validated', '--package-report', f.reportPath], f.root, {execute: f.execute}), /candidate\./);
    assert(!f.calls.some(call => call.args.includes('install')));
    assert.equal(existsSync(join(f.root, 'validated')), false);
  }
});

test('package validation survives moving the producer report and TGZ together', async t => {
  const f = packedFixture(t), moved = join(f.root, 'downloaded-artifact');
  renameSync(join(f.root, 'producer'), moved);
  const result = await packageTools.packageValidate(['validated', '--package-report', join(moved, 'report.json')], f.root, {execute: f.execute});
  assert.equal(join(f.root, result.tgz), join(moved, 'candidate.tgz')); assert.equal(result.tgzSHA256, f.report.tgzSHA256);
});

test('consumer rejects producer for a different valid commit without creating output or installing', async t => {
  const f = packedFixture(t); f.report.commit = 'b'.repeat(40); writeFileSync(f.reportPath, JSON.stringify(f.report));
  await assert.rejects(() => packageTools.packageValidate(['validated', '--package-report', f.reportPath], f.root, {execute: f.execute}), /candidate.package_commit_mismatch/);
  assert.equal(existsSync(join(f.root, 'validated')), false); assert(!f.calls.some(call => call.args.includes('install')));
});

test('consumer rejects changed installed bytes before launch and preserves a failed not-run report', async t => {
  const f = packedFixture(t); f.files['native/windows-amd64/lycheedev.exe'] = Buffer.from('different executable');
  await assert.rejects(() => packageTools.packageValidate(['validated', '--package-report', f.reportPath], f.root, {execute: f.execute}), /candidate.installed_binary_changed/);
  const report = JSON.parse(readFileSync(join(f.root, 'validated/report.json')));
  assert.equal(report.state, 'failed'); assert.equal(report.npmSmoke, 'not_run'); assert.equal(report.validation.state, 'not_run');
  assert.equal(report.tgzSHA256, f.report.tgzSHA256); assert(!f.calls.some(call => call.args.includes('version')));
});

test('consumer rejects TGZ replacement during installation without marking smoke passed', async t => {
  const f = packedFixture(t);
  const execute = (command, args, options) => {
    const result = f.execute(command, args, options);
    if (args.includes('install')) writeFileSync(f.tgz, 'replacement archive');
    return result;
  };
  await assert.rejects(() => packageTools.packageValidate(['validated', '--package-report', f.reportPath], f.root, {execute}), /candidate.package_input_changed/);
  const report = JSON.parse(readFileSync(join(f.root, 'validated/report.json')));
  assert.equal(report.state, 'failed'); assert.equal(report.npmSmoke, 'not_run');
});

test('validation accepts only an explicit existing package report and paired Source inputs', () => {
  for (const args of [[], ['out'], ['out', '--artifact', 'record'], ['out', '--package-report'],
    ['out', '--package-report', 'one', '--package-report', 'two'], ['out', 'luac', '--package-report', 'report'],
    ['out', '--package-report', 'report', '--home', 'home'], ['out', '--package-report', 'report', '--assembly-only']]) {
    assert.throws(() => packageTools.parsePackageValidationOptions(args), /candidate\./);
  }
});

test('optional Source validation executes the installed package CLI and keeps overall partial coverage', async t => {
  const f = packedFixture(t);
  const execute = (command, args, options) => {
    if (!args.includes('source')) return f.execute(command, args, options);
    f.calls.push({command, args});
    return {status: 0, handlesReaped: true, logs: [], stderr: Buffer.alloc(0), stdout: Buffer.from(JSON.stringify({ok: true,
      result: {staticValid: true, syntaxCompilation: {passed: true, compilerSHA256: f.report.compilerSHA256, checkedLua: 1}}, captures: []}))};
  };
  const report = await packageTools.packageValidate(['validated', '--package-report', f.reportPath, '--home', 'fixed-home', '--pin', 'PIN-fixed'], f.root, {execute});
  assert.equal(report.validation.state, 'passed'); assert.equal(report.state, 'partial_passed');
  const source = f.calls.find(call => call.args.includes('source'));
  assert.equal(source.args[0], join(f.root, 'validated/installed/node_modules/lycheedev/bin/lycheedev.mjs'));
  assert.equal(source.args[source.args.indexOf('--snapshot') + 1], 'PIN-fixed');
  assert.equal(source.args[source.args.indexOf('--home') + 1], join(f.root, 'fixed-home'));
});

test('explicit producer assembles once and packs one TGZ without install or Cargo', async t => {
  const f = packedFixture(t), {root} = f;
  for (const path of ['packages/npm/lycheedev/bin', 'addon', 'skills/lycheedev/scripts', 'luac', 'native']) mkdirSync(join(root, path), {recursive: true});
  for (const name of ['README.md', 'LICENSE', 'THIRD_PARTY_NOTICES']) writeFileSync(join(root, 'packages/npm/lycheedev', name), name);
  writeFileSync(join(root, 'packages/npm/lycheedev/package.json'), JSON.stringify({name: 'lycheedev', version: f.report.packageVersion}));
  writeFileSync(join(root, 'packages/npm/lycheedev/bin/lycheedev.mjs'), '// npm launcher');
  writeFileSync(join(root, 'skills/lycheedev/scripts/lycheedev.mjs'), '// skill launcher');
  writeFileSync(join(root, 'addon/Addon.lua'), 'return true'); writeFileSync(join(root, 'luac/LICENSE'), 'compiler license');
  writeFileSync(join(root, 'release/npm-channel.json'), JSON.stringify({version: f.report.packageVersion}));
  const native = join(root, 'native/cli.exe'); writeFileSync(native, f.files['native/windows-amd64/lycheedev.exe']);
  const artifactPath = join(root, 'native/artifact.json');
  writeFileSync(artifactPath, JSON.stringify({schema: 'lycheedev.rust-build-artifact.v1', profile: 'release', sourceBinary: native,
    binarySHA256: f.report.binarySHA256, inputHashes: {sourceTree: sourceDigest(root)}}));
  const closurePath = join(root, 'closure'); mkdirSync(closurePath);
  const closure = {schema: 'lycheedev.rust-dependencies.v1', target: 'x86_64-pc-windows-msvc', cargoLockSHA256: createHash('sha256').update(readFileSync(join(root, 'Cargo.lock'))).digest('hex')};
  writeFileSync(join(closurePath, 'dependency-closure.json'), JSON.stringify(closure)); writeFileSync(join(closurePath, 'THIRD_PARTY_NOTICES'), 'dependency notices');
  const commands = [];
  const execute = (command, args, options) => {
    commands.push({command, args});
    if (!args.includes('pack')) return f.execute(command, args, options);
    const files = {};
    function visit(dir, prefix = '') { for (const entry of readdirSync(dir, {withFileTypes: true})) { const path = join(dir, entry.name), name = prefix + entry.name; if (entry.isDirectory()) visit(path, name + '/'); else files[name] = readFileSync(path); } }
    visit(options.cwd);
    const output = args[args.indexOf('--pack-destination') + 1]; writeFileSync(join(output, 'candidate.tgz'), tarball(files));
    return {status: 0, handlesReaped: true, stdout: Buffer.from(JSON.stringify([{filename: 'candidate.tgz'}])), stderr: Buffer.alloc(0), logs: []};
  };
  const stageCompiler = (pkg, resources) => {
    const bytes = f.files['payload/tool/luac/luac.exe'], path = 'tool/luac/luac.exe';
    mkdirSync(join(pkg, 'payload/tool/luac'), {recursive: true}); writeFileSync(join(pkg, 'payload', path), bytes);
    resources.push({path, bytes: bytes.length, sha256: f.report.compilerSHA256});
  };
  const report = await packageTools.packageProduce(['packed', 'luac', '--artifact', artifactPath, '--dependency-closure', closurePath, '--profile', 'release'], root, {execute, stageCompiler});
  assert.equal(report.schema, 'lycheedev.rust-npm-package.v1'); assert.equal(report.state, 'packed');
  assert.equal(report.npmSmoke, 'not_run'); assert.equal(report.validation.state, 'not_run'); assert.equal(report.tgzPathBase, 'report');
  assert.equal(commands.filter(call => call.args.includes('pack')).length, 1);
  assert(commands.every(call => call.command === 'git' || call.args.includes('pack')), 'no install, Cargo or closure generator');
  assert.equal(existsSync(join(root, 'packed/installed')), false);
  assert.equal(report.tgzSHA256, createHash('sha256').update(readFileSync(join(root, 'packed', report.tgz))).digest('hex'));
  const bytes = readFileSync(join(root, 'packed/report.json'));
  await assert.rejects(() => packageTools.packageProduce(['packed', 'luac', '--artifact', artifactPath, '--dependency-closure', closurePath, '--profile', 'release'], root, {execute, stageCompiler}), /EEXIST/);
  assert.equal(commands.filter(call => call.args.includes('pack')).length, 1);
  assert.deepEqual(readFileSync(join(root, 'packed/report.json')), bytes, 'existing producer receipt stays unchanged');
});
