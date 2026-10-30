// Isolated, unpublished candidate assembly and optional npm smoke. No publishing.
import {mkdirSync, readdirSync, readFileSync, writeFileSync, copyFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {join, resolve, dirname, delimiter, relative, isAbsolute} from 'node:path';
import {fileURLToPath} from 'node:url';
import {homedir} from 'node:os';
import {stageLuac} from './luac.mjs';
import {buildRust, verifyArtifact, sourceDigest} from './rust-build.mjs';
import {beginScratch, contained} from './rust-disk.mjs';
import {ownedSpawnSync} from './rust-owned-process.mjs';
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const writeJson = (path, value) => writeFileSync(path, JSON.stringify(value, null, 2) + '\n');

export function parsePackageOptions(argv) {
  const positional = [], values = {}, seen = new Set();
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (!arg.startsWith('--')) { positional.push(arg); continue; }
    if (!['--artifact', '--profile', '--home', '--pin', '--assembly-only', '--dependency-closure'].includes(arg)) throw new Error(`candidate.unknown_option: ${arg}`);
    if (seen.has(arg)) throw new Error(`candidate.duplicate_option: ${arg}`);
    seen.add(arg);
    if (arg === '--assembly-only') values[arg] = true;
    else { const value = argv[++i]; if (!value || value.startsWith('--')) throw new Error(`candidate.option_value_missing: ${arg}`); values[arg] = value; }
  }
  if (!positional[0] || !positional[1] || positional.length > 4) throw new Error('usage: node tools/rust-package-smoke.mjs <new-directory> <verified-luac-build> [fixed-home fixed-pin] [--artifact <artifact.json>] [--profile debug|release] [--assembly-only] [--dependency-closure <directory>]');
  for (const [index, flag] of [[2, '--home'], [3, '--pin']]) if (positional[index] && values[flag]) throw new Error(`candidate.duplicate_option: ${flag}`);
  const homeArg = positional[2] ?? values['--home'], pinArg = positional[3] ?? values['--pin'];
  if (Boolean(homeArg) !== Boolean(pinArg)) throw new Error('candidate.home_pin_pair_required');
  const profile = values['--profile'] ?? 'debug';
  if (!['debug', 'release'].includes(profile)) throw new Error('candidate.invalid_profile');
  if (values['--assembly-only'] && homeArg) throw new Error('candidate.assembly_only_does_not_validate');
  return {outArg: positional[0], luacArg: positional[1], homeArg, pinArg, artifactPath: values['--artifact'], dependencyClosurePath: values['--dependency-closure'], profile, assemblyOnly: values['--assembly-only'] === true};
}

// Explicit CI inputs contain all license text; never consult Cargo or its cache.
// The build manifest authenticates file hashes before CI supplies this directory.
export function prepareDependencyClosure(root, output, inputDirectory, generate) {
  let source;
  if (inputDirectory) source = resolve(root, inputDirectory);
  else { generate(output); source = output; }
  const bytes = readFileSync(join(source, 'dependency-closure.json'));
  const notices = readFileSync(join(source, 'THIRD_PARTY_NOTICES'));
  const closure = JSON.parse(bytes);
  if (closure.schema !== 'lycheedev.rust-dependencies.v1') throw new Error('candidate.dependency_closure_schema');
  if (closure.target !== 'x86_64-pc-windows-msvc') throw new Error('candidate.dependency_closure_target');
  if (closure.cargoLockSHA256 !== hash(readFileSync(join(root, 'Cargo.lock')))) throw new Error('candidate.stale_dependency_closure');
  if (inputDirectory) {
    mkdirSync(output);
    writeFileSync(join(output, 'dependency-closure.json'), bytes);
    writeFileSync(join(output, 'THIRD_PARTY_NOTICES'), notices);
  }
  return closure;
}

export function assertArtifactSource(artifact, currentSourceDigest, profile) {
  if (!/^[a-f0-9]{64}$/.test(currentSourceDigest) || artifact.inputHashes?.sourceTree !== currentSourceDigest) throw new Error('candidate.artifact_source_mismatch');
  if ((profile === 'release' && !['release', 'symbols'].includes(artifact.profile)) || (profile === 'debug' && !['dev', 'debug'].includes(artifact.profile)) || !['debug', 'release'].includes(profile)) throw new Error('candidate.artifact_profile_mismatch');
}

