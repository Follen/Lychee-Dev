// Local development staging for a real-client deployment: builds a release root
// (payload/ + release.json) from the working tree without running the release
// assembly (no corresponding-source archive, no npm pack, no binary build).
// The staged release is NOT release-eligible; it exists so `addon install` can
// deploy the current tree as a managed installation for live verification.
//
// Usage: node tools/live-stage.mjs --out <directory> [--commit <40-hex>]
import { createHash } from 'node:crypto';
import { mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const repository = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const args = process.argv.slice(2);
const option = (name) => {
  const index = args.indexOf(name);
  return index >= 0 ? args[index + 1] : undefined;
};
const out = option('--out');
if (!out) throw new Error('live-stage.missing_option: --out');
const version = JSON.parse(readFileSync(join(repository, 'release/version.json'), 'utf8')).version;
const commit = (option('--commit') ?? '0'.repeat(40)).toLowerCase();
if (!/^[0-9a-f]{40}$/.test(commit)) throw new Error('live-stage.invalid_commit');

// A release root must never carry local probe task blocks. Mirrors the release
// contract: the shipped queue is always the canonical empty literal.
const EMPTY_PROBE_QUEUE = 'local _, ns = ...\nns.ProbeDefinitions = {schema="lycheedev.queue.v1",entries={\n}}\n';
const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');

function walkFiles(root, prefix = '') {
  const files = [];
  for (const entry of readdirSync(root, { withFileTypes: true }).sort((a, b) => (a.name < b.name ? -1 : 1))) {
    const name = prefix ? `${prefix}/${entry.name}` : entry.name;
    if (entry.isDirectory()) files.push(...walkFiles(join(root, entry.name), name));
    else if (entry.isFile()) files.push({ name, path: join(root, entry.name) });
  }
  return files;
}

const absolute = resolve(out);
rmSync(absolute, { recursive: true, force: true });
const resources = [];
for (const [source, prefix] of [[join(repository, 'addon'), 'addon'], [join(repository, 'skills/lycheedev'), 'skill']]) {
  for (const file of walkFiles(source)) {
    const name = `${prefix}/${file.name}`;
    const content = name === 'addon/Bridge/Definitions.lua' ? Buffer.from(EMPTY_PROBE_QUEUE) : readFileSync(file.path);
    const destination = join(absolute, 'payload', ...name.split('/'));
    mkdirSync(dirname(destination), { recursive: true });
    writeFileSync(destination, content);
    resources.push({ path: name, bytes: content.length, sha256: sha256(content) });
  }
}
resources.sort((a, b) => (a.path < b.path ? -1 : 1));

const release = {
  schema: 'lycheedev.release.v1',
  version,
  commit,
  binaries: {
    'windows-amd64': { path: 'native/windows-amd64/lycheedev.exe', bytes: 1, sha256: sha256(Buffer.from('live-stage placeholder')) },
  },
  resources,
};
writeFileSync(join(absolute, 'release.json'), `${JSON.stringify(release, null, 2)}\n`);
console.log(JSON.stringify({ out: absolute, version, commit, resources: resources.length, toc: resources.find(r => r.path === 'addon/Lychee Dev.toc') }, null, 2));
