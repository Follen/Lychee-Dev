// Static, test-only Go 3.2.0 oracle. Never loads or evaluates repository code.
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { lstatSync, readFileSync, readdirSync } from 'node:fs';
import { resolve } from 'node:path';

export const GO_COMMIT = '3057a9666ab078270b63c7f06de748069b0d1f4b';
export const LIMITS = Object.freeze({ fileBytes: 2 * 1024 * 1024, totalAddonBytes: 64 * 1024 * 1024, files: 2048, tokens: 500000, depth: 64 });
export const SOURCES = Object.freeze({
  commands: 'internal/command/command_contract.go',
  products: 'internal/selection/products.go',
});
// The semantic adapter below is valid ONLY for these reviewed, immutable inputs.
const PINNED_HASHES = Object.freeze({
  commands: 'b56c73427279a89196dcdebc720827e4eb4bc79f55f0d76f7561d49f69eed9fa',
  products: 'a3fbfc49221c2e9ea5eeba175ab18e6c3c2ee0d02ea31750b3b25d45f996423b',
});
// Reviewed 2026-10-16: the advertised filter and Flag arity bodies remain
// byte-identical to 7dbcd1c. Immutable OnceLock catalog and borrowed implemented
// definitions preserve every serialized field, order and selection; three new
// Rust regressions cover full catalog/help equality. Keep the whole-file guard.
export const RUST_FILTER_HASH = '640f49e4911e39fbc674e5012a77dad01511dfef99bda1faa3936fb84fb9e0dc';
export const sha256 = data => createHash('sha256').update(data).digest('hex');
const assert = (condition, message) => { if (!condition) throw new Error(message); };

// Small bounded lexer, not a Go/Rust interpreter. Comments cannot supply tokens.
// JSON-compatible strings plus explicit Rust Unicode scalar/NUL escapes are decoded;
// unsupported escape syntax fails. Rust raw strings bypass escape decoding.
function quotedString(raw) {
  let normalized = '"';
  for (let i = 1; i < raw.length - 1; i++) {
    if (raw[i] !== '\\') { normalized += raw[i]; continue; }
    if (raw[i + 1] === '0') {
      normalized += '\\u0000';
      i++;
    } else if (raw[i + 1] === 'u' && raw[i + 2] === '{') {
      const end = raw.indexOf('}', i + 3);
      const digits = raw.slice(i + 3, end);
      assert(end >= 0 && /^[0-9a-fA-F]{1,6}$/.test(digits), 'invalid Rust Unicode escape');
      const value = Number.parseInt(digits, 16);
      assert(value <= 0x10ffff && !(value >= 0xd800 && value <= 0xdfff), 'invalid Unicode scalar');
      normalized += JSON.stringify(String.fromCodePoint(value)).slice(1, -1);
      i = end;
    } else {
      normalized += raw[i] + (raw[++i] ?? '');
    }
  }
  return JSON.parse(normalized + '"');
}
export function lex(source) {
  assert(Buffer.byteLength(source) <= LIMITS.fileBytes, 'source exceeds byte limit');
  const result = [];
  let i = 0;
  while (i < source.length) {
    if (/\s/.test(source[i])) { i++; continue; }
    if (source.startsWith('//', i)) { const end = source.indexOf('\n', i); i = end < 0 ? source.length : end + 1; continue; }
    if (source.startsWith('/*', i)) {
      const end = source.indexOf('*/', i + 2);
      assert(end >= 0, 'unterminated comment');
      assert(!source.slice(i + 2, end).includes('/*'), 'nested comments unsupported');
      i = end + 2; continue;
    }
    let token;
    let rawQuote = i;
    if (source[i] === 'r') {
      rawQuote++;
      while (source[rawQuote] === '#') rawQuote++;
    }
    if (rawQuote > i && source[rawQuote] === '"') {
      // Rust permits 0..255 hashes. Scan only for the matching delimiter;
      // backslashes, shorter delimiters and comment markers are literal data.
      const hashes = rawQuote - i - 1;
      assert(hashes <= 255, 'Rust raw string delimiter exceeds 255 hashes');
      const closing = '"' + '#'.repeat(hashes);
      const end = source.indexOf(closing, rawQuote + 1);
      assert(end >= 0, 'unterminated Rust raw string');
      // Rust source preprocessing folds CRLF; an isolated CR is invalid.
      const value = source.slice(rawQuote + 1, end).replace(/\r\n/g, '\n');
      assert(!value.includes('\r'), 'invalid carriage return in Rust raw string');
      token = { string: value };
      i = end + closing.length;
    } else if (source[i] === '"') {
      const start = i++;
      while (i < source.length && source[i] !== '"') {
        if (source[i] === '\\') i++;
        i++;
      }
      assert(i < source.length, 'unterminated string');
      token = { string: quotedString(source.slice(start, ++i)) };
    } else {
      const word = /^[A-Za-z_][A-Za-z_0-9]*|^[0-9]+/.exec(source.slice(i));
      token = word ? word[0] : source[i];
      i += word ? word[0].length : 1;
    }
    result.push(token);
    assert(result.length <= LIMITS.tokens, 'source exceeds token limit');
  }
  return result;
}
const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
function locate(tokens, pattern) {
  const hits = [];
  for (let i = 0; i <= tokens.length - pattern.length; i++) {
    if (pattern.every((token, j) => same(token, tokens[i + j]))) hits.push(i + pattern.length);
  }
  assert(hits.length === 1, `expected unique declaration: ${pattern.join(' ')}, found ${hits.length}`);
  return hits[0];
}
function block(tokens, start, opening = '{', closing = '}') {
  assert(tokens[start] === opening, `expected ${opening}`);
  let depth = 0;
  for (let i = start; i < tokens.length; i++) {
    if (tokens[i] === opening) { depth++; assert(depth <= LIMITS.depth, 'nesting limit'); }
    if (tokens[i] === closing && --depth === 0) return tokens.slice(start + 1, i);
  }
  throw new Error(`unterminated ${opening}`);
}
function functionBody(tokens, name) {
  const start = locate(tokens, ['fn', name, '(']);
  const brace = tokens.indexOf('{', start);
  assert(brace >= 0 && brace - start < 30, 'unexpected Rust function signature');
  return block(tokens, brace);
}