export function assertInstalledNativeVersion(response, expectedVersion) {
  if (response?.result?.platform !== 'windows/amd64' || !response.result.rust) throw new Error('candidate.not_rust');
  if (response.ok !== true || typeof expectedVersion !== 'string' || expectedVersion.length === 0 || response.result.version !== expectedVersion) throw new Error('candidate.version_mismatch');
}

export function assertSourceSyntaxValidation(response, compilerSHA256) {
  if (!response.ok || !response.result?.staticValid || !response.result.syntaxCompilation?.passed || response.result.syntaxCompilation.compilerSHA256 !== compilerSHA256) throw new Error('candidate.source_syntax_validation_failed');
}

export function packageSmokeEnvironment(scratch, parent = process.env) {
  const env = Object.fromEntries(Object.entries(parent).filter(([name]) => !/^(LYCHEEDEV_|NPM_CONFIG_)/i.test(name)
    && !['HOME', 'USERPROFILE', 'CODEX_HOME', 'APPDATA', 'LOCALAPPDATA', 'TEMP', 'TMP'].includes(name.toUpperCase())));
  const user = join(scratch, 'user');
  // Compiler dependencies stay in their existing offline toolchain caches;
  // user/product/npm state belongs exclusively to this scratch owner.
  return {...env, CARGO_HOME: parent.CARGO_HOME ?? join(homedir(), '.cargo'), RUSTUP_HOME: parent.RUSTUP_HOME ?? join(homedir(), '.rustup'),
    HOME: user, USERPROFILE: user, CODEX_HOME: join(user, '.codex'),
    APPDATA: join(user, 'AppData/Roaming'), LOCALAPPDATA: join(user, 'AppData/Local'),
    TEMP: join(scratch, 'temp'), TMP: join(scratch, 'temp'), LYCHEEDEV_HOME: join(scratch, 'home'),
    npm_config_cache: join(scratch, 'npm-cache'), npm_config_userconfig: join(user, '.npmrc'), npm_config_globalconfig: join(scratch, 'npm-globalrc')};
}

async function withPackageCommands(argv, root, execute, work) {
  if (process.platform !== 'win32' || process.arch !== 'x64') throw new Error('candidate.windows_amd64_required');
  root = resolve(root);
  const job = beginScratch(root, 'rust-package-smoke', {command: [process.execPath, fileURLToPath(import.meta.url), ...argv]}), logs = [];
  const isolatedEnvironment = packageSmokeEnvironment(job.directory);
  let state = 'failed', handlesReaped = true, commandSequence = 0;
  function run(command, args, cwd = root, environment = {}) {
    const index = commandSequence++, metadata = join(job.evidence, `command-${index}.json`), stdout = join(job.evidence, `stdout-${index}.bin`), stderr = join(job.evidence, `stderr-${index}.bin`);
    handlesReaped = false;
    const result = execute(command, args, {cwd, env: {...isolatedEnvironment, ...environment}, evidence: job.evidence, timeout: 300000});
    handlesReaped = result.handlesReaped === true; logs.push(...result.logs);
    writeJson(metadata, {command, args, cwd, exit: result.status, signal: result.signal, error: result.error?.message, handlesReaped});
    writeFileSync(stdout, result.stdout ?? Buffer.alloc(0)); writeFileSync(stderr, result.stderr ?? Buffer.alloc(0)); logs.push(metadata, stdout, stderr);
    if (!handlesReaped || result.error || result.status !== 0) throw new Error(`candidate.failed: ${command}: ${result.error?.message ?? result.status}`);
    return result.stdout.toString();
  }
  const npm = (args, cwd) => run(process.execPath, [process.env.npm_execpath || join(dirname(process.execPath), 'node_modules/npm/bin/npm-cli.js'), ...args], cwd);
  try {
    for (const key of ['HOME', 'TEMP', 'APPDATA', 'LOCALAPPDATA']) mkdirSync(isolatedEnvironment[key], {recursive: true});
    writeFileSync(isolatedEnvironment.npm_config_userconfig, ''); writeFileSync(isolatedEnvironment.npm_config_globalconfig, '');
    const result = await work({run, npm, logs}); state = 'succeeded'; return result;
  } catch (error) {
    if (error.buildEvidence) { const build = JSON.parse(readFileSync(error.buildEvidence)); if (!build.handlesReaped) handlesReaped = false; }
    const failure = join(job.evidence, 'failure.json'); writeJson(failure, {error: error.message, buildEvidence: error.buildEvidence}); logs.push(failure); throw error;
  } finally {
    job.seal({state: handlesReaped ? state : 'interrupted', handlesReaped, logs});
    if (handlesReaped) job.cleanup();
  }
}

