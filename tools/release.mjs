// Release assembly, sealing and release-gate checks for the lycheedev package
// (docs/toolkit/release-2.0.1.md, regression REL-01..14 / PKG-01..08).
// Subcommands are driven by .github/workflows/toolkit-release.yml and
// toolkit-ci.yml. Nothing here publishes, tags, or mutates tracked sources.
//
//   assemble            windows-amd64 native build + payload + release.json +
//                       native archive + addon ZIP + ONE npm pack + whitelist
//                       audit
//   verify-cgo          pre-step proving capture/decode still work with
//                       CGO_ENABLED=0 before the release build claims it
//   seal                fold test report digests into the external sealed
//                       manifest + SHA256SUMS after verification
//   verify-sealed       recompute every sealed digest right before publish
//   release-identity    strict tag <-> version source binding (REL-01/05/11)
//   dist-tag            2.0.1-rc.N -> next, otherwise latest (REL-11)
//   registry-state      exists-matching / exists-conflicting / absent / unknown
//                       (REL-09: unknown is never treated as absent)
//   verify-platform-evidence   windows-amd64 run evidence completeness (REL-13)
//   platform-evidence / release-notes / check-assets
//                       small report helpers used by the workflows
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import {
  copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, renameSync,
  rmSync, statSync, writeFileSync,
} from 'node:fs';
import { dirname, join, resolve, sep } from 'node:path';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';
import { readTgz, setTgzModes } from './tgz.mjs';
import { readZip, writeZip } from './zip.mjs';
import { writeTarGz } from './tar.mjs';
import { synchronizeVersion } from './version.mjs';

export const repository = resolve(dirname(fileURLToPath(import.meta.url)), '..');
export const REPOSITORY_URL = 'https://github.com/Follen/Lychee-Dev';
// Owner decision: the 2.0 release ships Windows amd64 ONLY. The Go source stays
// portable, but nothing else is built, packaged or verified.
export const TARGETS = [
  { target: 'windows-amd64', goos: 'windows', goarch: 'amd64', binary: 'native/windows-amd64/lycheedev.exe' },
];
// Assembly, binary identity verification and the host `version` check run on the
// Windows release host, the only platform that can execute the shipped binary.
const HOST_TARGET = 'windows-amd64';

const HEX64 = /^[a-f0-9]{64}$/;
const HEX40 = /^[a-f0-9]{40}$/;

/** Release-version policy: `-dev` versions are development-only packages. */
export function policyFor(version) {
  const development = version.endsWith('-dev');
  return {
    development,
    // §5: the published package cannot be private; the development manifest
    // stays private until the owner's combined-work license decision lands.
    privateMustBeTrue: development,
    // Development versions are never published, so they have no dist-tag.
    npmDistTag: development ? null : distTagFor(version),
  };
}

/** REL-11: release candidates only enter `next`; everything else is `latest`. */
export function distTagFor(version) {
  if (/^\d+\.\d+\.\d+-rc\.\d+$/.test(version)) return 'next';
  if (/^\d+\.\d+\.\d+$/.test(version)) return 'latest';
  throw new Error(`release.invalid_version: ${version}`);
}

/** REL-01/11: a release ref is exactly `v<major>.<minor>.<patch>[-rc.<n>]`. */
export function parseTag(tag) {
  const match = /^v(\d+\.\d+\.\d+(?:-rc\.\d+)?)$/.exec(tag ?? '');
  if (!match) throw new Error(`release.invalid_tag: ${tag}`);
  return match[1];
}

function sha256(bytes) { return createHash('sha256').update(bytes).digest('hex'); }
function sha512hex(bytes) { return createHash('sha512').update(bytes).digest('hex'); }
function sha512sri(bytes) { return `sha512-${createHash('sha512').update(bytes).digest('base64')}`; }

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    encoding: 'utf8', shell: false, windowsHide: true, timeout: 900000,
    maxBuffer: 64 * 1024 * 1024, ...options,
  });
  if (result.error) throw result.error;
  return result;
}
function runOk(command, args, options = {}) {
  const result = run(command, args, options);
  if (result.status !== 0) {
    throw new Error(`release.command_failed: ${command} ${args.join(' ')}\n${result.stdout}\n${result.stderr}`);
  }
  return result.stdout;
}

function git(args) { return runOk('git', args, { cwd: repository }).trim(); }

function walkFiles(root, prefix = '') {
  const files = [];
  for (const entry of readdirSync(root, { withFileTypes: true }).sort((a, b) => (a.name < b.name ? -1 : 1))) {
    const name = prefix ? `${prefix}/${entry.name}` : entry.name;
    if (entry.isSymbolicLink()) throw new Error(`release.linked_payload: ${name}`);
    if (entry.isDirectory()) files.push(...walkFiles(join(root, entry.name), name));
    else if (entry.isFile()) files.push({ name, path: join(root, entry.name) });
    else throw new Error(`release.non_file_payload: ${name}`);
  }
  return files;
}

const PAYLOAD_BAN = /(^|\/)(?:vendor|node_modules|tests?|\.git)(\/|$)|\.py$|(^|\/)(?:wowdoc|wowdata|wowdump|automation\.py)/i;
const REQUIRED_RESOURCES = [
  'skill/SKILL.md',
  'addon/Lychee Dev_Mainline.toc',
  'addon/Lychee Dev_Mists.toc',
  'addon/Lychee Dev_Wrath.toc',
  'addon/Lychee Dev_Forever.toc',
];
const EMPTY_PROBE_QUEUE = 'local _, ns = ...\nns.ProbeDefinitions = {schema="lycheedev.queue.v1",entries={\n}}\n';

/** REL-01 identity: commit + worktree state from VCS, never from the caller's cwd. */
export function identity({ allowDirty = false, requireTagRef = false, tag } = {}) {
  const commit = git(['rev-parse', 'HEAD']).toLowerCase();
  if (!HEX40.test(commit)) throw new Error('release.unknown_identity: commit');
  const dirtyStatus = git(['status', '--porcelain']);
  const dirty = dirtyStatus.length > 0;
  const claimedTag = tag ?? null;
  let tagVerified = false;
  if (requireTagRef) {
    if (!claimedTag) throw new Error('release.tag_required: --tag is mandatory for release builds');
    const pointed = git(['rev-parse', `${claimedTag}^{commit}`]).toLowerCase();
    if (pointed !== commit) throw new Error(`release.tag_commit_mismatch: ${claimedTag} -> ${pointed}, HEAD ${commit}`);
    tagVerified = true;
  } else if (claimedTag) {
    try {
      tagVerified = git(['rev-parse', `${claimedTag}^{commit}`]).toLowerCase() === commit;
    } catch { tagVerified = false; }
  }
  if (dirty && !allowDirty) {
    throw new Error(`release.identity_dirty: uncommitted worktree state never enters a release (REL-01/REL-05); use --allow-dirty for development rehearsals only\n${dirtyStatus.split('\n').slice(0, 20).join('\n')}`);
  }
  return { commit, dirty, tag: claimedTag, tagVerified, developmentEscape: dirty && allowDirty };
}