// A literal grammar: keyed records, lists, booleans, strings and approved helpers.
// Identifiers are never resolved from the host or evaluated with eval/import.
class Literals {
  constructor(tokens, helpers = {}) { this.tokens = tokens; this.i = 0; this.helpers = helpers; this.depth = 0; }
  take(value) { assert(same(this.tokens[this.i++], value), `expected literal token ${value} at ${this.i - 1}`); }
  list(close) {
    const result = [];
    while (this.tokens[this.i] !== close) {
      assert(this.i < this.tokens.length, `missing ${close}`);
      result.push(this.value());
      if (this.tokens[this.i] !== close) this.take(',');
    }
    this.take(close); return result;
  }
  value() {
    assert(++this.depth <= LIMITS.depth, 'literal depth limit');
    const token = this.tokens[this.i++];
    let result;
    if (token && typeof token === 'object') result = token.string;
    else if (token === 'true' || token === 'false') result = token === 'true';
    else if (/^[0-9]+$/.test(token)) result = Number(token);
    else if (token === '[') {
      this.take(']');
      assert(['flagSpec', 'commandContract', 'DataProductProfile'].includes(this.tokens[this.i++]), 'unsupported array type');
      this.take('{'); result = this.list('}');
    } else if (token === '{') {
      result = Object.create(null);
      while (this.tokens[this.i] !== '}') {
        const key = this.tokens[this.i++];
        assert(typeof key === 'string' && /^[A-Za-z_]\w*$/.test(key), 'invalid literal key');
        assert(!Object.hasOwn(result, key), `duplicate literal key ${key}`);
        this.take(':'); result[key] = this.value();
        if (this.tokens[this.i] !== '}') this.take(',');
      }
      this.take('}');
    } else if (['Definition', 'flagSpec', 'Product'].includes(token) && this.tokens[this.i] === '{') result = this.value();
    else if (Object.hasOwn(this.helpers, token) && this.tokens[this.i] === '(') {
      this.take('('); result = this.helpers[token](...this.list(')'));
    } else throw new Error(`unsupported literal expression: ${String(token)}`);
    this.depth--; return result;
  }
  whole() { const result = this.value(); assert(this.i === this.tokens.length, 'unexpected trailing literal tokens'); return result; }
}
export function parseLiteral(source, helpers = {}) { return new Literals(lex(source), helpers).whole(); }
function arrayAfter(tokens, marker, helpers = {}) { const parser = new Literals(tokens.slice(locate(tokens, marker)), helpers); return parser.value(); }
function unique(values, label) { assert(new Set(values).size === values.length, `duplicate ${label}`); }
function checkedKeys(object, allowed, label) { for (const key of Object.keys(object)) assert(allowed.includes(key), `${label}: unknown field ${key}`); }

