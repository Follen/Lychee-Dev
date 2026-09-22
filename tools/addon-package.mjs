// Addon release ZIP build and structural verification (regression PKG-04/PKG-05):
// the archive must be exactly the four TOC manifests, their load files and
// Media/ under one `Lychee Dev/` directory — tests, tools, docs, local task
// blocks and user data never ship. Built with the deterministic writer in
// tools/zip.mjs; zero dependencies.
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, readdirSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { readZip, writeZip } from './zip.mjs';

const repository = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const ROOT = 'Lychee Dev';
const CLIENTS = ['Mainline', 'Mists', 'Wrath', 'Forever'];
// The event catalog line is per-client like the Clients\ file; every other load
// after the first line must match verbatim and in order across all four TOCs.
const CLIENT_CATALOG = /^Modules[\\/]Events[\\/]CatalogData_[A-Za-z0-9]+\.lua$/;
const CATALOG_SLOT = 'Modules\\Events\\CatalogData_*.lua';
const EMPTY_DEFINITIONS = 'local _, ns = ...\nns.ProbeDefinitions = {schema="lycheedev.queue.v1",entries={\n}}\n';
// Ban tokens are path fragments on the lowercased, /-separated entry name.
const BANNED_FRAGMENTS = [
  'automation.py', 'wowdoc', 'wowdata', 'wowdump',
  'wtf/', 'savedvariables', '/account/', 'tests/', '/test/', 'tools/', 'docs/',
  'fixtures', 'node_modules', 'vendor/', '.git',
];

const tocKey = client => `${ROOT}/Lychee Dev_${client}.toc`;
const loadKey = line => `${ROOT}/${line.replaceAll('\\', '/')}`;

function walkFiles(root, prefix = '') {
  const files = [];
  for (const entry of readdirSync(join(root, prefix), { withFileTypes: true })) {
    const rel = prefix ? `${prefix}/${entry.name}` : entry.name;
    if (entry.isDirectory()) files.push(...walkFiles(root, rel));
    else if (entry.isFile()) files.push(rel);
  }
  return files.sort();
}

/**
 * Parse a WoW TOC manifest into its metadata headers and load list.
 * @param {string} text raw TOC text
 * @returns {{interface: string|null, title: string|null, version: string|null, savedVariables: string|null, loads: string[]}} header values (null when absent) and non-comment non-empty load lines verbatim (backslash form)
 */
export function parseToc(text) {
  const fields = { Interface: 'interface', Title: 'title', Version: 'version', SavedVariables: 'savedVariables' };
  const toc = { interface: null, title: null, version: null, savedVariables: null, loads: [] };
  for (const raw of text.replaceAll('\r\n', '\n').split('\n')) {
    const line = raw.trim();
    if (line === '') continue;
    const header = /^##\s*([^:]+):\s*(.*)$/.exec(line);
    if (header) {
      const field = fields[header[1].trim()];
      if (field) toc[field] = header[2].trim();
      continue;
    }
    if (line.startsWith('#')) continue;
    toc.loads.push(line);
  }
  return toc;
}

/**
 * Collect the exact release payload from the addon source tree.
 * @param {string} repoRoot repository root containing `addon/` and `release/`
 * @returns {{entries: Map<string, Buffer>, tocs: {name: string, parse: ReturnType<typeof parseToc>}[], report: {path: string, selected: 'toc'|'load'|'media'}[]}} entries keyed `Lychee Dev/...`, the four TOCs in client order, and one report row per entry
 * @throws {Error} `addon-package.missing_load` / `addon-package.missing_media` when a referenced load file or `addon/Media/` is missing on disk
 */