function licenseGate(packageRoot) {
  const license = join(packageRoot, 'LICENSE');
  if (!existsSync(license) || statSync(license).size === 0) {
    throw new Error(`license decision pending: ${license} is absent; the combined-work license decision belongs to the project owner (see THIRD_PARTY_NOTICES.md owner-decision section), and packaging refuses to proceed until the owner supplies LICENSE`);
  }
  for (const name of ['README.md', 'THIRD_PARTY_NOTICES', 'package.json', 'bin/lycheedev.mjs']) {
    if (!existsSync(join(packageRoot, name))) throw new Error(`release.missing_package_file: ${name}`);
  }
}

function stagePayload(stage, resources) {
  for (const [source, prefix] of [[join(repository, 'addon'), 'addon'], [join(repository, 'skills/lycheedev'), 'skill']]) {
    for (const file of walkFiles(source)) {
      const name = `${prefix}/${file.name}`;
      if (PAYLOAD_BAN.test(name)) throw new Error(`release.unexpected_payload: ${name}`);
      const content = readFileSync(file.path);
      const destination = join(stage, 'payload', prefix, ...file.name.split('/'));
      mkdirSync(dirname(destination), { recursive: true });
      writeFileSync(destination, content);
      resources.push({ path: name, bytes: content.length, sha256: sha256(content) });
    }
  }
  resources.sort((a, b) => (a.path < b.path ? -1 : 1));
  for (const required of REQUIRED_RESOURCES) {
    if (!resources.some(resource => resource.path === required)) throw new Error(`release.missing_resource: ${required}`);
  }
}

function buildBinaries(stage, { cgo }) {
  const env = { ...process.env };
  delete env.GOFLAGS;
  if (cgo === 'zero') env.CGO_ENABLED = '0'; else delete env.CGO_ENABLED;
  const built = {};
  for (const entry of TARGETS) {
    const output = join(stage, 'native', entry.target, entry.target.startsWith('windows') ? 'lycheedev.exe' : 'lycheedev');
    mkdirSync(dirname(output), { recursive: true });
    runOk('go', ['build', '-trimpath', '-buildvcs=true', '-o', output, './cmd/lycheedev'], {
      cwd: repository,
      env: { ...env, GOOS: entry.goos, GOARCH: entry.goarch },
    });
    const bytes = readFileSync(output);
    built[entry.target] = { path: entry.binary, bytes: bytes.length, sha256: sha256(bytes) };
  }
  return built;
}

/** Parse `go version -m` output. Build lines carry a leading tab ("\tbuild\tvcs=..."). */
export function parseVcsIdentity(output) {
  return {
    revision: /^\s*build\tvcs\.revision=([a-f0-9]{40})$/m.exec(output)?.[1],
    modified: /^\s*build\tvcs\.modified=(true|false)$/m.exec(output)?.[1],
  };
}

/** VCS stamps + host `version` output must agree with the fixed identity (REL-01/05). */
export function verifyBinaryIdentity(stage, { commit, dirty, version }) {
  const report = {};
  for (const entry of TARGETS) {
    const output = runOk('go', ['version', '-m', join(stage, ...entry.binary.split('/'))]);
    const { revision, modified } = parseVcsIdentity(output);
    if (!revision) throw new Error(`release.unknown_identity: ${entry.target} binary carries no VCS stamp (REL-01)`);
    if (revision !== commit) throw new Error(`release.binary_commit_mismatch: ${entry.target} ${revision} != ${commit} (REL-05)`);
    if (modified !== String(dirty)) throw new Error(`release.binary_dirty_mismatch: ${entry.target} vcs.modified=${modified}, worktree=${dirty}`);
    report[entry.target] = { commit: revision, workspaceDirty: modified === 'true' };
  }
  const host = join(stage, ...TARGETS.find(entry => entry.target === HOST_TARGET).binary.split('/'));
  const identityOutput = JSON.parse(runOk(host, ['version', '--format=json']));
  if (!identityOutput.ok) throw new Error('release.version_command_failed');
  const observed = identityOutput.result;
  if (observed.version !== version) throw new Error(`release.version_mismatch: binary ${observed.version} != ${version}`);
  if (observed.commit !== commit) throw new Error(`release.version_commit_mismatch: ${observed.commit} != ${commit} (REL-05)`);
  if (observed.workspaceDirty !== dirty) throw new Error('release.version_dirty_mismatch');
  report.version = observed;
  return report;
}

/**
 * Licensing closure (implementation-status.md): `git archive` of the exact
 * commit + `go mod vendor` output + go.mod/go.sum + notices, provably buildable
 * offline, hashed into SHA256SUMS and the external sealed manifest.
 */
export function buildCorrespondingSource({
  commit, version, tag, outDirectory, cgo = 'zero',
  buildPackage = './cmd/lycheedev', requiredPaths,
}) {
  return buildCorrespondingSourceArchive(...arguments);
}