export function extractPinnedGo(commandBytes, productBytes) {
  assert(sha256(commandBytes) === PINNED_HASHES.commands, 'Go command source hash does not match frozen adapter');
  assert(sha256(productBytes) === PINNED_HASHES.products, 'Go products source hash does not match frozen adapter');
  const products = arrayAfter(lex(productBytes.toString('utf8')), ['func', 'DataProducts', '(', ')', '[', ']', 'DataProductProfile', '{', 'return']);
  assert(products.length === 7, 'expected seven Go product profiles');
  unique(products.map(p => p.Product), 'Go product');
  const tokens = lex(commandBytes.toString('utf8'));
  const helperArray = name => {
    const start = locate(tokens, ['func', name, '(']);
    const tail = tokens.slice(start, tokens.indexOf('func', start) < 0 ? tokens.length : tokens.indexOf('func', start));
    return arrayAfter(tail, ['return'], {});
  };
  const doctor = helperArray('liveDoctorTargetFlags');
  const dataStart = locate(tokens, ['func', 'dataSourceFlags', '(']);
  const baseFlags = arrayAfter(tokens.slice(dataStart), ['return', 'append', '(']);
  const helpers = {
    dataProductFlag: () => ({ name: '--product', display: `--product <${products.map(p => p.Product).join('|')}> (legacy: forever)`, value: true }),
    liveDoctorTargetFlags: () => doctor,
    dataSourceFlags: (...extra) => [...baseFlags, ...extra],
    append: (base, ...extra) => { assert(Array.isArray(base), 'append requires literal list'); return [...base, ...extra]; },
  };
  const contracts = arrayAfter(tokens, ['var', 'commandContracts', '='], helpers);
  assert(contracts.length === 84, 'expected 84 Go command contracts');
  unique(contracts.map(c => c.Definition.Path), 'Go command');
  const commands = contracts.map(c => {
    checkedKeys(c, ['Definition', 'flags', 'positional', 'allowsHome'], 'command');
    const d = c.Definition;
    checkedKeys(d, ['Path', 'Summary', 'Mutates'], 'definition');
    const own = c.flags ?? [];
    const flags = [];
    const add = (name, display, value = false) => flags.push({ name, display, value });
    if (c.allowsHome) add('--home', '--home <root>', true);
    add('--format', '--format text|json|jsonl', true);
    const project = d.Path !== 'live connect' && !d.Path.startsWith('project ') && own.some(f => f.name === '--snapshot');
    if (project) add('--project', '--project <directory>', true);
    for (const f of own) {
      checkedKeys(f, ['name', 'display', 'value'], 'flag');
      add(f.name, f.display, f.value ?? false);
    }
    if (d.Path.startsWith('data ') && d.Path !== 'data hotfix') {
      add('--timeout-seconds', '--timeout-seconds <1-3600> (default 300; entire query)', true);
    } else if (['live connect', 'live execute', 'live disconnect', 'live status', 'live resume', 'live session', 'live reload', 'live reload fallback', 'live bugs'].includes(d.Path)) {
      add('--project', '--project <directory>', true);
      if (!['live status', 'live session'].includes(d.Path)) {
        add('--wait-seconds', '--wait-seconds <1-600> (default 120)', true); add('--no-cache', '--no-cache');
      }
      if (d.Path === 'live execute') add('--policy', '--policy <opaque|observation> (default opaque)', true);
    }
    unique(flags.map(f => f.name), `flag in ${d.Path}`);
    return {
      path: d.Path,
      summary: d.Summary + (project ? '; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins' : ''),
      mutates: d.Mutates,
      arguments: c.positional ? [c.positional] : [],
      flags: flags.map(f => f.display),
      parserFlags: flags,
    };
  });
  return { commands, products: products.map(p => ({ product: p.Product, family: p.Family, productCode: p.ProductCode, folder: p.Folder, buildSeries: p.BuildSeries, reusedSlot: p.ReusedSlot ?? false })) };
}