async function produceOrSmoke(argv, root, {execute = ownedSpawnSync, stageCompiler = stageLuac, produceOnly = false} = {}) {
  const options = parsePackageOptions(argv);
  const {outArg, luacArg, homeArg, pinArg, artifactPath, dependencyClosurePath, profile, assemblyOnly} = options;
  if (produceOnly && (!artifactPath || !dependencyClosurePath)) throw new Error('candidate.explicit_package_inputs_required');
  if (produceOnly && (assemblyOnly || homeArg)) throw new Error('candidate.production_does_not_validate');
  root = resolve(root);
  const out = contained(root, resolve(root, outArg)), luacRoot = resolve(root, luacArg);
  return withPackageCommands(produceOnly ? ['produce', ...argv] : argv, root, execute, async ({run, npm, logs}) => {
    mkdirSync(out); const pkgRoot = join(out, 'package'); mkdirSync(pkgRoot);
    // Explicit sealed artifacts are reusable only against their exact current
    // build inputs. Never relabel a historical EXE with today's tree/luac pin.
    const recordPath = artifactPath ? resolve(root, artifactPath) : join(out, 'native-artifact/artifact.json');
    const artifact = artifactPath ? verifyArtifact(root, recordPath) : buildRust(root, join(out, 'native-artifact'), {profile: profile === 'debug' ? 'dev' : 'release', oneShot: false, isolateUser: true, owner: 'rust-package-smoke'});
    const currentSourceDigest = sourceDigest(root);
    assertArtifactSource(artifact, currentSourceDigest, profile);
    const artifactRecordSHA256 = hash(readFileSync(recordPath));
    const pkg = JSON.parse(readFileSync(join(root, 'packages/npm/lycheedev/package.json')));
    pkg.bin = {lycheedev: 'bin/lycheedev.mjs'};
    pkg.private = true; // This rehearsal is never a publishable package.
    writeJson(join(pkgRoot, 'package.json'), pkg);
    for (const name of ['README.md', 'LICENSE', 'THIRD_PARTY_NOTICES']) copyFileSync(join(root, 'packages/npm/lycheedev', name), join(pkgRoot, name));
    const closureRoot = join(out, 'license-closure');
    prepareDependencyClosure(root, closureRoot, dependencyClosurePath,
      directory => run(process.execPath, [join(root, 'tools/rust-dependencies.mjs'), directory]));
    writeFileSync(join(pkgRoot, 'THIRD_PARTY_NOTICES'), readFileSync(join(pkgRoot, 'THIRD_PARTY_NOTICES'), 'utf8') + '\n\nRust dependency closure\n\n' + readFileSync(join(closureRoot, 'THIRD_PARTY_NOTICES'), 'utf8') + '\n\nLua 5.1.5 compiler/runtime\n\n' + readFileSync(join(luacRoot, 'LICENSE'), 'utf8'));
    mkdirSync(join(pkgRoot, 'bin')); copyFileSync(join(root, 'packages/npm/lycheedev/bin/lycheedev.mjs'), join(pkgRoot, 'bin/lycheedev.mjs'));
    const resources = [];
    function copyTree(source, prefix) {
      for (const entry of readdirSync(source, {withFileTypes: true}).sort((a, b) => a.name.localeCompare(b.name))) {
        if (entry.isSymbolicLink()) throw new Error('candidate.symlink');
        const src = join(source, entry.name), path = `${prefix}/${entry.name}`;
        if (entry.isDirectory()) copyTree(src, path);
        else {
          if (!entry.isFile()) throw new Error('candidate.non_regular_resource');
          const bytes = readFileSync(src); mkdirSync(dirname(join(pkgRoot, 'payload', path)), {recursive: true}); writeFileSync(join(pkgRoot, 'payload', path), bytes);
          resources.push({path, bytes: bytes.length, sha256: hash(bytes)});
        }
      }
    }
    copyTree(join(root, 'addon'), 'addon'); copyTree(join(root, 'skills/lycheedev'), 'skill'); stageCompiler(pkgRoot, resources, luacRoot);
    resources.sort((a, b) => a.path < b.path ? -1 : a.path > b.path ? 1 : 0);
    const binary = readFileSync(artifact.sourceBinary);
    if (hash(binary) !== artifact.binarySHA256) throw new Error('candidate.artifact_changed');
    mkdirSync(join(pkgRoot, 'native/windows-amd64'), {recursive: true}); writeFileSync(join(pkgRoot, 'native/windows-amd64/lycheedev.exe'), binary);
    const commit = run('git', ['rev-parse', 'HEAD']).trim(), workspaceDirty = run('git', ['status', '--porcelain', '--untracked-files=normal']).trim() !== '';
    if (!/^[a-f0-9]{40}$/.test(commit)) throw new Error('candidate.invalid_commit');
    const release = {schema: 'lycheedev.release.v1', version: pkg.version, commit, runtime: 'rust', npmChannel: JSON.parse(readFileSync(join(root, 'release/npm-channel.json'))), binaries: {'windows-amd64': {path: 'native/windows-amd64/lycheedev.exe', bytes: binary.length, sha256: hash(binary)}}, resources};
    if (release.npmChannel.version !== release.version) throw new Error('candidate.channel_version_mismatch');
    writeJson(join(pkgRoot, 'release.json'), release);
    const report = {
      schema: 'lycheedev.rust-npm-smoke.v1', state: 'assembled', developmentOnly: true, assemblyOnly, profile, packageRoot: pkgRoot,
      commit, workspaceDirty, sourceTreeSHA256: currentSourceDigest, artifactRecordSHA256, binarySHA256: artifact.binarySHA256,
      releaseManifestSHA256: hash(readFileSync(join(pkgRoot, 'release.json'))), resourceInventorySHA256: hash(Buffer.from(JSON.stringify(resources))),
      compilerSHA256: hash(readFileSync(join(pkgRoot, 'payload/tool/luac/luac.exe'))), licenseClosureSHA256: hash(readFileSync(join(closureRoot, 'dependency-closure.json'))),
      packageVersion: pkg.version, runtimeDependencies: pkg.dependencies || {},
      npmSmoke: 'not_run', validation: {state: 'not_run'}, semanticRuntime: {engine: 'emmy-native', luac: 'pinned_inventory', publicValidation: 'not_run'},
      limitations: [`${profile} unpublished candidate only`, 'business and performance acceptance are separate from package assembly', 'official corresponding-source rebuild and publication gates not run'],
    };
    if (!assemblyOnly) {
      const packed = JSON.parse(npm(['pack', '--ignore-scripts', '--json', '--pack-destination', out], pkgRoot))[0];
      const tgz = contained(out, resolve(out, packed.filename));
      if (produceOnly) {
        Object.assign(report, {schema: 'lycheedev.rust-npm-package.v1', state: 'packed',
          tgz: relative(out, tgz).replaceAll('\\', '/'), tgzPathBase: 'report', tgzSHA256: hash(readFileSync(tgz))});
      } else {
        Object.assign(report, installPackedPackage({root, out, tgz, production: report, homeArg, pinArg, run, npm}),
          {state: 'partial_passed', npmSmoke: 'passed', tgz, tgzSHA256: hash(readFileSync(tgz))});
      }
    }
    // Staging and optional smoke must not change any compiled input or sealed EXE.
    assertArtifactSource(verifyArtifact(root, recordPath), sourceDigest(root), profile);
    if (hash(readFileSync(recordPath)) !== artifactRecordSHA256 || hash(readFileSync(join(pkgRoot, 'native/windows-amd64/lycheedev.exe'))) !== artifact.binarySHA256 || hash(readFileSync(join(pkgRoot, 'release.json'))) !== report.releaseManifestSHA256) throw new Error('candidate.identity_changed_during_assembly');
    for (const resource of resources) {
      const bytes = readFileSync(join(pkgRoot, 'payload', resource.path));
      if (bytes.length !== resource.bytes || hash(bytes) !== resource.sha256) throw new Error(`candidate.payload_changed: ${resource.path}`);
    }
    writeJson(join(out, 'report.json'), report); logs.push(join(out, 'report.json'));
    return report;
  });
}