async function buildCorrespondingSourceArchive({
  commit, version, tag, outDirectory, cgo = 'zero',
  buildPackage = './cmd/lycheedev', requiredPaths,
}) {
  const archiveName = `lycheedev-${version}-corresponding-source.tar.gz`;
  const prefix = `lycheedev-${version}-corresponding-source/`;
  const required = requiredPaths ?? [
    'go.mod', 'go.sum', 'cmd/lycheedev/main.go', 'THIRD_PARTY_NOTICES.md',
    'packages/npm/lycheedev/THIRD_PARTY_NOTICES', 'addon/Lychee Dev_Mainline.toc',
    'skills/lycheedev/SKILL.md',
  ];
  const scratch = mkdtempSync(join(tmpdir(), 'lycheedev-source-'));
  try {
    const tarPath = join(scratch, 'source.tar');
    runOk('git', ['archive', '--format=tar', '-o', tarPath, commit], { cwd: repository });
    const tree = join(scratch, 'tree');
    mkdirSync(tree);
    // Extract with relative paths from the scratch directory: GNU tar (Git Bash)
    // misparses "C:\" drive paths as remote hosts / escaped arguments, while
    // bsdtar differs again; relative names sidestep every variant.
    runOk('tar', ['-xf', 'source.tar', '-C', 'tree'], { cwd: scratch });
    for (const name of required) {
      if (!existsSync(join(tree, ...name.split('/')))) {
        throw new Error(`release.source_archive_incomplete: ${name} is not in git archive ${commit}; commit the tree before releasing (corresponding source must build offline)`);
      }
    }
    runOk('go', ['mod', 'vendor'], { cwd: tree });
    if (!existsSync(join(tree, 'vendor', 'modules.txt'))) throw new Error('release.source_archive_incomplete: vendor/modules.txt');
    // The offline build must run the exact toolchain `go` resolves to (auto may
    // select a GOTOOLCHAIN module newer than the base install), because
    // GOSUMDB=off cannot re-authenticate a toolchain module and
    // GOTOOLCHAIN=local would silently fall back to the older base compiler.
    const goBinary = join(runOk('go', ['env', 'GOROOT']).trim(), 'bin', `go${process.platform === 'win32' ? '.exe' : ''}`);
    const buildEnv = { ...process.env, GOPROXY: 'off', GOSUMDB: 'off', GOFLAGS: '-mod=vendor', GOTOOLCHAIN: 'local' };
    if (cgo === 'zero') buildEnv.CGO_ENABLED = '0'; else delete buildEnv.CGO_ENABLED;
    const archiveBuild = join(scratch, 'build-from-archive');
    runOk(goBinary, ['build', '-trimpath', '-buildvcs=false', '-o', archiveBuild, buildPackage], { cwd: tree, env: buildEnv });
    // Rebuild-compare: the vendored archive must reproduce the checkout's own
    // un-stamped binary byte for byte (VCS stamps are compared separately).
    // Both sides build identically vendored: `-mod=vendor` embeds dependency
    // info WITHOUT h1 checksums, so a vendor build and a module-cache build
    // can never be byte-equal.
    // The compare side exports the INDEX with line-ending filters disabled:
    // on autocrlf checkouts the working tree carries CRLF while `git archive`
    // always emits blob bytes (LF), and source bytes feed the embedded build
    // ID. Worktree-vs-index drift stays the identity() clean-tree gate's job.
    const indexTree = join(scratch, 'index-tree');
    mkdirSync(indexTree);
    runOk('git', ['-c', 'core.autocrlf=false', 'checkout-index', '-a', '-f', `--prefix=${indexTree}/`], { cwd: repository });
    const indexBuild = join(scratch, 'build-from-index');
    runOk(goBinary, ['mod', 'vendor'], { cwd: indexTree });
    runOk(goBinary, ['build', '-trimpath', '-buildvcs=false', '-o', indexBuild, buildPackage], { cwd: indexTree, env: buildEnv });
    const rebuildVerified = sha256(readFileSync(archiveBuild)) === sha256(readFileSync(indexBuild));
    if (!rebuildVerified) throw new Error(`release.source_rebuild_mismatch: archive sha256 ${sha256(readFileSync(archiveBuild))} != index-export sha256 ${sha256(readFileSync(indexBuild))}`);

    const entries = walkFiles(tree).map(file => ({ name: `${prefix}${file.name}`, bytes: readFileSync(file.path), mode: 0o644 }));
    const target = join(outDirectory, archiveName);
    await writeTarGz(entries, target);
    const bytes = readFileSync(target);
    return {
      correspondingSource: {
        repository: REPOSITORY_URL, commit, tag, archive: archiveName, sha256: sha256(bytes),
      },
      artifact: { name: archiveName, bytes: bytes.length, sha256: sha256(bytes), sha512: sha512hex(bytes) },
      rebuildVerified,
      entryCount: entries.length,
    };
  } finally {
    if (process.env.LYCHEEDEV_KEEP_SOURCE_SCRATCH === '1') {
      process.stderr.write(`release: source scratch kept at ${scratch}\n`);
    } else {
      rmSync(scratch, { recursive: true, force: true });
    }
  }
}

function releaseJson({ version, commit, binaries, resources, correspondingSource }) {
  return { schema: 'lycheedev.release.v1', version, commit, binaries, resources, correspondingSource };
}

function writeJson(path, value) { writeFileSync(path, `${JSON.stringify(value, null, 2)}\n`); }