export function collectAddonZipEntries(repoRoot) {
  const entries = new Map();
  const tocs = [];
  const report = [];
  const loads = new Map();
  for (const client of CLIENTS) {
    const key = tocKey(client);
    const bytes = readFileSync(join(repoRoot, 'addon', `Lychee Dev_${client}.toc`));
    const parse = parseToc(bytes.toString('utf8'));
    tocs.push({ name: `Lychee Dev_${client}.toc`, parse });
    entries.set(key, bytes);
    report.push({ path: key, selected: 'toc' });
    for (const line of parse.loads) if (!loads.has(loadKey(line))) loads.set(loadKey(line), line);
  }
  for (const [key, line] of loads) {
    const disk = join(repoRoot, 'addon', ...line.split(/[\\/]/));
    if (!existsSync(disk)) throw new Error(`addon-package.missing_load: addon/${line.replaceAll('\\', '/')}`);
    entries.set(key, readFileSync(disk));
    report.push({ path: key, selected: 'load' });
  }
  const mediaRoot = join(repoRoot, 'addon', 'Media');
  const media = existsSync(mediaRoot) ? walkFiles(mediaRoot) : [];
  if (media.length === 0) throw new Error('addon-package.missing_media: addon/Media');
  for (const rel of media) {
    const key = `${ROOT}/Media/${rel}`;
    entries.set(key, readFileSync(join(mediaRoot, ...rel.split('/'))));
    report.push({ path: key, selected: 'media' });
  }
  report.sort((a, b) => (a.path < b.path ? -1 : a.path > b.path ? 1 : 0));
  return { entries, tocs, report };
}

/**
 * Build the deterministic addon release ZIP.
 * @param {{repoRoot: string, outPath: string}} options repoRoot and the full output file path
 * @returns {Promise<{path: string, bytes: number, sha256: string, files: string[], report: {path: string, selected: string}[]}>} bytes is the ZIP byte length and sha256 its lowercase hex digest
 */
export async function buildAddonZip({ repoRoot, outPath }) {
  const { entries, report } = collectAddonZipEntries(repoRoot);
  await writeZip(entries, outPath);
  const zipBytes = readFileSync(outPath);
  return {
    path: outPath,
    bytes: zipBytes.length,
    sha256: createHash('sha256').update(zipBytes).digest('hex'),
    files: [...entries.keys()].sort(),
    report,
  };
}

/**
 * Structurally verify an addon release ZIP (regression PKG-04/PKG-05).
 * @param {Buffer|string} input ZIP bytes or a filesystem path
 * @param {{repoRoot?: string}} [options] `repoRoot` additionally requires every TOC `## Version:` to equal `release/version.json`
 * @returns {{ok: boolean, violations: string[], files: string[], sharedOrder: string[]|null}} sharedOrder is the load order every TOC must share after its `Clients\` line (per-client catalog masked), null when no TOC parsed
 */