export async function packageProduce(argv, root = process.cwd(), options = {}) {
  return produceOrSmoke(argv, root, {...options, produceOnly: true});
}

// Local convenience only. CI uses the explicit produce/validate subcommands.
export async function packageSmoke(argv, root = process.cwd(), options = {}) {
  return produceOrSmoke(argv, root, options);
}

export function parsePackageValidationOptions(argv) {
  const positional = [], values = {};
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (!arg.startsWith('--')) { positional.push(arg); continue; }
    if (!['--package-report', '--home', '--pin'].includes(arg)) throw new Error(`candidate.unknown_option: ${arg}`);
    if (values[arg] !== undefined) throw new Error(`candidate.duplicate_option: ${arg}`);
    const value = argv[++i];
    if (!value || value.startsWith('--')) throw new Error(`candidate.option_value_missing: ${arg}`);
    values[arg] = value;
  }
  if (positional.length !== 1 || !values['--package-report']) throw new Error('candidate.package_validation_inputs_required');
  if (Boolean(values['--home']) !== Boolean(values['--pin'])) throw new Error('candidate.home_pin_pair_required');
  return {outArg: positional[0], packageReportPath: values['--package-report'], homeArg: values['--home'], pinArg: values['--pin']};
}

function readPackedPackage(root, reportPath) {
  const path = contained(root, resolve(root, reportPath)), bytes = readFileSync(path), report = JSON.parse(bytes);
  if (report.schema !== 'lycheedev.rust-npm-package.v1' || report.state !== 'packed') throw new Error('candidate.package_not_packed');
  if (!['debug', 'release'].includes(report.profile)) throw new Error('candidate.invalid_profile');
  if (report.sourceTreeSHA256 !== sourceDigest(root)) throw new Error('candidate.artifact_source_mismatch');
  if (!/^[a-f0-9]{40}$/.test(report.commit)) throw new Error('candidate.invalid_commit');
  if (report.tgzPathBase !== 'report' || typeof report.tgz !== 'string' || !report.tgz || isAbsolute(report.tgz)) throw new Error('candidate.package_tgz_path');
  const tgz = contained(dirname(path), resolve(dirname(path), report.tgz));
  if (hash(readFileSync(tgz)) !== report.tgzSHA256) throw new Error('candidate.package_tgz_changed');
  return {path, bytes, report, tgz};
}