async function assembleCommand(argv) {
  const options = parseOptions(argv);
  const out = resolve(requiredOption(options, '--out'));
  const npmCli = resolve(requiredOption(options, '--npm-cli'));
  const cgo = requiredOption(options, '--cgo');
  if (!['zero', 'default'].includes(cgo)) throw new Error('release.invalid_cgo: use zero|default');
  const packageRoot = join(repository, 'packages', 'npm', 'lycheedev');
  const { version } = synchronizeVersion(repository, false); // REL-01
  const policy = policyFor(version);
  const wantTag = options['--tag'] ?? `v${version}`;
  const id = identity({ allowDirty: options['--allow-dirty'] === true, requireTagRef: options['--require-tag-ref'] === true, tag: wantTag });
  if (options['--require-tag-ref'] === true && wantTag !== `v${version}`) {
    throw new Error(`release.tag_version_mismatch: ${wantTag} != v${version} (REL-01)`);
  }
  licenseGate(packageRoot); // forces the owner's combined-work license decision
  mkdirSync(out, { recursive: true });
  const stage = join(out, 'npm-stage');
  rmSync(stage, { recursive: true, force: true });
  mkdirSync(join(stage, 'bin'), { recursive: true });

  const versionRecord = JSON.parse(readFileSync(join(repository, 'release/version.json'), 'utf8'));
  if (versionRecord.version !== version) throw new Error('release.version_drift');

  for (const name of ['package.json', 'README.md', 'LICENSE', 'THIRD_PARTY_NOTICES']) {
    copyFileSync(join(packageRoot, name), join(stage, name));
  }
  copyFileSync(join(packageRoot, 'bin/lycheedev.mjs'), join(stage, 'bin/lycheedev.mjs'));

  const resources = [];
  stagePayload(stage, resources);
  const binaries = buildBinaries(stage, { cgo });
  const binaryIdentity = verifyBinaryIdentity(stage, { commit: id.commit, dirty: id.dirty, version });
  const source = await buildCorrespondingSource({
    commit: id.commit, version, tag: wantTag, outDirectory: out, cgo,
  });
  writeJson(join(stage, 'release.json'), releaseJson({
    version, commit: id.commit, binaries, resources, correspondingSource: source.correspondingSource,
  }));

  // Native archive (windows-amd64 only): single-platform release.json
  // declarations are allowed there; binary/addon/skill bytes must equal the npm
  // package's.
  const artifacts = { [source.artifact.name]: source.artifact };
  for (const entry of TARGETS) {
    const record = releaseJson({
      version, commit: id.commit, binaries: { [entry.target]: binaries[entry.target] },
      resources, correspondingSource: source.correspondingSource,
    });
    const root = `lycheedev-${version}-${entry.target}`;
    const files = [
      { name: `${root}/${entry.binary}`, bytes: readFileSync(join(stage, ...entry.binary.split('/'))), mode: entry.target.startsWith('windows') ? 0o644 : 0o755 },
      { name: `${root}/payload`, directory: true },
      ...resources.map(resource => ({
        name: `${root}/payload/${resource.path}`,
        bytes: readFileSync(join(stage, 'payload', ...resource.path.split('/'))),
        mode: 0o644,
      })),
      { name: `${root}/release.json`, bytes: Buffer.from(`${JSON.stringify(record, null, 2)}\n`), mode: 0o644 },
      ...['README.md', 'LICENSE', 'THIRD_PARTY_NOTICES'].map(name => ({
        name: `${root}/${name}`, bytes: readFileSync(join(stage, name)), mode: 0o644,
      })),
    ].filter(file => !file.directory);
    const zipName = `lycheedev-${version}-${entry.target}.zip`;
    await writeZip(files, join(out, zipName));
    const zipBytes = readFileSync(join(out, zipName));
    artifacts[zipName] = { name: zipName, bytes: zipBytes.length, sha256: sha256(zipBytes), sha512: sha512hex(zipBytes) };
  }

  // Addon release ZIP: top-level "Lychee Dev/", TOC loads + Media (PKG-04/05).
  const { buildAddonZip, verifyAddonZip } = await import('./addon-package.mjs');
  const addonName = `lycheedev-${version}-addon.zip`;
  const addonBuild = await buildAddonZip({ repoRoot: repository, outPath: join(out, addonName) });
  const addonVerify = verifyAddonZip(readFileSync(join(out, addonName)), { repoRoot: repository });
  if (!addonVerify.ok) throw new Error(`release.addon_zip_invalid: ${addonVerify.violations.join('; ')}`);
  artifacts[addonName] = { name: addonName, bytes: addonBuild.bytes, sha256: addonBuild.sha256, sha512: sha512hex(readFileSync(join(out, addonName))) };
  // The addon ZIP and the npm payload must agree byte for byte (§6).
  const zipEntries = readZip(readFileSync(join(out, addonName)));
  for (const [name, bytes] of zipEntries) {
    const resource = name.replace(/^Lychee Dev\//, '');
    const payload = resources.find(candidate => candidate.path === `addon/${resource}`);
    if (!payload) throw new Error(`release.zip_payload_mismatch: ${name} is not in payload/addon`);
    const staged = readFileSync(join(stage, 'payload', 'addon', ...resource.split('/')));
    if (sha256(staged) !== payload.sha256 || !staged.equals(bytes)) throw new Error(`release.zip_payload_mismatch: ${name} bytes differ from payload/addon`);
  }

  // npm pack exactly once; the sealed tgz is the only publish input (§6).
  const pack = run(process.execPath, [npmCli, 'pack', '--json', '--pack-destination', out,
    '--userconfig', emptyNpmrc(out), '--cache', join(out, '.npm-cache'), '--offline', '--ignore-scripts', '--no-audit', '--no-fund'],
  { cwd: stage });
  if (pack.status !== 0) throw new Error(`release.npm_pack_failed\n${pack.stdout}\n${pack.stderr}`);
  const packed = JSON.parse(pack.stdout)[0];
  const tgzName = `lycheedev-${version}.tgz`;
  renameSync(join(out, packed.filename), join(out, tgzName));
  // npm pack on Windows emits mode 0644 for every entry; restore the POSIX exec
  // bit the launcher needs, in the same tarball (never re-packed).
  const normalized = setTgzModes(readFileSync(join(out, tgzName)), name => (
    /^package\/bin\/lycheedev\.mjs$/.test(name) ? 0o755 : undefined
  ));
  writeFileSync(join(out, tgzName), normalized);
  const tgzBytes = readFileSync(join(out, tgzName));
  const audit = auditTgz(readTgz(tgzBytes), { version, expectedCommit: id.commit, sourceRoot: packageRoot });
  if (audit.violations.length) throw new Error(`release.tgz_audit_failed: ${audit.violations.join('; ')}`);
  artifacts[tgzName] = { name: tgzName, bytes: tgzBytes.length, sha256: sha256(tgzBytes), sha512: sha512hex(tgzBytes), integrity: sha512sri(tgzBytes) };

  const npmVersion = runOk(process.execPath, [npmCli, '--version']).trim();
  const goVersion = runOk('go', ['version']).trim();
  const manifest = {
    schema: 'lycheedev.seal.v1',
    version,
    commit: id.commit,
    tag: wantTag,
    tagVerified: id.tagVerified,
    identity: { workspaceDirty: id.dirty, verified: !id.dirty && id.tagVerified },
    toolchain: {
      go: goVersion, node: process.version, npm: npmVersion,
      cgo: cgo === 'zero' ? 'CGO_ENABLED=0' : 'toolchain default (CGO_ENABLED=0 not claimed)',
      targets: TARGETS.map(entry => entry.target),
    },
    binaries,
    resources,
    correspondingSource: source.correspondingSource,
    correspondingSourceRebuildVerified: source.rebuildVerified,
    binaryIdentity,
    npm: { name: 'lycheedev', tarball: tgzName, sha256: artifacts[tgzName].sha256, sha512: artifacts[tgzName].sha512, integrity: artifacts[tgzName].integrity, packed: { compressedBytes: packed.size, unpackedBytes: packed.unpackedSize, entryCount: packed.entryCount } },
    artifacts,
    testReports: {},
    deviations: [
      ...(cgo === 'zero' ? [] : ['CGO_ENABLED=0 not claimed: capture/decode pre-step unavailable (verify-cgo)']),
      ...(id.developmentEscape ? ['development escape: dirty worktree accepted; NOT release-eligible'] : []),
      ...(id.tagVerified ? [] : [`git tag ${wantTag} not verified against the commit in this assembly`]),
      ...(policy.development ? [`development version ${version}: npm private=true, not publishable`] : []),
      ...(source.rebuildVerified ? [] : ['corresponding-source rebuild comparison not verified']),
    ],
  };
  mkdirSync(join(out, 'sealed'), { recursive: true });
  writeJson(join(out, 'sealed', 'release-manifest.json'), manifest);
  writeSha256Sums(out, manifest);
  return { out, version, commit: id.commit, tag: wantTag, tgz: tgzName, artifacts: Object.keys(artifacts), audit };
}

function emptyNpmrc(out) {
  const path = join(out, 'empty.npmrc');
  writeFileSync(path, '');
  return path;
}

function writeSha256Sums(out, manifest) {
  const lines = [];
  const sealed = join(out, 'sealed');
  for (const name of Object.keys(manifest.artifacts).sort()) {
    lines.push(`${manifest.artifacts[name].sha256}  ${name}`);
  }
  const manifestBytes = readFileSync(join(sealed, 'release-manifest.json'));
  lines.push(`${sha256(manifestBytes)}  release-manifest.json`);
  for (const name of Object.keys(manifest.testReports).sort()) {
    lines.push(`${manifest.testReports[name].sha256}  ${name}`);
  }
  writeFileSync(join(sealed, 'SHA256SUMS'), `${lines.join('\n')}\n`);
}

function recomputeArtifacts(out, manifest) {
  const violations = [];
  for (const [name, record] of Object.entries(manifest.artifacts)) {
    const path = join(out, name);
    if (!existsSync(path)) { violations.push(`missing artifact ${name}`); continue; }
    const bytes = readFileSync(path);
    if (bytes.length !== record.bytes) violations.push(`size changed ${name}`);
    if (sha256(bytes) !== record.sha256) violations.push(`sha256 changed ${name} (REL-06)`);
    if (sha512hex(bytes) !== record.sha512) violations.push(`sha512 changed ${name} (REL-06)`);
    if (record.integrity && sha512sri(bytes) !== record.integrity) violations.push(`integrity changed ${name}`);
  }
  return violations;
}

async function sealCommand(argv) {
  const options = parseOptions(argv);
  const out = resolve(requiredOption(options, '--out'));
  const manifestPath = join(out, 'sealed', 'release-manifest.json');
  const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));
  const violations = recomputeArtifacts(out, manifest);
  if (violations.length) throw new Error(`release.artifact_changed_after_assembly: ${violations.join('; ')}`);
  const reports = collectOptions(options, '--report');
  manifest.testReports = {};
  for (const report of reports) {
    const bytes = readFileSync(report);
    manifest.testReports[report.replaceAll('\\', '/').split('/').pop()] = { bytes: bytes.length, sha256: sha256(bytes) };
  }
  writeJson(manifestPath, manifest);
  writeSha256Sums(out, manifest);
  return { sealed: manifestPath, reports: Object.keys(manifest.testReports) };
}

