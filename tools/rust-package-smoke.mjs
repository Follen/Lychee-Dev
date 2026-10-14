// Isolated, unpublished candidate assembly and optional npm smoke. No publishing.
import {mkdirSync, readdirSync, readFileSync, writeFileSync, copyFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {join, resolve, dirname} from 'node:path';
import {fileURLToPath} from 'node:url';
import {stageLuac} from './luac.mjs';
import {stageLuaLS} from './luals.mjs';
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
    if (!['--artifact', '--profile', '--home', '--pin', '--luals-archive', '--assembly-only'].includes(arg)) throw new Error(`candidate.unknown_option: ${arg}`);
    if (seen.has(arg)) throw new Error(`candidate.duplicate_option: ${arg}`);
    seen.add(arg);
    if (arg === '--assembly-only') values[arg] = true;
    else { const value = argv[++i]; if (!value || value.startsWith('--')) throw new Error(`candidate.option_value_missing: ${arg}`); values[arg] = value; }
  }
  if (!positional[0] || !positional[1] || positional.length > 5) throw new Error('usage: node tools/rust-package-smoke.mjs <new-directory> <verified-luac-build> [fixed-home fixed-pin [verified-LuaLS-zip]] [--artifact <artifact.json>] [--profile debug|release] [--assembly-only]');
  for (const [index, flag] of [[2, '--home'], [3, '--pin'], [4, '--luals-archive']]) if (positional[index] && values[flag]) throw new Error(`candidate.duplicate_option: ${flag}`);
  const homeArg = positional[2] ?? values['--home'], pinArg = positional[3] ?? values['--pin'];
  if (Boolean(homeArg) !== Boolean(pinArg)) throw new Error('candidate.home_pin_pair_required');
  const profile = values['--profile'] ?? 'debug';
  if (!['debug', 'release'].includes(profile)) throw new Error('candidate.invalid_profile');
  if (values['--assembly-only'] && homeArg) throw new Error('candidate.assembly_only_does_not_validate');
  return {outArg: positional[0], luacArg: positional[1], homeArg, pinArg, lualsArg: positional[4] ?? values['--luals-archive'], artifactPath: values['--artifact'], profile, assemblyOnly: values['--assembly-only'] === true};
}

export function assertArtifactSource(artifact, currentSourceDigest, profile) {
  if (!/^[a-f0-9]{64}$/.test(currentSourceDigest) || artifact.inputHashes?.sourceTree !== currentSourceDigest) throw new Error('candidate.artifact_source_mismatch');
  if ((profile === 'release' && !['release', 'symbols'].includes(artifact.profile)) || (profile === 'debug' && !['dev', 'debug'].includes(artifact.profile)) || !['debug', 'release'].includes(profile)) throw new Error('candidate.artifact_profile_mismatch');
}