function verifyInstalledPackage(installed, report) {
  const manifestBytes = readFileSync(join(installed, 'release.json'));
  if (hash(manifestBytes) !== report.releaseManifestSHA256) throw new Error('candidate.installed_release_manifest_changed');
  const manifest = JSON.parse(manifestBytes), pkg = JSON.parse(readFileSync(join(installed, 'package.json')));
  if (manifest.schema !== 'lycheedev.release.v1' || manifest.runtime !== 'rust' || manifest.commit !== report.commit
    || manifest.version !== report.packageVersion || pkg.version !== report.packageVersion) throw new Error('candidate.installed_identity_mismatch');
  const binary = manifest.binaries?.['windows-amd64'];
  if (binary?.path !== 'native/windows-amd64/lycheedev.exe' || binary.sha256 !== report.binarySHA256) throw new Error('candidate.installed_binary_identity');
  const bytes = readFileSync(join(installed, binary.path));
  if (bytes.length !== binary.bytes || hash(bytes) !== report.binarySHA256) throw new Error('candidate.installed_binary_changed');
  if (!Array.isArray(manifest.resources) || hash(Buffer.from(JSON.stringify(manifest.resources))) !== report.resourceInventorySHA256) throw new Error('candidate.installed_resource_inventory');
  for (const resource of manifest.resources) {
    const path = contained(join(installed, 'payload'), resolve(installed, 'payload', resource.path));
    const bytes = readFileSync(path);
    if (bytes.length !== resource.bytes || hash(bytes) !== resource.sha256) throw new Error(`candidate.installed_resource_changed: ${resource.path}`);
  }
  if (hash(readFileSync(join(installed, 'payload/tool/luac/luac.exe'))) !== report.compilerSHA256) throw new Error('candidate.installed_compiler_changed');
  return pkg;
}