function verifySealedCommand(argv) {
  const options = parseOptions(argv);
  const out = resolve(requiredOption(options, '--out'));
  const sealed = join(out, 'sealed');
  const manifest = JSON.parse(readFileSync(join(sealed, 'release-manifest.json'), 'utf8'));
  const violations = recomputeArtifacts(out, manifest);
  for (const [name, record] of Object.entries(manifest.testReports)) {
    const candidates = [join(sealed, name), join(out, name), ...(options['--report-dir'] ? [join(options['--report-dir'], name)] : [])];
    const path = candidates.find(candidate => existsSync(candidate));
    if (!path) { violations.push(`missing test report ${name}`); continue; }
    const bytes = readFileSync(path);
    if (bytes.length !== record.bytes || sha256(bytes) !== record.sha256) violations.push(`test report changed ${name}`);
  }
  const sums = readFileSync(join(sealed, 'SHA256SUMS'), 'utf8').split('\n').filter(Boolean);
  const expected = new Map(sums.map(line => {
    const [digest, name] = line.split(/\s{2}/);
    return [name, digest];
  }));
  if (expected.get('release-manifest.json') !== sha256(readFileSync(join(sealed, 'release-manifest.json')))) {
    violations.push('release-manifest.json digest mismatch in SHA256SUMS');
  }
  for (const name of Object.keys(manifest.artifacts)) {
    if (expected.get(name) !== manifest.artifacts[name].sha256) violations.push(`SHA256SUMS mismatch ${name}`);
  }
  return { ok: violations.length === 0, violations, version: manifest.version, commit: manifest.commit, artifacts: Object.keys(manifest.artifacts) };
}

function releaseIdentityCommand(argv) {
  const options = parseOptions(argv);
  const ref = requiredOption(options, '--ref');
  const tag = requiredOption(options, '--tag');
  const version = parseTag(tag); // strict v<semver> incl. -rc.N (REL-01/11)
  const synchronized = synchronizeVersion(repository, false);
  if (synchronized.version !== version) throw new Error(`release.tag_version_mismatch: ${tag} vs source ${synchronized.version} (REL-01)`);
  const policy = policyFor(version);
  const pkg = JSON.parse(readFileSync(join(repository, 'packages/npm/lycheedev/package.json'), 'utf8'));
  if (pkg.private === true && !policy.privateMustBeTrue) throw new Error('release.private_package: a release package cannot be private (§5)');
  if (pkg.private !== true && policy.privateMustBeTrue) throw new Error('release.public_dev_package: a -dev package must stay private');
  const id = identity({ requireTagRef: true, tag });
  if (id.dirty) throw new Error('release.identity_dirty (REL-01/05)');
  if (!ref.startsWith('refs/tags/')) throw new Error(`release.not_a_tag_ref: ${ref}`);
  return {
    ok: true, version, tag, distTag: policy.npmDistTag, commit: id.commit, tagVerified: id.tagVerified,
    private: pkg.private === true, publishable: !policy.privateMustBeTrue,
  };
}

/** REL-09: never treat timeout/403/unknown responses as "package absent". */
export function classifyRegistry({ status, body, expectedIntegrity }) {
  if (status === 404) return { state: 'absent' };
  if (status !== 200 || typeof body !== 'string') return { state: 'unknown', status };
  try {
    const metadata = JSON.parse(body);
    const integrity = metadata?.dist?.integrity;
    if (typeof integrity !== 'string') return { state: 'unknown', status };
    if (expectedIntegrity && integrity !== expectedIntegrity) return { state: 'exists-conflicting', integrity };
    return { state: expectedIntegrity ? 'exists-matching' : 'exists', integrity };
  } catch {
    return { state: 'unknown', status };
  }
}

