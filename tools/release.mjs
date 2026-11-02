// Current Rust product release helpers.
//
// This module has no build, publish, registry or Go step: it only holds the
// reusable policy and package-audit logic that the current delivery flow and
// tests share. The actual npm package assembly happens once in the CI build
// stage (`tools/rust-package-smoke.mjs --assembly-only`) and the CI
// release candidate/seal logic lives in `tools/ci-release.mjs`; nothing here
// compiles a binary, packs a second tarball or contacts a registry.
//
// Commands:
//   audit-tgz           content whitelist audit of the packed Rust npm tarball
//   policy / dist-tag   version-bound npm channel policy
//   verify-npm-channel  read-back comparison used by a future publish workflow
import { createHash } from 'node:crypto';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { readTgz } from './tgz.mjs';

export const repository = resolve(dirname(fileURLToPath(import.meta.url)), '..');
export const REPOSITORY_URL = 'https://github.com/Follen/Lychee-Dev';
// Delivery target: Windows amd64 only. The Rust CLI binary is produced by the
// CI build stage; this module never builds it.
export const TARGETS = Object.freeze([
  { target: 'windows-amd64', binary: 'native/windows-amd64/lycheedev.exe' },
]);
// Current payload families inside `payload/`. The retired LuaLS payload
// (`tool/luals`) is not part of the product and must never appear again.
const PAYLOAD_FAMILIES = Object.freeze(['addon', 'skill', 'tool/luac']);
const REQUIRED_LUAC_FILES = Object.freeze(['runtime.json', 'LICENSE', 'lua-5.1.5.tar.gz']);
const HEX40 = /^[a-f0-9]{40}$/;
const HEX64 = /^[a-f0-9]{64}$/;

const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');

/** Release-version policy: `-dev` versions are development-only packages. */
export function policyFor(version) {
  const development = version.endsWith('-dev');
  return {
    development,
    // The published package cannot be private; only an explicit development
    // version policy may keep the development manifest private.
    privateMustBeTrue: development,
    // Development versions are never published, so they have no dist-tag.
    npmDistTag: development ? null : distTagFor(version),
  };
}

/** Channel shape; the reviewed manifest owns the tag and version selection. */
export function validateNpmChannel(value, version) {
  const stable = /^\d+\.\d+\.\d+$/;
  if (!value || typeof value !== 'object' || Array.isArray(value) ||
      Object.keys(value).sort().join(',') !== 'distTag,preserveLatest,schema,version' ||
      value.schema !== 'lycheedev.npm-channel.v1' || typeof value.distTag !== 'string' ||
      !/^[a-z][a-z0-9._-]*$/.test(value.distTag) || value.distTag === 'latest' ||
      typeof value.version !== 'string' || typeof value.preserveLatest !== 'string' ||
      !stable.test(value.version) || !stable.test(value.preserveLatest) ||
      value.version === value.preserveLatest || (version !== undefined && value.version !== version)) {
    throw new Error('release.invalid_npm_channel');
  }
  return value;
}

function npmChannel(version) {
  return validateNpmChannel(JSON.parse(readFileSync(join(repository, 'release/npm-channel.json'), 'utf8')), version);
}

export function distTagFor(version) {
  return npmChannel(version).distTag;
}

export function verifyNpmChannelTags(tags, channel, version, distTag, phase) {
  validateNpmChannel(channel, version);
  if (distTag !== channel.distTag || !['before', 'after'].includes(phase) ||
      !tags || typeof tags !== 'object' || Array.isArray(tags) ||
      tags.latest !== channel.preserveLatest || (phase === 'after' && tags[distTag] !== version)) {
    throw new Error('release.npm_channel_mismatch');
  }
  return { ok: true, version, distTag, latest: tags.latest, phase };
}