export async function packageSmoke(argv, root = process.cwd()) {
  const {outArg, luacArg, homeArg, pinArg, lualsArg, artifactPath, profile, assemblyOnly} = parsePackageOptions(argv);
  if (process.platform !== 'win32' || process.arch !== 'x64') throw new Error('candidate.windows_amd64_required');
  root = resolve(root);
  const out = contained(root, resolve(outArg)), luacRoot = resolve(luacArg);
  const job = beginScratch(root, 'rust-package-smoke', {command: [process.execPath, fileURLToPath(import.meta.url), ...argv]}), logs = [];
  let state = 'failed', handlesReaped = true, commandSequence = 0;
  function run(command, args, cwd = root) {
    const index = commandSequence++, metadata = join(job.evidence, `command-${index}.json`), stdout = join(job.evidence, `stdout-${index}.bin`), stderr = join(job.evidence, `stderr-${index}.bin`);
    handlesReaped = false;
    const result = ownedSpawnSync(command, args, {cwd, env: {...process.env, npm_config_cache: join(job.directory, 'npm-cache')}, evidence: job.evidence, timeout: 300000});
    handlesReaped = result.handlesReaped === true; logs.push(...result.logs);
    writeJson(metadata, {command, args, cwd, exit: result.status, signal: result.signal, error: result.error?.message, handlesReaped});
    writeFileSync(stdout, result.stdout ?? Buffer.alloc(0)); writeFileSync(stderr, result.stderr ?? Buffer.alloc(0)); logs.push(metadata, stdout, stderr);
    if (!handlesReaped || result.error || result.status !== 0) throw new Error(`candidate.failed: ${command}: ${result.error?.message ?? result.status}`);
    return result.stdout.toString();
  }
  const npm = (args, cwd) => run(process.execPath, [process.env.npm_execpath || join(dirname(process.execPath), 'node_modules/npm/bin/npm-cli.js'), ...args], cwd);
  try {
    mkdirSync(out); const pkgRoot = join(out, 'package'); mkdirSync(pkgRoot);
    // Explicit sealed artifacts are reusable only against their exact current
    // build inputs. Never relabel a historical EXE with today's tree/luac pin.
    const recordPath = artifactPath ? resolve(artifactPath) : join(out, 'native-artifact/artifact.json');
    const artifact = artifactPath ? verifyArtifact(root, recordPath) : buildRust(root, join(out, 'native-artifact'), {profile: profile === 'debug' ? 'dev' : 'release', oneShot: true, owner: 'rust-package-smoke'});
    const currentSourceDigest = sourceDigest(root);
    assertArtifactSource(artifact, currentSourceDigest, profile);
    const artifactRecordSHA256 = hash(readFileSync(recordPath));
    const pkg = JSON.parse(readFileSync(join(root, 'packages/npm/lycheedev/package.json')));
    pkg.bin = {lycheedev: 'bin/lycheedev.mjs'};
    pkg.private = true; // This rehearsal is never a publishable package.
    writeJson(join(pkgRoot, 'package.json'), pkg);
    for (const name of ['README.md', 'LICENSE', 'THIRD_PARTY_NOTICES']) copyFileSync(join(root, 'packages/npm/lycheedev', name), join(pkgRoot, name));
    const closureRoot = join(out, 'license-closure');
    run(process.execPath, [join(root, 'tools/rust-dependencies.mjs'), closureRoot]);
    const closure = JSON.parse(readFileSync(join(closureRoot, 'dependency-closure.json')));
    if (closure.cargoLockSHA256 !== hash(readFileSync(join(root, 'Cargo.lock')))) throw new Error('candidate.stale_dependency_closure');
    writeFileSync(join(pkgRoot, 'THIRD_PARTY_NOTICES'), readFileSync(join(pkgRoot, 'THIRD_PARTY_NOTICES'), 'utf8') + '\n\nRust dependency closure\n\n' + readFileSync(join(closureRoot, 'THIRD_PARTY_NOTICES'), 'utf8') + '\n\nLua 5.1.5 compiler/runtime\n\n' + readFileSync(join(luacRoot, 'LICENSE'), 'utf8'));
    mkdirSync(join(pkgRoot, 'bin')); copyFileSync(join(root, 'packages/npm/lycheedev/bin/lycheedev-rust.mjs'), join(pkgRoot, 'bin/lycheedev.mjs'));
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
    copyTree(join(root, 'addon'), 'addon'); copyTree(join(root, 'skills/lycheedev'), 'skill'); stageLuac(pkgRoot, resources, luacRoot);
    if (lualsArg) {
      await stageLuaLS(pkgRoot, resources, {archivePath: resolve(lualsArg)});
      const licenses = resources.filter(resource => resource.path.startsWith('tool/luals/') && /(^|\/)(license|copying|notice)([-_.]|$)/i.test(resource.path));
      writeFileSync(join(pkgRoot, 'THIRD_PARTY_NOTICES'), readFileSync(join(pkgRoot, 'THIRD_PARTY_NOTICES'), 'utf8') + '\n\nPinned legacy LuaLS runtime\n\n' + licenses.map(resource => `${resource.path}\n${readFileSync(join(pkgRoot, 'payload', resource.path), 'utf8')}`).join('\n\n'));
    }
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
      npmSmoke: 'not_run', validation: {state: 'not_run'}, semanticRuntime: {engine: 'native-common-source', luac: 'pinned_inventory', legacyLuaLS: lualsArg ? 'included' : 'not_included', publicValidation: 'not_run'},
      limitations: [`${profile} unpublished candidate only`, 'full fixed Go 3.2.0 business parity and semantic/session acceptance remain unverified', 'official corresponding-source rebuild and publication gates not run'],
    };
    if (!assemblyOnly) {
      const packed = JSON.parse(npm(['pack', '--ignore-scripts', '--json', '--pack-destination', out], pkgRoot))[0];
      const installation = join(out, 'installed'); mkdirSync(installation);
      npm(['install', '--prefix', installation, '--ignore-scripts', '--no-audit', '--no-fund', '--offline', join(out, packed.filename)], out);
      const installed = join(installation, 'node_modules/lycheedev');
      const version = JSON.parse(run(process.execPath, [join(installed, 'bin/lycheedev.mjs'), 'version', '--format', 'json'], out));
      if (version.result.platform !== 'windows/amd64' || !version.result.rust) throw new Error('candidate.not_rust');
      const payload = join(installed, 'payload/tool/luac'); run(join(payload, 'luac.exe'), ['-v'], out);
      writeFileSync(join(out, 'valid.lua'), 'local value = 1\nreturn value\n'); run(join(payload, 'luac.exe'), ['-p', join(out, 'valid.lua')], out);
      if (homeArg && pinArg) {
        const addon = join(out, 'test-addon'); mkdirSync(addon);
        writeFileSync(join(addon, 'Test.toc'), '## Interface: 120100\nGood.lua\n'); writeFileSync(join(addon, 'Good.lua'), 'local value = 1\nreturn value\n');
        const response = JSON.parse(run(process.execPath, [join(installed, 'bin/lycheedev.mjs'), 'source', 'validate', '--home', resolve(homeArg), '--snapshot', pinArg, '--path', addon, '--toc', 'Test.toc', '--format', 'json'], out));
        writeJson(join(out, 'source-validate.json'), response);
        if (!response.ok || !response.result.valid || !response.result.syntaxCompilation?.passed || response.result.syntaxCompilation.compilerSHA256 !== report.compilerSHA256) throw new Error('candidate.source_syntax_validation_failed');
        report.validation = {state: 'passed', compilerSHA256: response.result.syntaxCompilation.compilerSHA256, checkedLua: response.result.syntaxCompilation.checkedLua, captures: response.captures.map(c => c.id)};
      }
      Object.assign(report, {state: 'partial_passed', npmSmoke: 'passed', ignoreScripts: true, offlineInstall: true, installedRoot: installed, runtimeDependencies: pkg.dependencies || {}, platform: version.result.platform, tgz: join(out, packed.filename), tgzSHA256: hash(readFileSync(join(out, packed.filename))), version});
    }
    // Staging and optional smoke must not change any compiled input or sealed EXE.
    assertArtifactSource(verifyArtifact(root, recordPath), sourceDigest(root), profile);
    if (hash(readFileSync(recordPath)) !== artifactRecordSHA256 || hash(readFileSync(join(pkgRoot, 'native/windows-amd64/lycheedev.exe'))) !== artifact.binarySHA256 || hash(readFileSync(join(pkgRoot, 'release.json'))) !== report.releaseManifestSHA256) throw new Error('candidate.identity_changed_during_assembly');
    for (const resource of resources) {
      const bytes = readFileSync(join(pkgRoot, 'payload', resource.path));
      if (bytes.length !== resource.bytes || hash(bytes) !== resource.sha256) throw new Error(`candidate.payload_changed: ${resource.path}`);
    }
    writeJson(join(out, 'report.json'), report); logs.push(join(out, 'report.json')); state = 'succeeded';
    return report;
  } catch (error) {
    if (error.buildEvidence) { const build = JSON.parse(readFileSync(error.buildEvidence)); if (!build.handlesReaped) handlesReaped = false; }
    const failure = join(job.evidence, 'failure.json'); writeJson(failure, {error: error.message, buildEvidence: error.buildEvidence}); logs.push(failure); throw error;
  } finally {
    job.seal({state: handlesReaped ? state : 'interrupted', handlesReaped, logs});
    if (handlesReaped) job.cleanup();
  }
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) console.log(JSON.stringify(await packageSmoke(process.argv.slice(2))));