export async function registryStateCommand(argv, fetchImpl = fetch) {
  const options = parseOptions(argv);
  const name = requiredOption(options, '--name');
  const version = requiredOption(options, '--version');
  const expectedIntegrity = options['--integrity'];
  const url = `https://registry.npmjs.org/${name}/${version}`;
  let result;
  try {
    const response = await fetchImpl(url, { headers: { accept: 'application/json' }, signal: AbortSignal.timeout(30000) });
    result = classifyRegistry({ status: response.status, body: await response.text(), expectedIntegrity });
  } catch (error) {
    result = { state: 'unknown', error: String(error?.message ?? error) };
  }
  const record = { name, version, url, ...result };
  if (options['--out']) writeJson(resolve(options['--out']), record);
  if (result.state === 'exists-conflicting') process.exitCode = 1;
  if (result.state === 'unknown') process.exitCode = 2;
  // The top-level dispatcher is the only stdout writer. The --out variant
  // keeps the compact return used by release CI and persists the full record.
  return options['--out'] ? result : record;
}

function verifyPlatformEvidenceCommand(argv) {
  const options = parseOptions(argv);
  const out = resolve(requiredOption(options, '--out'));
  const evidenceDirectory = resolve(requiredOption(options, '--evidence'));
  const manifest = JSON.parse(readFileSync(join(out, 'sealed', 'release-manifest.json'), 'utf8'));
  const violations = [];
  const platforms = {};
  for (const entry of TARGETS) {
    const path = join(evidenceDirectory, `${entry.target}.json`);
    if (!existsSync(path)) {
      violations.push(`${entry.target}: MISSING run evidence — gate fails, never warns past (REL-13)`);
      continue;
    }
    const evidence = JSON.parse(readFileSync(path, 'utf8'));
    platforms[entry.target] = evidence;
    if (evidence.platform !== entry.target) violations.push(`${entry.target}: evidence platform mismatch`);
    if (evidence.commit !== manifest.commit) violations.push(`${entry.target}: commit ${evidence.commit} != ${manifest.commit} (REL-05)`);
    if (evidence.version !== manifest.version) violations.push(`${entry.target}: version mismatch`);
    const sealed = manifest.binaries[entry.target];
    if (evidence.binarySha256 !== sealed.sha256) violations.push(`${entry.target}: binary digest mismatch (REL-05)`);
    if (evidence.status !== 'passed') violations.push(`${entry.target}: smoke status ${evidence.status} (REL-13)`);
  }
  return { ok: violations.length === 0, violations, platforms: Object.keys(platforms) };
}

function platformEvidenceCommand(argv) {
  const options = parseOptions(argv);
  const manifestPath = resolve(requiredOption(options, '--manifest'));
  const smokePath = resolve(requiredOption(options, '--smoke'));
  const target = requiredOption(options, '--target');
  const outPath = resolve(requiredOption(options, '--out'));
  const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));
  const smoke = JSON.parse(readFileSync(smokePath, 'utf8'));
  const record = {
    platform: target, version: manifest.version, commit: manifest.commit,
    binarySha256: manifest.binaries[target]?.sha256, status: smoke.status,
    launcher: smoke.launcher ?? null, report: smokePath,
  };
  writeJson(outPath, record);
  return record;
}

function verifyRegistryReadbackCommand(argv) {
  const options = parseOptions(argv);
  const name = requiredOption(options, '--name');
  const version = requiredOption(options, '--version');
  const viewPath = resolve(requiredOption(options, '--view'));
  const tgzBytes = readFileSync(resolve(requiredOption(options, '--tgz')));
  const view = JSON.parse(readFileSync(viewPath, 'utf8'));
  const violations = [];
  const expectedIntegrity = sha512sri(tgzBytes);
  const dist = view.dist ?? {};
  if (view.name !== name) violations.push(`registry name ${view.name}`);
  if (view.version !== version) violations.push(`registry version ${view.version}`);
  if (dist.integrity !== expectedIntegrity) violations.push(`registry integrity ${dist.integrity} != ${expectedIntegrity} (REL-06/12)`);
  if (!dist.attestations && !(Array.isArray(dist.signatures) && dist.signatures.length)) {
    violations.push('registry provenance/attestations missing (REL-12)');
  }
  const repositoryUrl = view.repository?.url ?? '';
  if (!repositoryUrl.includes('Follen/Lychee-Dev')) violations.push(`registry repository ${repositoryUrl}`);
  return { ok: violations.length === 0, violations, integrity: dist.integrity, attestations: dist.attestations ?? dist.signatures };
}

function checkAssetsCommand(argv) {
  const options = parseOptions(argv);
  const local = resolve(requiredOption(options, '--local'));
  const downloaded = resolve(requiredOption(options, '--downloaded'));
  const names = requiredOption(options, '--names').split(',');
  const missing = [];
  const conflicting = [];
  const matching = [];
  for (const name of names) {
    const existing = join(downloaded, name);
    if (!existsSync(existing)) { missing.push(name); continue; }
    const same = sha256(readFileSync(existing)) === sha256(readFileSync(join(local, name)));
    if (same) matching.push(name); else conflicting.push(name); // §9: published bytes are never replaced
  }
  return { ok: conflicting.length === 0, missing, conflicting, matching };
}

function releaseNotesCommand(argv) {
  const options = parseOptions(argv);
  const out = resolve(requiredOption(options, '--out'));
  const manifest = JSON.parse(readFileSync(join(resolve(requiredOption(options, '--sealed')), 'release-manifest.json'), 'utf8'));
  const lines = [
    `# lycheedev ${manifest.version}`,
    '',
    `- Commit: \`${manifest.commit}\` (tag \`${manifest.tag}\`, tag verified: ${manifest.tagVerified})`,
    `- npm dist-tag: \`${distTagFor(manifest.version)}\``,
    `- Toolchain: ${manifest.toolchain.go}, node ${manifest.toolchain.node}, npm ${manifest.toolchain.npm}, ${manifest.toolchain.cgo}`,
    '',
    '## Artifacts',
    '',
    '| file | bytes | sha256 |',
    '| --- | --- | --- |',
    ...Object.entries(manifest.artifacts).sort().map(([name, record]) => `| ${name} | ${record.bytes} | \`${record.sha256}\` |`),
    '',
    `Corresponding source: \`${manifest.correspondingSource.archive}\` (sha256 \`${manifest.correspondingSource.sha256}\`), repository ${manifest.correspondingSource.repository}, commit \`${manifest.correspondingSource.commit}\`.`,
    '',
    '## Verification',
    '',
    '- Windows required jobs and the windows-amd64 run smoke passed in the release workflow; reports remain in its sealed-bundle artifact. Digests are sealed in `release-manifest.json` / `SHA256SUMS`.',
    '- This release does not import or migrate any legacy tool data; new-format state is created from scratch on first use.',
    '- Third-party components and their licenses: `THIRD_PARTY_NOTICES` inside the npm package.',
  ];
  if (manifest.deviations.length) lines.push('', '## Recorded deviations', '', ...manifest.deviations.map(item => `- ${item}`));
  writeFileSync(out, `${lines.join('\n')}\n`);
  return { out };
}