/**
 * Content whitelist audit of the packed npm tarball (PKG-05/REL-08) for the
 * current Rust payload layout. `developmentOnly` accepts the private rehearsal
 * package produced by `tools/rust-package-smoke.mjs` (never publishable).
 * @param {{name: string, bytes: Buffer}[]} entries `package/...` entries
 */
export function auditTgz(entries, { version, expectedCommit, sourceRoot, developmentOnly = false } = {}) {
  const violations = [];
  const byName = new Map(entries.map(entry => [entry.name, entry.bytes]));
  const allowed = name => name === 'package/package.json' || name === 'package/README.md' || name === 'package/LICENSE'
    || name === 'package/THIRD_PARTY_NOTICES' || name === 'package/bin/lycheedev.mjs' || name === 'package/release.json'
    || TARGETS.some(entry => `package/${entry.binary}` === name)
    || PAYLOAD_FAMILIES.some(family => name.startsWith(`package/payload/${family}/`) && !name.endsWith('/'));
  const required = ['package/package.json', 'package/README.md', 'package/LICENSE', 'package/THIRD_PARTY_NOTICES',
    'package/bin/lycheedev.mjs', 'package/release.json',
    ...TARGETS.map(entry => `package/${entry.binary}`),
    ...REQUIRED_LUAC_FILES.map(path => `package/payload/tool/luac/${path}`)];
  for (const entry of entries) {
    if (!allowed(entry.name)) violations.push(`forbidden entry: ${entry.name} (PKG-05 whitelist)`);
    if (/luals|lua-language-server/i.test(entry.name)) violations.push(`retired LuaLS payload: ${entry.name}`);
    if (!entry.name.startsWith('package/payload/tool/luac/')
      && /(^|\/)(?:tests?|docs?|fixtures|node_modules|vendor|\.git)(\/|$)/i.test(entry.name)) violations.push(`banned content: ${entry.name} (PKG-05)`);
    if (/\.py$|\.test\.mjs$|wowdoc|wowdata|wowdump/i.test(entry.name)) violations.push(`banned content: ${entry.name} (PKG-05)`);
    if (/(^|\/)(?:WTF|SavedVariables)(\/|$)|workspace\.json/i.test(entry.name)) violations.push(`banned content: ${entry.name} (PKG-05)`);
  }
  for (const name of required) if (!byName.has(name)) violations.push(`missing entry: ${name}`);
  const families = new Set();
  for (const entry of entries) for (const family of PAYLOAD_FAMILIES) if (entry.name.startsWith(`package/payload/${family}/`)) families.add(family);
  for (const family of ['addon', 'skill']) if (!families.has(family)) violations.push(`missing payload family: ${family}`);

  const pkg = byName.has('package/package.json') ? JSON.parse(byName.get('package/package.json').toString('utf8')) : {};
  if (pkg.name !== 'lycheedev') violations.push(`package name ${pkg.name}`);
  if (version && pkg.version !== version) violations.push(`package version ${pkg.version} != ${version} (REL-01)`);
  if (pkg.type !== 'module') violations.push('package type must be module');
  if (pkg.license !== 'MIT') violations.push(`package license ${pkg.license}`);
  if (pkg.bin?.lycheedev !== 'bin/lycheedev.mjs') violations.push('bin.lycheedev must be bin/lycheedev.mjs');
  for (const field of ['dependencies', 'optionalDependencies', 'peerDependencies']) {
    if (pkg[field] && Object.keys(pkg[field]).length) violations.push(`${field} must be empty (REL-08)`);
  }
  if (pkg.engines?.node !== '>=22.14.0') violations.push(`engines.node ${pkg.engines?.node}`);
  if (pkg.publishConfig?.access !== 'public') violations.push('publishConfig.access must be public');
  if (pkg.publishConfig?.registry !== 'https://registry.npmjs.org/') violations.push('publishConfig.registry must be the official registry');
  if (!`${pkg.repository?.url ?? ''}`.includes('Follen/Lychee-Dev')) violations.push(`repository.url ${pkg.repository?.url}`);
  if (pkg.repository?.directory !== 'packages/npm/lycheedev') violations.push(`repository.directory ${pkg.repository?.directory}`);
  for (const name of ['bin/', 'native/', 'payload/', 'release.json', 'README.md', 'LICENSE', 'THIRD_PARTY_NOTICES']) {
    if (!(pkg.files ?? []).includes(name)) violations.push(`files whitelist must include ${name}`);
  }
  if (developmentOnly) {
    if (pkg.private !== true) violations.push('development-only candidate package must be private');
  } else if (version && (pkg.private === true) !== policyFor(version).privateMustBeTrue) {
    violations.push(`private=${pkg.private} violates the ${policyFor(version).development ? 'development' : 'release'} version policy`);
  }

  const manifestBytes = byName.get('package/release.json');
  if (manifestBytes) {
    let manifest;
    try { manifest = JSON.parse(manifestBytes.toString('utf8')); } catch { violations.push('release.json is not valid JSON'); }
    if (manifest) {
      if (manifest.schema !== 'lycheedev.release.v1') violations.push(`release.json schema ${manifest.schema}`);
      if (manifest.runtime !== 'rust') violations.push(`release.json runtime ${manifest.runtime}`);
      if (version && manifest.version !== version) violations.push(`release.json version ${manifest.version} (REL-01)`);
      if (!HEX40.test(manifest.commit ?? '')) violations.push('release.json commit must be 40 lowercase hex');
      if (expectedCommit && manifest.commit !== expectedCommit) violations.push(`release.json commit ${manifest.commit} != ${expectedCommit} (REL-05)`);
      try {
        const channel = validateNpmChannel(manifest.npmChannel, version);
        const authority = npmChannel(version ?? manifest.version);
        if (Object.keys(authority).some(key => channel[key] !== authority[key])) throw new Error('release.channel_differs');
      } catch { violations.push('release.json npmChannel is not the current channel contract'); }
      const binaries = manifest.binaries ?? {};
      for (const entry of TARGETS) {
        const record = binaries[entry.target];
        if (!record) { violations.push(`release.json missing binary ${entry.target}`); continue; }
        if (record.path !== entry.binary) violations.push(`release.json binary path ${record.path}`);
        const bytes = byName.get(`package/${entry.binary}`);
        if (bytes && (bytes.length !== record.bytes || sha256(bytes) !== record.sha256)) violations.push(`binary record mismatch ${entry.target} (REL-06)`);
      }
      for (const extra of Object.keys(binaries)) if (!TARGETS.some(entry => entry.target === extra)) violations.push(`release.json unknown binary ${extra}`);
      const listed = new Set();
      for (const resource of manifest.resources ?? []) {
        if (!PAYLOAD_FAMILIES.some(family => resource.path?.startsWith(`${family}/`))) violations.push(`resource outside supported payload: ${resource.path}`);
        if (listed.has(resource.path)) violations.push(`duplicate resource ${resource.path}`);
        listed.add(resource.path);
        const bytes = byName.get(`package/payload/${resource.path}`);
        if (!bytes) { violations.push(`resource missing from payload: ${resource.path}`); continue; }
        if (bytes.length !== resource.bytes || sha256(bytes) !== resource.sha256) violations.push(`resource record mismatch ${resource.path} (REL-06)`);
      }
      for (const entry of entries) {
        const match = /^package\/payload\/((?:addon|skill|tool\/luac)\/.*)$/.exec(entry.name);
        if (match && !listed.has(match[1])) violations.push(`payload file not in release.json resources: ${match[1]}`);
      }
      // The managed luac payload must be the exact pinned compiler identity.
      const runtimeFile = byName.get('package/payload/tool/luac/runtime.json');
      if (runtimeFile) {
        const pinned = JSON.parse(readFileSync(join(repository, 'release/tools/luac.json'), 'utf8'));
        if (!runtimeFile.equals(Buffer.from(`${JSON.stringify(pinned.runtime, null, 2)}\n`))) violations.push('managed luac runtime identity differs from release/tools/luac.json');
      }
    }
  }
  const notices = byName.get('package/THIRD_PARTY_NOTICES');
  if (notices) {
    const text = notices.toString('utf8');
    if (!text.includes('Rust dependency closure')) violations.push('THIRD_PARTY_NOTICES is missing the Rust dependency closure');
    if (!text.includes('Lua 5.1.5')) violations.push('THIRD_PARTY_NOTICES is missing the managed Lua 5.1.5 notice');
  }
  if (sourceRoot) {
    for (const name of ['README.md', 'LICENSE']) {
      const packed = byName.get(`package/${name}`);
      const sourcePath = join(sourceRoot, name);
      if (!existsSync(sourcePath)) { violations.push(`missing package source ${name}`); continue; }
      if (packed && !packed.equals(readFileSync(sourcePath))) violations.push(`${name} is not byte-identical to the repository source`);
    }
  }
  return { ok: violations.length === 0, violations, entryCount: entries.length };
}