function git(root, args, maxBuffer = LIMITS.fileBytes) {
  return execFileSync('git', ['--no-pager', '--no-optional-locks', '-c', 'core.fsmonitor=false', ...args], { cwd: root, maxBuffer, timeout: 10000, stdio: ['ignore', 'pipe', 'pipe'] });
}
export function buildFixture(root) {
  const blobs = Object.fromEntries(Object.entries(SOURCES).map(([key, path]) => [key, git(root, ['cat-file', 'blob', `${GO_COMMIT}:${path}`])]));
  const extracted = extractPinnedGo(blobs.commands, blobs.products);
  const tree = git(root, ['ls-tree', '-rz', GO_COMMIT, '--', 'addon']).toString('utf8').split('\0').filter(Boolean);
  assert(tree.length === 55, 'expected 55 pinned addon files');
  let total = 0;
  const addon = tree.map(entry => {
    const match = /^(100644|100755) blob ([0-9a-f]{40})\t(addon\/[^\r\n]+)$/.exec(entry);
    assert(match, 'unexpected addon Git tree entry or non-regular file');
    const path = match[3]; checkRelative(path);
    const bytes = git(root, ['cat-file', 'blob', match[2]]);
    total += bytes.length; assert(total <= LIMITS.totalAddonBytes, 'addon byte limit');
    return { path, bytes: bytes.length, sha256: sha256(bytes) };
  });
  const goTestTree = git(root, ['ls-tree', '-rz', GO_COMMIT]).toString('utf8').split('\0').filter(entry => /\t[^\r\n]+_test\.go$/.test(entry));
  assert(goTestTree.length === 359, 'expected 359 fixed Go test source files');
  let testBytes = 0;
  const goTests = goTestTree.map(entry => {
    const match = /^(100644|100755) blob ([0-9a-f]{40})\t([^\r\n]+_test\.go)$/.exec(entry);
    assert(match, 'unexpected Go test tree entry'); checkRelative(match[3]);
    const bytes = git(root, ['cat-file', 'blob', match[2]]);
    testBytes += bytes.length; assert(testBytes <= LIMITS.totalAddonBytes, 'Go test inventory byte limit');
    return { path: match[3], bytes: bytes.length, sha256: sha256(bytes) };
  });
  return {
    schema: 'lycheedev.rust-static-oracle.v1', oracle: { release: 'Go v3.2.0', commit: GO_COMMIT },
    extraction: 'Bounded literal parsing plus hash-locked adapters for publicDefinition/sharedFlags/projectSelection. No Go, Lua, Rust or repository config execution.',
    sources: Object.entries(SOURCES).map(([key, path]) => ({ path, bytes: blobs[key].length, sha256: sha256(blobs[key]) })),
    ...extracted, addon,
    goTestReference: { scope: 'All tracked *_test.go files at the fixed Go commit; helper fixtures and non-Go harnesses require separate dependency mapping.', mappingState: 'unreviewed', files: goTests },
  };
}
function checkRelative(path) {
  assert(typeof path === 'string' && !/[\\:\x00-\x1f]/.test(path) && path.split('/').every(s => s && s !== '.' && s !== '..'), 'unsafe relative path');
}
export function readBounded(root, path) {
  checkRelative(path);
  let current = resolve(root);
  for (const component of path.split('/')) {
    current = resolve(current, component);
    assert(!lstatSync(current).isSymbolicLink(), `symlink refused: ${path}`);
  }
  const stat = lstatSync(current);
  assert(stat.isFile() && stat.size <= LIMITS.fileBytes, `not bounded regular file: ${path}`);
  const bytes = readFileSync(current);
  assert(bytes.length <= LIMITS.fileBytes, `file grew past limit: ${path}`);
  return bytes;
}
export function addonInventory(root) {
  const files = []; let total = 0; let entries = 0;
  function walk(path, depth) {
    assert(depth <= LIMITS.depth, 'addon nesting limit');
    checkRelative(path);
    const full = resolve(root, path);
    const stat = lstatSync(full);
    assert(!stat.isSymbolicLink(), `addon symlink refused: ${path}`);
    assert(++entries <= LIMITS.files, 'addon entry limit');
    if (stat.isDirectory()) for (const child of readdirSync(full).sort()) walk(`${path}/${child}`, depth + 1);
    else {
      const bytes = readBounded(root, path);
      total += bytes.length; assert(total <= LIMITS.totalAddonBytes, 'addon total byte limit');
      files.push({ path, bytes: bytes.length, sha256: sha256(bytes) });
    }
  }
  walk('addon', 0); return files.sort((a, b) => a.path.localeCompare(b.path, 'en'));
}
export const RUST_FILES = Object.freeze({
  inventory: 'crates/lycheedev-cli/src/commands.json', filter: 'crates/lycheedev-cli/src/contract.rs',
  products: 'crates/lycheedev-core/src/client.rs', sourceCatalog: 'crates/lycheedev-source/src/catalog.rs',
  source: 'crates/lycheedev-source/src/lib.rs', data: 'crates/lycheedev-data/src/lib.rs', live: 'crates/lycheedev-live/src/lib.rs',
  delivery: 'crates/lycheedev-delivery/src/lib.rs', workspace: 'crates/lycheedev-core/src/workspace.rs',
  evidence: 'crates/lycheedev-core/src/evidence.rs', cache: 'crates/lycheedev-core/src/cache.rs',
});
export function parseRegistrations(source) {
  const body = functionBody(lex(source), 'commands');
  assert(body[0] === '&' && body[1] === '[', 'commands() must be literal slice');
  const parser = new Literals(body.slice(2));
  const result = parser.list(']');
  assert(parser.i === body.length - 2, 'commands() contains nonliteral statements');
  assert(result.every(v => typeof v === 'string'), 'non-string registration'); unique(result, 'registration'); return result;
}
export function extractAdvertised(inventory, texts) {
  if (sha256(texts.filter) !== RUST_FILTER_HASH) return { state: 'unverified', reason: 'contract.rs changed; review the static advertised-filter adapter', commands: null };
  const registrations = Object.fromEntries(['source', 'data', 'live', 'delivery', 'workspace', 'evidence', 'cache'].map(key => [key, parseRegistrations(texts[key].toString('utf8'))]));
  const commands = inventory.filter(d => {
    if (['version', 'describe', 'project init', 'project status', 'project lock', 'target available'].includes(d.path)) return true;
    const family = d.path.startsWith('source ') ? 'source' : /^(data|asset) /.test(d.path) ? 'data' : d.path.startsWith('live ') ? 'live' : /^(addon|skill) /.test(d.path) || d.path === 'update' ? 'delivery' : null;
    return family ? registrations[family].includes(d.path) : ['workspace', 'evidence', 'cache'].some(key => registrations[key].includes(d.path));
  });
  return { state: 'static-extracted', commands, registrations, behavior: 'unverified: registration does not establish handler correctness or availability' };
}
// The whole containing source is authenticated before this narrow match adapter.
export function extractArities(source) {
  if (sha256(source) !== RUST_FILTER_HASH) return null;
  const text = source.toString('utf8');
  const body = text.match(/pub fn takes_value\(&self\) -> bool \{[\s\S]*?match self\.name\(\) \{([\s\S]*?)\n        \}/)?.[1];
  assert(body, 'explicit arity implementation missing');
  const pattern = /((?:\s*"--[a-z-]+"\s*\|?)+)\s*=>\s*(?:\{\s*(false|true)\s*\}|(false|true)),?/g;
  const arms = [...body.matchAll(pattern)];
  assert(arms.length === 2, 'explicit arity arms changed');
  const result = Object.create(null);
  for (const [, names, braced, plain] of arms) for (const [, name] of names.matchAll(/"(--[a-z-]+)"/g)) {
    assert(!Object.hasOwn(result, name), 'duplicate explicit arity');
    result[name] = (braced ?? plain) === 'true';
  }
  assert(body.replace(pattern, '').trim() === 'name => panic!("flag missing explicit contract arity: {name}"),', 'unreviewed arity fallback');
  return result;
}
export function extractRustProducts(source) {
  const tokens = lex(source);
  const start = locate(tokens, ['pub', 'static', 'PRODUCTS', ':', '[', 'Product', ';']);
  const count = Number(tokens[start]);
  assert(Number.isInteger(count) && count <= 100, 'invalid Rust product count');
  const parser = new Literals(tokens.slice(start + 4), { Some: value => value, None: () => null });
  // Rust None is a literal enum variant, not a call.
  parser.tokens = parser.tokens.flatMap(t => t === 'None' ? ['None', '(', ')'] : [t]);
  const products = parser.list(']');
  assert(products.length === count, 'Rust product count mismatch');
  unique(products.map(p => p.id), 'Rust product');
  return products;
}
export function extractSourceTracks(source) {
  const tokens = lex(source);
  const start = locate(tokens, [{ string: 'wow-ui-source' }, ',', { string: 'World of Warcraft UI Source Mirror' }, ',', { string: 'https://github.com/Gethe/wow-ui-source.git' }, ',', 'vec', '!', '[']);
  const tracks = Object.create(null); let i = start;
  while (tokens[i] !== ']') {
    assert(tokens[i++] === '(', 'expected source track tuple');
    const key = tokens[i++]; assert(key && typeof key === 'object', 'expected track string');
    assert(tokens[i++] === ',', 'expected track separator');
    const value = tokens[i++]; assert(value && typeof value === 'object', 'expected ref string');
    assert(tokens[i++] === ')' && tokens[i++] === ',', 'expected track end');
    assert(!Object.hasOwn(tracks, key.string), 'duplicate source track'); tracks[key.string] = value.string;
  }
  return tracks;
}
export function loadCandidate(root) {
  const texts = Object.fromEntries(Object.entries(RUST_FILES).map(([key, path]) => [key, readBounded(root, path)]));
  const inventory = JSON.parse(texts.inventory.toString('utf8'));
  validateCommands(inventory);
  return {
    inventory, advertised: extractAdvertised(inventory, texts), arities: extractArities(texts.filter), products: extractRustProducts(texts.products.toString('utf8')),
    sourceTracks: extractSourceTracks(texts.sourceCatalog.toString('utf8')), addon: addonInventory(root),
    sources: Object.entries(RUST_FILES).map(([key, path]) => ({ path, bytes: texts[key].length, sha256: sha256(texts[key]) })),
  };
}
export function validateCommands(commands) {
  assert(Array.isArray(commands) && commands.length <= 1000, 'invalid command inventory');
  for (const c of commands) {
    assert(c && typeof c.path === 'string' && typeof c.summary === 'string' && typeof c.mutates === 'boolean', 'invalid command definition');
    for (const key of ['flags', 'arguments']) assert(Array.isArray(c[key]) && c[key].every(v => typeof v === 'string'), `invalid ${key}`);
    unique(c.flags.map(f => f.split(/\s+/)[0]), `flag in ${c.path}`);
  }
  unique(commands.map(c => c.path), 'command');
}
export function diffByKey(expected, actual, key, fields) {
  unique(expected.map(v => v[key]), `expected ${key}`); unique(actual.map(v => v[key]), `actual ${key}`);
  const left = new Map(expected.map(v => [v[key], v])); const right = new Map(actual.map(v => [v[key], v]));
  const missing = [...left.keys()].filter(k => !right.has(k)).sort();
  const extra = [...right.keys()].filter(k => !left.has(k)).sort();
  const changed = [];
  for (const [name, want] of left) {
    if (!right.has(name)) continue;
    const got = right.get(name);
    const changes = fields.filter(f => !same(want[f], got[f])).map(field => ({ field, expected: want[field], actual: got[field] }));
    if (changes.length) changed.push({ [key]: name, changes });
  }
  return { state: missing.length || extra.length || changed.length ? 'fail' : 'pass', expectedCount: expected.length, actualCount: actual.length, missing, extra, changed };
}
export function compare(oracle, candidate) {
  validateCommands(oracle.commands); validateCommands(candidate.inventory);
  const commands = diffByKey(oracle.commands, candidate.inventory, 'path', ['summary', 'mutates', 'arguments']);
  const flags = diffByKey(oracle.commands, candidate.inventory, 'path', ['flags']);
  // Mirror Flag::takes_value only while its containing source is hash-locked.
  const parserFlagArity = candidate.advertised.commands === null || !candidate.arities ? { state: 'unverified', reason: 'Rust Flag::takes_value adapter needs review' } : diffByKey(
    oracle.commands.map(c => ({ path: c.path, arity: c.parserFlags.map(f => ({ name: f.name, value: f.value })) })),
    candidate.inventory.map(c => ({ path: c.path, arity: c.flags.map(display => {
      const [name] = display.split(/\s+/);
      assert(Object.hasOwn(candidate.arities, name), `unmapped Rust flag arity: ${name}`);
      return { name, value: candidate.arities[name] };
    }) })), 'path', ['arity']);
  const advertised = candidate.advertised.commands === null ? candidate.advertised : diffByKey(oracle.commands, candidate.advertised.commands, 'path', []);
  const products = diffByKey(oracle.products.map(p => ({ id: p.product, catalog_product: p.productCode, series: p.buildSeries })), candidate.products, 'id', ['catalog_product', 'series']);
  const addon = diffByKey(oracle.addon, candidate.addon, 'path', ['bytes', 'sha256']);
  const checks = { commands, flags, parserFlagArity, advertised, products, addon };
  const status = Object.values(checks).some(c => c.state === 'fail') ? 'fail' : Object.values(checks).some(c => c.state === 'unverified') ? 'unverified' : 'pass';
  return {
    schema: 'lycheedev.rust-static-differential.v1', oracle: oracle.oracle, staticStatus: status,
    goTestReference: { fileCount: oracle.goTestReference?.files.length ?? 0, mappingState: 'unreviewed', reason: 'Pinned test bytes are a reference inventory, not translated Rust coverage or executed test evidence.' },
    behavioralParity: 'unverified', scope: 'Static declarations and addon bytes only; no binaries, runtime tests, game operations or performance measurements.',
    ...checks,
    sourceFamilyMapping: { state: 'unverified', expected: oracle.products.map(p => ({ product: p.product, family: p.family })), availableRustSourceTracks: candidate.sourceTracks, reason: 'Catalog tracks alone do not prove regional product-to-family resolution; runtime selection and cross-region isolation remain unverified.' },
    productSemantics: { state: 'unverified', reason: 'Folder discovery, reused-slot guards, API eligibility and client metadata validation are not established by catalog literals.' },
    registrationSemantics: { state: 'unverified', reason: 'Advertised commands are statically reconstructed from contract.rs and commands() slices; registration is not execution evidence.' },
    candidateSources: candidate.sources ?? [],
  };
}