export function parseOptions(argv) {
  const options = {};
  for (let index = 0; index < argv.length; index++) {
    const token = argv[index];
    if (!token.startsWith('--')) throw new Error(`release.unknown_argument: ${token}`);
    const next = argv[index + 1];
    let value;
    if (next === undefined || next.startsWith('--')) value = true;
    else { value = next; index++; }
    // Repeated options accumulate (collectOptions consumes the array form);
    // `seal --report a --report b` must fold every report, not just the last.
    const existing = options[token];
    if (existing === undefined) options[token] = value;
    else if (Array.isArray(existing)) existing.push(value);
    else options[token] = [existing, value];
  }
  return options;
}
function requiredOption(options, name) {
  const value = options[name];
  if (typeof value !== 'string') throw new Error(`release.missing_option: ${name}`);
  return value;
}
function collectOptions(options, name) {
  const value = options[name];
  if (value === undefined) return [];
  return Array.isArray(value) ? value : [value];
}

/**
 * Content whitelist audit of the packed npm tarball (PKG-05, REL-08).
 * @param {{name: string, bytes: Buffer}[]} entries `package/...` entries
 */
export function auditTgz(entries, { version, expectedCommit, sourceRoot } = {}) {
  const violations = [];
  const byName = new Map(entries.map(entry => [entry.name, entry.bytes]));
  const allowed = name => name === 'package/package.json' || name === 'package/README.md' || name === 'package/LICENSE'
    || name === 'package/THIRD_PARTY_NOTICES' || name === 'package/bin/lycheedev.mjs' || name === 'package/release.json'
    || TARGETS.some(entry => `package/${entry.binary}` === name)
    || /^package\/payload\/(?:addon|skill)\/[^/].*$/.test(name);
  const required = ['package/package.json', 'package/README.md', 'package/LICENSE', 'package/THIRD_PARTY_NOTICES',
    'package/bin/lycheedev.mjs', 'package/release.json',
    ...TARGETS.map(entry => `package/${entry.binary}`),
    ...REQUIRED_RESOURCES.map(path => `package/payload/${path}`)];
  for (const entry of entries) {
    if (!allowed(entry.name)) violations.push(`forbidden entry: ${entry.name} (PKG-05 whitelist)`);
    if (/(^|\/)(?:tests?|docs?|fixtures|node_modules|vendor|\.git)(\/|$)|\.py$|\.test\.mjs$|wowdoc|wowdata|wowdump|automation\.py|workspace\.json|WTF|SavedVariables/i.test(entry.name)) {
      violations.push(`banned content: ${entry.name} (PKG-05)`);
    }
  }
  for (const name of required) if (!byName.has(name)) violations.push(`missing entry: ${name}`);
  const pkg = byName.has('package/package.json') ? JSON.parse(byName.get('package/package.json').toString('utf8')) : {};
  if (pkg.name !== 'lycheedev') violations.push(`package name ${pkg.name}`);
  if (version && pkg.version !== version) violations.push(`package version ${pkg.version} != ${version} (REL-01)`);
  if (pkg.type !== 'module') violations.push('package type must be module');
  if (pkg.bin?.lycheedev !== 'bin/lycheedev.mjs') violations.push('bin.lycheedev must be bin/lycheedev.mjs');
  for (const field of ['dependencies', 'optionalDependencies', 'peerDependencies']) {
    if (pkg[field] && Object.keys(pkg[field]).length) violations.push(`${field} must be empty (REL-08)`);
  }
  if (pkg.engines?.node !== '>=22.14.0') violations.push(`engines.node ${pkg.engines?.node}`);
  if (pkg.publishConfig?.access !== 'public') violations.push('publishConfig.access must be public');
  if (pkg.publishConfig?.registry !== 'https://registry.npmjs.org/') violations.push('publishConfig.registry must be the official registry');
  if (!`${pkg.repository?.url ?? ''}`.includes('Follen/Lychee-Dev')) violations.push(`repository.url ${pkg.repository?.url}`);
  if (pkg.repository?.directory !== 'packages/npm/lycheedev') violations.push(`repository.directory ${pkg.repository?.directory}`);
  for (const name of ['README.md', 'THIRD_PARTY_NOTICES', 'release.json', 'native/', 'payload/']) {
    if (!(pkg.files ?? []).includes(name)) violations.push(`files whitelist must include ${name}`);
  }
  const policy = version ? policyFor(version) : null;
  if (policy && (pkg.private === true) !== policy.privateMustBeTrue) violations.push(`private=${pkg.private} violates the ${policy.development ? 'development' : 'release'} version policy (§5)`);

  const manifestBytes = byName.get('package/release.json');
  if (manifestBytes) {
    let manifest;
    try { manifest = JSON.parse(manifestBytes.toString('utf8')); } catch { violations.push('release.json is not valid JSON'); }
    if (manifest) {
      if (manifest.schema !== 'lycheedev.release.v1') violations.push(`release.json schema ${manifest.schema}`);
      if (version && manifest.version !== version) violations.push(`release.json version ${manifest.version} (REL-01)`);
      if (!HEX40.test(manifest.commit ?? '')) violations.push('release.json commit must be 40 lowercase hex');
      if (expectedCommit && manifest.commit !== expectedCommit) violations.push(`release.json commit ${manifest.commit} != ${expectedCommit} (REL-05)`);
      const binaryTargets = Object.keys(manifest.binaries ?? {});
      for (const entry of TARGETS) {
        const record = manifest.binaries?.[entry.target];
        if (!record) { violations.push(`release.json missing binary ${entry.target} (npm assembly requires exactly the shipped target)`); continue; }
        if (record.path !== entry.binary) violations.push(`release.json binary path ${record.path}`);
        const bytes = byName.get(`package/${entry.binary}`);
        if (!bytes) continue;
        if (bytes.length !== record.bytes || sha256(bytes) !== record.sha256) violations.push(`binary record mismatch ${entry.target} (REL-06)`);
      }
      for (const extra of binaryTargets) if (!TARGETS.some(entry => entry.target === extra)) violations.push(`release.json unknown binary ${extra}`);
      const listed = new Set();
      for (const resource of manifest.resources ?? []) {
        if (!/^(?:addon|skill)\//.test(resource.path ?? '')) violations.push(`resource outside payload/addon+skill: ${resource.path}`);
        if (listed.has(resource.path)) violations.push(`duplicate resource ${resource.path}`);
        listed.add(resource.path);
        const bytes = byName.get(`package/payload/${resource.path}`);
        if (!bytes) { violations.push(`resource missing from payload: ${resource.path}`); continue; }
        if (bytes.length !== resource.bytes || sha256(bytes) !== resource.sha256) violations.push(`resource record mismatch ${resource.path} (REL-06)`);
      }
      for (const entry of entries) {
        const match = /^package\/payload\/((?:addon|skill)\/.*)$/.exec(entry.name);
        if (match && !listed.has(match[1])) violations.push(`payload file not in release.json resources: ${match[1]}`);
      }
      const queue = byName.get('package/payload/addon/Bridge/Definitions.lua');
      if (queue && queue.toString('utf8') !== EMPTY_PROBE_QUEUE) violations.push('payload carries local task blocks: addon/Bridge/Definitions.lua (PKG-05)');
      const source = manifest.correspondingSource;
      if (!source) violations.push('release.json missing correspondingSource (licensing closure; npm assembly requires it)');
      else {
        if (source.repository !== REPOSITORY_URL) violations.push(`correspondingSource.repository ${source.repository}`);
        if (source.commit !== manifest.commit) violations.push('correspondingSource.commit must equal release.json commit');
        if (version && source.tag !== `v${version}`) violations.push(`correspondingSource.tag ${source.tag} != v${version}`);
        if (version && source.archive !== `lycheedev-${version}-corresponding-source.tar.gz`) violations.push(`correspondingSource.archive ${source.archive}`);
        if (!HEX64.test(source.sha256 ?? '')) violations.push('correspondingSource.sha256 must be 64 lowercase hex');
      }
    }
  }
  if (sourceRoot) {
    for (const name of ['README.md', 'LICENSE', 'THIRD_PARTY_NOTICES']) {
      const packed = byName.get(`package/${name}`);
      const sourcePath = join(sourceRoot, name);
      if (!existsSync(sourcePath)) {
        if (name === 'LICENSE') violations.push('license decision pending: packages/npm/lycheedev/LICENSE is absent');
        else violations.push(`missing package source ${name}`);
        continue;
      }
      if (!packed) continue;
      if (!packed.equals(readFileSync(sourcePath))) violations.push(`${name} is not byte-identical to the repository source`);
    }
  }
  return { ok: violations.length === 0, violations, entryCount: entries.length };
}

function verifyCgoCommand(argv) {
  const options = parseOptions(argv);
  const scratch = mkdtempSync(join(tmpdir(), 'lycheedev-cgo-'));
  try {
    const binary = join(scratch, process.platform === 'win32' ? 'lycheedev-probe.exe' : 'lycheedev-probe');
    runOk('go', ['build', '-trimpath', '-buildvcs=true', '-o', binary, './cmd/lycheedev'], {
      cwd: repository, env: { ...process.env, CGO_ENABLED: '0' },
    });
    const evidence = { status: 'passed', binary, cgo: 'CGO_ENABLED=0', tests: [] };
    // Capture ingestion and decode must survive a pure-Go build before the
    // release assembly is allowed to claim CGO_ENABLED=0.
    for (const [name, pattern, envPrefix] of [
      ['hotfix capture decode', '^TestHotfixNamedTableAndLatestCLI$', 'LYCHEEDEV_HOTFIX'],
      ['asset export decode', '^(TestAssetExportProjectOffline|TestAssetImageExport(ProjectOffline|FailuresPreserveOutput))$', 'LYCHEEDEV_ASSET'],
    ]) {
      // LYCHEEDEV_*_LAUNCHER alone makes the tests execute the native CGO=0
      // binary directly; adding _NODE would route through node and try to
      // parse the executable as JavaScript.
      const output = runOk('go', ['test', '-count=1', '-run', pattern, '-v', './internal/command'], {
        cwd: repository,
        env: { ...process.env, [`${envPrefix}_LAUNCHER`]: binary },
      });
      if (!output.includes('using installed') || !output.includes('--- PASS:')) {
        throw new Error(`release.cgo_pre_step_failed: ${name} did not pass through the CGO_ENABLED=0 binary`);
      }
      evidence.tests.push({ name, pattern, output: output.split('\n').filter(line => line.includes('PASS') || line.includes('using installed')).join('\n') });
    }
    if (options['--out']) {
      mkdirSync(resolve(options['--out']), { recursive: true });
      writeJson(join(resolve(options['--out']), 'cgo-evidence.json'), evidence);
    }
    return evidence;
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
}

const commands = {
  assemble: assembleCommand,
  'verify-cgo': verifyCgoCommand,
  seal: sealCommand,
  'verify-sealed': verifySealedCommand,
  'release-identity': releaseIdentityCommand,
  'dist-tag': argv => ({ tag: distTagFor(requiredOption(parseOptions(argv), '--version')) }),
  'registry-state': registryStateCommand,
  'verify-platform-evidence': verifyPlatformEvidenceCommand,
  'platform-evidence': platformEvidenceCommand,
  'verify-registry-readback': verifyRegistryReadbackCommand,
  'check-assets': checkAssetsCommand,
  'release-notes': releaseNotesCommand,
};

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [command, ...rest] = process.argv.slice(2);
  try {
    if (!commands[command]) throw new Error(`Usage: node tools/release.mjs <${Object.keys(commands).join('|')}> [options]`);
    const result = await commands[command](rest);
    if (result !== undefined) process.stdout.write(`${JSON.stringify(result, null, 2)}\n`);
  } catch (error) {
    process.stderr.write(`${error.message}\n`);
    process.exitCode = 1;
  }
}
