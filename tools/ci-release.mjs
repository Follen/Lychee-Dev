// CI release identity and release-ready assembly.
//
// `identity` freezes the version/tag/commit/channel identity of a tag run.
// `candidate` consumes the already-built release artifacts plus the results of
// the validate stage and seals the SAME npm tarball that the isolated install
// smoke consumed - it never compiles, re-packs or replaces a binary.
// `seal` re-verifies that sealed candidate against the tag identity.
//
// Publication is deliberately not implemented here: the current task has no
// publication authorization, and the old Go release pipeline is retired.
import { copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { dirname, join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { verifyArtifacts } from './ci-artifacts.mjs';

const repository = resolve(import.meta.dirname ?? fileURLToPath(new URL('.', import.meta.url)), '..');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const readJson = path => JSON.parse(readFileSync(path, 'utf8'));
const writeJson = (path, value) => writeFileSync(path, `${JSON.stringify(value, null, 2)}\n`, { flag: 'wx' });
const IDENTITY_SCHEMA = 'lycheedev.ci-release-identity.v1';
const CANDIDATE_SCHEMA = 'lycheedev.ci-release-candidate.v1';
const SEALED_SCHEMA = 'lycheedev.ci-release-sealed.v1';

function git(args) {
  const result = spawnSync('git', args, { cwd: repository, encoding: 'utf8', windowsHide: true, shell: false, timeout: 60000 });
  if (result.error || result.status !== 0) throw new Error(`ci-release.git_failed: ${result.error?.message ?? result.status}`);
  return result.stdout.trim();
}

function exactFiles() {
  const status = git(['status', '--porcelain', '--untracked-files=no']);
  if (status) throw new Error(`ci-release.worktree_not_clean: ${status.split(/\r?\n/).slice(0, 5).join('; ')}`);
  const commit = git(['rev-parse', 'HEAD']);
  if (!/^[a-f0-9]{40}$/.test(commit)) throw new Error(`ci-release.invalid_commit: ${commit}`);
  return commit;
}

function installedRust() {
  const result = spawnSync('rustc', ['-Vv'], { encoding: 'utf8', windowsHide: true, shell: false, timeout: 60000 });
  if (result.error || result.status !== 0) throw new Error(`ci-release.rustc_failed: ${result.error?.message ?? result.status}`);
  const value = result.stdout.replaceAll('\r\n', '\n');
  const release = /^release:\s*(\S+)$/m.exec(value)?.[1];
  const host = /^host:\s*(\S+)$/m.exec(value)?.[1];
  if (release !== readJson(join(repository, 'release/tools/rust.json')).rustc) throw new Error(`ci-release.rust_mismatch: ${release}`);
  if (host !== readJson(join(repository, 'release/tools/rust.json')).host) throw new Error(`ci-release.rust_host_mismatch: ${host}`);
  return { rustc: release, host };
}

export function releaseIdentity({ tag, output, ref, unverified = false } = {}) {
  if (!tag) throw new Error('ci-release.tag_required');
  if (!/^v(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)(?:-[0-9A-Za-z.-]+)?$/.test(tag)) throw new Error(`ci-release.invalid_tag: ${tag}`);
  const versionFile = readJson(join(repository, 'release/version.json'));
  const version = versionFile.version;
  if (versionFile.schema !== 'lycheedev.version.v1') throw new Error('ci-release.invalid_version_source');
  if (tag !== `v${version}`) throw new Error(`ci-release.tag_version_mismatch: ${tag} != v${version}`);
  const packageJson = readJson(join(repository, 'packages/npm/lycheedev/package.json'));
  if (packageJson.version !== version) throw new Error('ci-release.package_version_mismatch');
  const channel = readJson(join(repository, 'release/npm-channel.json'));
  if (channel.schema !== 'lycheedev.npm-channel.v1' || channel.version !== version) throw new Error('ci-release.npm_channel_version_mismatch');
  if (typeof channel.distTag !== 'string' || !channel.distTag) throw new Error('ci-release.npm_channel_dist_tag_missing');
  // Tag verification is mandatory for the release workflow; branch/rehearsal
  // runs derive the same identity but can never pass the tag-verified seal.
  if (!unverified) {
    if (!ref) throw new Error('ci-release.ref_required');
    if (ref !== `refs/tags/${tag}`) throw new Error(`ci-release.ref_tag_mismatch: ${ref} != refs/tags/${tag}`);
  }
  const commit = exactFiles();
  const rust = installedRust();
  const identity = {
    schema: IDENTITY_SCHEMA, version, tag, commit, distTag: channel.distTag, npmChannel: channel,
    rust, node: process.versions.node, platform: `${process.platform}/${process.arch}`, tagVerified: !unverified, publication: 'not_performed',
  };
  if (output) {
    const path = resolve(repository, output);
    if (existsSync(path)) throw new Error(`ci-release.identity_exists: ${output}`);
    mkdirSync(dirname(path), { recursive: true });
    writeJson(path, identity);
  }
  return identity;
}

function copyArtifact(source, destination, expectedSHA256, description) {
  if (!existsSync(source)) throw new Error(`ci-release.${description}_missing: ${source}`);
  const bytes = readFileSync(source);
  if (expectedSHA256 && hash(bytes) !== expectedSHA256) throw new Error(`ci-release.${description}_changed`);
  copyFileSync(source, destination);
  if (hash(readFileSync(destination)) !== hash(bytes)) throw new Error(`ci-release.${description}_copy_changed`);
  return { name: destination.split(/[\\/]/).pop(), bytes: bytes.length, sha256: hash(bytes) };
}

function reportEntries(options) {
  return (options.reports ?? []).map(entry => {
    const match = /^([A-Za-z0-9._-]+)=(.+)$/.exec(entry);
    if (!match) throw new Error(`ci-release.invalid_report: ${entry}`);
    const path = resolve(repository, match[2]);
    if (!existsSync(path)) throw new Error(`ci-release.validation_report_missing: ${match[2]}`);
    return { name: match[1], path };
  });
}

export function releaseCandidate(options = {}) {
  if (!options.identity) throw new Error('ci-release.identity_required');
  if (!options.buildManifest) throw new Error('ci-release.build_manifest_required');
  if (!options.testManifest) throw new Error('ci-release.test_manifest_required');
  if (!options.npmReport) throw new Error('ci-release.npm_report_required');
  if (!options.output) throw new Error('ci-release.output_required');
  const identity = readJson(resolve(repository, options.identity));
  if (identity.schema !== IDENTITY_SCHEMA) throw new Error(`ci-release.unknown_identity: ${identity.schema}`);
  if (identity.tag !== `v${identity.version}`) throw new Error('ci-release.identity_tag_mismatch');
  if (identity.commit !== git(['rev-parse', 'HEAD'])) throw new Error('ci-release.identity_commit_mismatch');
  const build = verifyArtifacts({ manifestPath: options.buildManifest, require: ['cli-release', 'luac', 'lua51', 'addon-zip', 'skill', 'npm-package'] });
  const tests = verifyArtifacts({ manifestPath: options.testManifest, require: ['test-programs'] });
  const buildManifest = readJson(resolve(repository, options.buildManifest));
  const testManifest = readJson(resolve(repository, options.testManifest));
  const npm = readJson(resolve(repository, options.npmReport));
  if (npm.schema !== 'lycheedev.rust-npm-smoke.v1') throw new Error(`ci-release.unknown_npm_report: ${npm.schema}`);
  if (npm.assemblyOnly !== false || npm.npmSmoke !== 'passed') throw new Error('ci-release.npm_smoke_not_run');
  if (!['partial_passed', 'passed'].includes(npm.state)) throw new Error(`ci-release.npm_state: ${npm.state}`);
  if (npm.profile !== 'release') throw new Error(`ci-release.npm_profile: ${npm.profile}`);
  if (npm.version !== identity.version) throw new Error('ci-release.npm_version_mismatch');
  if (npm.commit !== identity.commit) throw new Error('ci-release.npm_commit_mismatch');
  if (npm.sourceTreeSHA256 !== buildManifest.sourceTree) throw new Error('ci-release.npm_source_mismatch');
  if (npm.binarySHA256 !== buildManifest.components['cli-release'].binarySHA256) throw new Error('ci-release.npm_binary_mismatch');
  const assembly = readJson(join(repository, buildManifest.components['npm-package'].dir, 'report.json'));
  if (assembly.binarySHA256 !== npm.binarySHA256) throw new Error('ci-release.build_assembly_binary_mismatch');
  const tgz = resolve(repository, npm.tgz);
  if (!existsSync(tgz)) throw new Error(`ci-release.tgz_missing: ${npm.tgz}`);
  if (hash(readFileSync(tgz)) !== npm.tgzSHA256) throw new Error('ci-release.tgz_changed');
  const addonZip = resolve(repository, buildManifest.components['addon-zip'].zip);
  if (!existsSync(addonZip)) throw new Error('ci-release.addon_zip_missing');
  if (hash(readFileSync(addonZip)) !== buildManifest.components['addon-zip'].zipSHA256) throw new Error('ci-release.addon_zip_changed');
  const out = resolve(repository, options.output);
  if (existsSync(out)) throw new Error(`ci-release.output_exists: ${options.output}`);
  mkdirSync(out, { recursive: true });
  const tgzRecord = copyArtifact(tgz, join(out, `lycheedev-${identity.version}.tgz`), npm.tgzSHA256, 'npm_tgz');
  const addonRecord = copyArtifact(addonZip, join(out, `lycheedev-${identity.version}-addon.zip`), buildManifest.components['addon-zip'].zipSHA256, 'addon_zip');
  const reports = [];
  for (const report of reportEntries(options)) {
    const destination = join(out, 'reports', report.name);
    mkdirSync(dirname(destination), { recursive: true });
    copyFileSync(report.path, destination);
    const bytes = readFileSync(destination);
    reports.push({ name: report.name, bytes: bytes.length, sha256: hash(bytes) });
  }
  const manifest = {
    schema: CANDIDATE_SCHEMA, version: identity.version, tag: identity.tag, commit: identity.commit, distTag: identity.distTag,
    sourceTree: buildManifest.sourceTree, rust: identity.rust, node: identity.node, platform: identity.platform,
    artifacts: { npmTgz: tgzRecord, addonZip: addonRecord },
    components: {
      cli: { binarySHA256: buildManifest.components['cli-release'].binarySHA256, bytes: buildManifest.components['cli-release'].bytes },
      luac: { executableSHA256: buildManifest.components.luac.executableSHA256, version: buildManifest.components.luac.version },
      lua51: { executableSHA256: buildManifest.components.lua51.executableSHA256 },
      testPrograms: { programs: testManifest.components['test-programs'].programs, support: testManifest.components['test-programs'].support },
    },
    validation: { reports, npmSmoke: npm.state, installationKind: npm.installationKind, offlineInstall: npm.offlineInstall === true },
    buildManifest: { path: options.buildManifest, files: build.files, sourceTree: build.sourceTree },
    testManifest: { path: options.testManifest, files: tests.files, sourceTree: tests.sourceTree },
    publication: 'not_performed',
    limitations: ['release candidate only; no npm publication, no GitHub release, no registry read-back'],
  };
  writeJson(join(out, 'release-manifest.json'), manifest);
  writeSums(out, manifest);
  return { output: options.output, manifest: join(out, 'release-manifest.json'), artifacts: manifest.artifacts, reports: reports.map(report => report.name) };
}

function sumsPath(out) { return join(out, 'SHA256SUMS'); }

function writeSums(out, manifest) {
  const names = ['release-manifest.json', ...Object.values(manifest.artifacts ?? {}).map(record => record.name), ...(manifest.validation?.reports ?? []).map(report => `reports/${report.name}`)];
  const lines = names.map(name => `${hash(readFileSync(join(out, name)))}  ${name}`);
  writeFileSync(sumsPath(out), `${lines.join('\n')}\n`);
}

export function sealRelease({ candidate, identity, output } = {}) {
  if (!candidate) throw new Error('ci-release.candidate_required');
  if (!identity) throw new Error('ci-release.identity_required');
  if (!output) throw new Error('ci-release.output_required');
  const tag = readJson(resolve(repository, identity));
  if (tag.schema !== IDENTITY_SCHEMA || tag.tagVerified !== true) throw new Error('ci-release.identity_not_verified');
  const source = resolve(repository, candidate);
  const manifestPath = join(source, 'release-manifest.json');
  if (!existsSync(manifestPath)) throw new Error('ci-release.candidate_manifest_missing');
  const manifest = readJson(manifestPath);
  if (manifest.schema !== CANDIDATE_SCHEMA) throw new Error(`ci-release.unknown_candidate: ${manifest.schema}`);
  if (manifest.version !== tag.version || manifest.commit !== tag.commit || manifest.tag !== tag.tag) throw new Error('ci-release.candidate_identity_mismatch');
  if (manifest.publication !== 'not_performed') throw new Error('ci-release.candidate_publication_unexpected');
  const sums = new Map(readFileSync(sumsPath(source), 'utf8').split('\n').filter(Boolean).map(line => {
    const [digest, name] = line.split(/\s{2}/);
    return [name, digest];
  }));
  for (const [name, digest] of sums) {
    const path = join(source, name);
    if (!existsSync(path)) throw new Error(`ci-release.sealed_file_missing: ${name}`);
    if (hash(readFileSync(path)) !== digest) throw new Error(`ci-release.sealed_file_changed: ${name}`);
  }
  for (const name of ['release-manifest.json', ...Object.values(manifest.artifacts).map(record => record.name)]) {
    if (!sums.has(name)) throw new Error(`ci-release.sealed_file_unlisted: ${name}`);
  }
  const out = resolve(repository, output);
  if (existsSync(out)) throw new Error(`ci-release.output_exists: ${output}`);
  mkdirSync(out, { recursive: true });
  for (const name of sums.keys()) {
    const target = join(out, name);
    mkdirSync(dirname(target), { recursive: true });
    copyFileSync(join(source, name), target);
  }
  const sealed = {
    schema: SEALED_SCHEMA, version: manifest.version, tag: manifest.tag, commit: manifest.commit, distTag: manifest.distTag,
    sourceTree: manifest.sourceTree, artifacts: manifest.artifacts, validation: manifest.validation, rust: manifest.rust,
    identity: tag, publication: 'not_performed',
  };
  writeJson(join(out, 'release-sealed.json'), sealed);
  const lines = [...sums.entries()].map(([name, digest]) => `${digest}  ${name}`);
  lines.push(`${hash(readFileSync(join(out, 'release-sealed.json')))}  release-sealed.json`);
  writeFileSync(join(out, 'SHA256SUMS'), `${lines.join('\n')}\n`);
  return { output: output, sealed: join(out, 'release-sealed.json'), version: sealed.version, commit: sealed.commit, artifacts: sealed.artifacts };
}

function parse(argv) {
  const options = { reports: [] };
  for (let index = 0; index < argv.length; index++) {
    const arg = argv[index], value = argv[index + 1];
    if (arg === '--tag') { options.tag = value; index++; }
    else if (arg === '--ref') { options.ref = value; index++; }
    else if (arg === '--unverified') options.unverified = true;
    else if (arg === '--out') { options.output = value; index++; }
    else if (arg === '--identity') { options.identity = value; index++; }
    else if (arg === '--build-manifest') { options.buildManifest = value; index++; }
    else if (arg === '--test-manifest') { options.testManifest = value; index++; }
    else if (arg === '--npm-report') { options.npmReport = value; index++; }
    else if (arg === '--report') { options.reports.push(value); index++; }
    else if (arg === '--candidate') { options.candidate = value; index++; }
    else throw new Error(`ci-release.unknown_option: ${arg}`);
  }
  return options;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const [command, ...argv] = process.argv.slice(2);
    const options = parse(argv);
    if (command === 'identity') process.stdout.write(`${JSON.stringify(releaseIdentity(options), null, 2)}\n`);
    else if (command === 'candidate') process.stdout.write(`${JSON.stringify(releaseCandidate(options), null, 2)}\n`);
    else if (command === 'seal') process.stdout.write(`${JSON.stringify(sealRelease(options), null, 2)}\n`);
    else throw new Error('usage: node tools/ci-release.mjs identity --tag vX.Y.Z --out <identity.json> | candidate --identity <json> --build-manifest <json> --test-manifest <json> --npm-report <json> --out <new-directory> --report name=path | seal --candidate <dir> --identity <json> --out <new-directory>');
  } catch (error) { process.stderr.write(`${error.message}\n`); process.exitCode = 1; }
}