export function verifyAddonZip(input, options = {}) {
  const buffer = typeof input === 'string' ? readFileSync(input) : input;
  const violations = [];
  let zip;
  try {
    zip = readZip(buffer); // duplicate names already fail here (check 10)
  } catch (error) {
    if (!String(error.message).startsWith('zip.duplicate_entry')) throw error;
    return { ok: false, violations: [String(error.message)], files: [], sharedOrder: null };
  }
  const names = [...zip.keys()];

  for (const name of names) {
    if (!name.startsWith(`${ROOT}/`)) violations.push(`entry outside ${ROOT}/: ${name}`);
  }

  const parsed = new Map();
  for (const client of CLIENTS) {
    const key = tocKey(client);
    const bytes = zip.get(key);
    if (!bytes) violations.push(`missing TOC: ${key}`);
    else parsed.set(key, parseToc(bytes.toString('utf8')));
  }

  for (const [key, parse] of parsed) {
    for (const line of parse.loads) {
      const target = loadKey(line);
      if (!zip.has(target)) violations.push(`missing load file: ${target} (referenced by ${key})`);
    }
  }

  const shared = new Map();
  for (const [key, parse] of parsed) {
    const first = parse.loads[0] ?? '';
    const clients = parse.loads.filter(line => /^Clients[\\/]/i.test(line));
    if (!/^Clients[\\/][^\\/]+$/.test(first)) {
      violations.push(`${key}: first load line is not one file under Clients\\: ${first || '<none>'}`);
    }
    if (clients.length !== 1) {
      violations.push(`${key}: expected exactly one Clients\\ load line, found ${clients.length}`);
    }
    shared.set(key, parse.loads.slice(1).map(line => (CLIENT_CATALOG.test(line) ? CATALOG_SLOT : line)));
  }
  const reference = [...shared][0];
  const refKey = reference?.[0] ?? null;
  const refOrder = reference?.[1] ?? [];
  for (const [key, order] of shared) {
    if (key === refKey) continue;
    if (order.length !== refOrder.length) {
      violations.push(`${key}: shared load count ${order.length} != ${refOrder.length} (${refKey})`);
    }
    for (let i = 0; i < Math.max(order.length, refOrder.length); i++) {
      if (order[i] !== refOrder[i]) {
        violations.push(`${key}: shared load order at ${i} is "${order[i] ?? '<missing>'}", ${refKey} has "${refOrder[i] ?? '<missing>'}"`);
      }
    }
  }

  const expected = options.repoRoot
    ? JSON.parse(readFileSync(join(options.repoRoot, 'release/version.json'), 'utf8')).version
    : null;
  const refVersion = [...parsed].find(([, parse]) => parse.version);
  for (const [key, parse] of parsed) {
    if (!parse.version) {
      violations.push(`${key}: missing ## Version: header`);
      continue;
    }
    if (refVersion && key !== refVersion[0] && parse.version !== refVersion[1].version) {
      violations.push(`${key}: ## Version: ${parse.version} != ${refVersion[1].version} (${refVersion[0]})`);
    }
    if (expected != null && parse.version !== expected) {
      violations.push(`${key}: ## Version: ${parse.version} != ${expected} (release/version.json)`);
    }
  }

  if (!names.some(name => name.startsWith(`${ROOT}/Media/`) && !name.endsWith('/'))) {
    violations.push(`missing Media content: ${ROOT}/Media/`);
  }

  const tocKeys = new Set(CLIENTS.map(tocKey));
  const loadKeys = new Set();
  for (const parse of parsed.values()) for (const line of parse.loads) loadKeys.add(loadKey(line));
  for (const name of names) {
    const selected = tocKeys.has(name) ? 'toc' : loadKeys.has(name) ? 'load'
      : name.startsWith(`${ROOT}/Media/`) ? 'media' : null;
    if (!selected) violations.push(`entry not selected as toc/load/media: ${name}`);
    const lowered = name.toLowerCase().replaceAll('\\', '/');
    if (lowered.endsWith('.py')) violations.push(`banned entry ${name} (.py file)`);
    for (const fragment of BANNED_FRAGMENTS) {
      if (lowered.includes(fragment)) violations.push(`banned entry ${name} (contains ${fragment})`);
    }
    if (name.toLowerCase().endsWith('.lua') && zip.get(name).length === 0) {
      violations.push(`zero-length Lua entry: ${name}`);
    }
  }

  // The probe registry ships empty: local task blocks must never reach release.
  const definitions = zip.get(`${ROOT}/Bridge/Definitions.lua`);
  if (definitions && definitions.toString('utf8').replaceAll('\r\n', '\n') !== EMPTY_DEFINITIONS) {
    violations.push(`task block: ${ROOT}/Bridge/Definitions.lua must be the empty probe queue`);
  }

  return {
    ok: violations.length === 0,
    violations,
    files: [...names].sort(),
    sharedOrder: refKey ? [...refOrder] : null,
  };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const argv = process.argv.slice(2);
  const option = name => {
    const index = argv.indexOf(name);
    return index >= 0 ? argv[index + 1] : undefined;
  };
  const usage = 'Usage: node tools/addon-package.mjs build --out <directory> | verify --zip <path>';
  try {
    const version = JSON.parse(readFileSync(join(repository, 'release/version.json'), 'utf8')).version;
    if (argv[0] === 'build') {
      const out = option('--out');
      if (!out) throw new Error(usage);
      mkdirSync(out, { recursive: true });
      const result = await buildAddonZip({ repoRoot: repository, outPath: join(out, `lycheedev-${version}-addon.zip`) });
      process.stdout.write(`${JSON.stringify(result, null, 2)}\n`);
      const verify = verifyAddonZip(result.path, { repoRoot: repository });
      if (!verify.ok) {
        process.stderr.write(`${JSON.stringify(verify, null, 2)}\n`);
        process.exitCode = 1;
      }
    } else if (argv[0] === 'verify') {
      const zipPath = option('--zip');
      if (!zipPath) throw new Error(usage);
      const verify = verifyAddonZip(zipPath, { repoRoot: repository });
      process.stdout.write(`${JSON.stringify(verify, null, 2)}\n`);
      if (!verify.ok) process.exitCode = 1;
    } else {
      throw new Error(usage);
    }
  } catch (error) {
    process.stderr.write(`${error.message}\n`);
    process.exitCode = 1;
  }
}