function parseOptions(argv) {
  const options = {};
  for (let index = 0; index < argv.length; index++) {
    const token = argv[index];
    if (!token.startsWith('--')) throw new Error(`release.unexpected_argument: ${token}`);
    const next = argv[index + 1];
    if (next === undefined || next.startsWith('--')) options[token] = true;
    else { options[token] = next; index++; }
  }
  return options;
}

function requiredOption(options, name) {
  const value = options[name];
  if (typeof value !== 'string') throw new Error(`release.missing_option: ${name}`);
  return value;
}

function auditCommand(argv) {
  const options = parseOptions(argv);
  const tgz = resolve(requiredOption(options, '--tgz'));
  if (!existsSync(tgz)) throw new Error(`release.tgz_missing: ${tgz}`);
  const bytes = readFileSync(tgz), entries = readTgz(bytes);
  const version = typeof options['--version'] === 'string' ? options['--version'] : JSON.parse(readFileSync(join(repository, 'release/version.json'), 'utf8')).version;
  const expectedCommit = typeof options['--commit'] === 'string' ? options['--commit'] : undefined;
  const sourceRoot = options['--source-root'] ? resolve(options['--source-root']) : join(repository, 'packages/npm/lycheedev');
  const report = { ...auditTgz(entries, { version, expectedCommit, sourceRoot, developmentOnly: options['--development-only'] === true }),
    tgzSHA256: sha256(bytes), tgzBytes: bytes.length };
  if (typeof options['--out'] === 'string') writeFileSync(resolve(options['--out']), `${JSON.stringify(report, null, 2)}\n`);
  if (!report.ok) throw new Error(`release.tgz_audit_failed: ${report.violations.join('; ')}`);
  return report;
}

const commands = {
  'audit-tgz': auditCommand,
  policy: argv => policyFor(requiredOption(parseOptions(argv), '--version')),
  'dist-tag': argv => ({ tag: distTagFor(requiredOption(parseOptions(argv), '--version')) }),
  'verify-npm-channel': argv => {
    const options = parseOptions(argv);
    return verifyNpmChannelTags(JSON.parse(readFileSync(requiredOption(options, '--view'), 'utf8')),
      npmChannel(requiredOption(options, '--version')), requiredOption(options, '--version'),
      requiredOption(options, '--dist-tag'), requiredOption(options, '--phase'));
  },
};

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const [command, ...rest] = process.argv.slice(2);
    if (!commands[command]) throw new Error(`Usage: node tools/release.mjs <${Object.keys(commands).join('|')}> [options]`);
    const result = commands[command](rest);
    if (result !== undefined) process.stdout.write(`${JSON.stringify(result, null, 2)}\n`);
  } catch (error) {
    process.stderr.write(`${error.message}\n`);
    process.exitCode = 1;
  }
}