function installPackedPackage({root, out, tgz, production, homeArg, pinArg, run, npm}) {
  // npm's real Windows global layout and shims, confined to this output prefix.
  const installation = join(out, 'installed'); mkdirSync(installation);
  npm(['install', '--global', '--prefix', installation, '--ignore-scripts', '--no-audit', '--no-fund', '--offline', tgz], out);
  const installed = join(installation, 'node_modules/lycheedev'), pkg = verifyInstalledPackage(installed, production);
  const version = JSON.parse(run(process.execPath, [join(installed, 'payload/skill/scripts/lycheedev.mjs'), 'version', '--format', 'json'], out,
    {PATH: `${installation}${delimiter}${process.env.PATH ?? process.env.Path ?? ''}`, LYCHEEDEV_BIN: ''}));
  assertInstalledNativeVersion(version, production.packageVersion);
  const payload = join(installed, 'payload/tool/luac'); run(join(payload, 'luac.exe'), ['-v'], out);
  writeFileSync(join(out, 'valid.lua'), 'local value = 1\nreturn value\n'); run(join(payload, 'luac.exe'), ['-p', join(out, 'valid.lua')], out);
  let validation = {state: 'not_run'};
  if (homeArg) {
    const addon = join(out, 'test-addon'); mkdirSync(addon);
    writeFileSync(join(addon, 'Test.toc'), '## Interface: 120100\nGood.lua\n'); writeFileSync(join(addon, 'Good.lua'), 'local value = 1\nreturn value\n');
    const response = JSON.parse(run(process.execPath, [join(installed, 'bin/lycheedev.mjs'), 'source', 'validate', '--home', resolve(root, homeArg), '--snapshot', pinArg, '--path', addon, '--toc', 'Test.toc', '--format', 'json'], out));
    writeJson(join(out, 'source-validate.json'), response);
    assertSourceSyntaxValidation(response, production.compilerSHA256);
    validation = {state: 'passed', compilerSHA256: response.result.syntaxCompilation.compilerSHA256, checkedLua: response.result.syntaxCompilation.checkedLua, captures: response.captures.map(c => c.id)};
  }
  return {validation, ignoreScripts: true, offlineInstall: true, installationKind: 'isolated-global-prefix',
    installedPrefix: installation, installedRoot: installed, skillLauncherSmoke: 'passed',
    runtimeDependencies: pkg.dependencies || {}, platform: version.result.platform, version};
}

export async function packageValidate(argv, root = process.cwd(), {execute = ownedSpawnSync} = {}) {
  const {outArg, packageReportPath, homeArg, pinArg} = parsePackageValidationOptions(argv);
  root = resolve(root);
  const input = readPackedPackage(root, packageReportPath), {report: production, tgz} = input;
  const out = contained(root, resolve(root, outArg));
  return withPackageCommands(['validate', ...argv], root, execute, async ({run, npm, logs}) => {
    if (run('git', ['rev-parse', 'HEAD']).trim() !== production.commit) throw new Error('candidate.package_commit_mismatch');
    mkdirSync(out);
    const reportPath = join(out, 'report.json');
    const report = {...production, schema: 'lycheedev.rust-npm-smoke.v1', assemblyOnly: false, state: 'failed', npmSmoke: 'not_run',
      validation: {state: 'not_run'}, packageReport: relative(root, input.path).replaceAll('\\', '/'), packageReportSHA256: hash(input.bytes),
      tgz: relative(root, tgz).replaceAll('\\', '/'), tgzPathBase: 'workspace'};
    try {
      const smoke = installPackedPackage({root, out, tgz, production, homeArg, pinArg, run, npm});
      // Authentication and installation must not modify or replace the input archive.
      if (hash(readFileSync(tgz)) !== production.tgzSHA256 || hash(readFileSync(input.path)) !== report.packageReportSHA256) throw new Error('candidate.package_input_changed');
      if (sourceDigest(root) !== production.sourceTreeSHA256) throw new Error('candidate.artifact_source_mismatch');
      Object.assign(report, smoke, {state: 'partial_passed', npmSmoke: 'passed'});
      return report;
    } catch (error) { report.error = error.message; throw error; }
    finally { writeJson(reportPath, report); logs.push(reportPath); }
  });
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [mode, ...args] = process.argv.slice(2);
  console.log(JSON.stringify(await (mode === 'produce' ? packageProduce(args) : mode === 'validate' ? packageValidate(args) : packageSmoke(process.argv.slice(2)))));
}
