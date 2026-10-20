import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, symlinkSync, rmSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { resolve, dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync, spawnSync } from 'node:child_process';
import {
  GO_COMMIT, LIMITS, RUST_FILTER_HASH, RUST_FILES, sha256, lex, parseLiteral, parseRegistrations, extractRustProducts,
  extractSourceTracks, extractPinnedGo, extractAdvertised, extractArities, buildFixture, compare,
  validateCommands, diffByKey, readBounded, addonInventory, loadCandidate,
} from './oracle.mjs';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
// Adapter unit tests exercise the exact reviewed input, not the evolving
// candidate. Production loadCandidate still reads and authenticates the worktree.
const reviewedRustCommit = 'c508b10ef0e651fc8721d251feef5d25e41a0454';
function reviewedRustContract() {
  const source = execFileSync('git', ['--no-replace-objects', '--no-pager', '--no-optional-locks',
    '-c', 'core.fsmonitor=false', 'cat-file', 'blob', `${reviewedRustCommit}:${RUST_FILES.filter}`],
  { cwd: root, maxBuffer: LIMITS.fileBytes, timeout: 10000, stdio: ['ignore', 'pipe', 'pipe'] });
  assert.equal(sha256(source), RUST_FILTER_HASH, 'historical adapter input identity');
  return source;
}
const clone = value => structuredClone(value);
const definition = (path = 'inspect') => ({ path, summary: 'Inspect only', mutates: false, arguments: [], flags: ['--format text|json|jsonl', '--read-only'], parserFlags: [{ name: '--format', display: '--format text|json|jsonl', value: true }, { name: '--read-only', display: '--read-only', value: false }] });
function fixture() {
  const commands = [definition()];
  const oracle = { oracle: { release: 'controlled fixture', commit: GO_COMMIT }, commands, products: [{ product: 'test', family: 'test', productCode: 'wow_test', folder: '_test_', buildSeries: '1.0', reusedSlot: false }], addon: [{ path: 'addon/a.lua', bytes: 1, sha256: 'a'.repeat(64) }] };
  const candidate = { inventory: clone(commands), arities: { '--format': true, '--read-only': false }, advertised: { state: 'static-extracted', commands: clone(commands) }, products: [{ id: 'test', catalog_product: 'wow_test', series: '1.0' }], addon: clone(oracle.addon), sourceTracks: { test: 'test' } };
  return { oracle, candidate };
}
test('controlled exact static match passes without claiming behavioral parity', () => {
  const { oracle, candidate } = fixture(); const result = compare(oracle, candidate);
  assert.equal(result.staticStatus, 'pass');
  for (const key of ['commands', 'flags', 'parserFlagArity', 'advertised', 'products', 'addon']) assert.equal(result[key].state, 'pass');
  assert.equal(result.behavioralParity, 'unverified'); assert.equal(result.sourceFamilyMapping.state, 'unverified');
});
test('missing and extra commands fail even when counts are equal', () => {
  const { oracle, candidate } = fixture(); candidate.inventory[0].path = 'replacement';
  const result = compare(oracle, candidate);
  assert.equal(result.staticStatus, 'fail'); assert.deepEqual(result.commands.missing, ['inspect']); assert.deepEqual(result.commands.extra, ['replacement']);
});
test('command metadata, arguments and flag displays each retain exact differences', () => {
  const { oracle, candidate } = fixture();
  Object.assign(candidate.inventory[0], { summary: 'Changed', mutates: true, arguments: ['<new>'], flags: ['--format json', '--read-only'] });
  const result = compare(oracle, candidate);
  assert.deepEqual(result.commands.changed[0].changes.map(c => c.field), ['summary', 'mutates', 'arguments']);
  assert.equal(result.flags.changed.length, 1);
  assert.equal(result.parserFlagArity.state, 'pass', 'help prose must not invent arity');
});
test('advertised surface is distinct from complete inventory and handler proof', () => {
  const { oracle, candidate } = fixture(); candidate.advertised.commands = [];
  const result = compare(oracle, candidate);
  assert.equal(result.commands.state, 'pass'); assert.equal(result.advertised.state, 'fail');
  assert.deepEqual(result.advertised.missing, ['inspect']); assert.equal(result.registrationSemantics.state, 'unverified');
});
test('legacy product IDs are not normalized into canonical regional profiles', () => {
  const { oracle, candidate } = fixture();
  oracle.products[0].product = 'forever-asia-beta'; candidate.products[0].id = 'forever';
  const result = compare(oracle, candidate);
  assert.equal(result.products.state, 'fail'); assert.deepEqual(result.products.missing, ['forever-asia-beta']); assert.deepEqual(result.products.extra, ['forever']);
});
test('product identity and build-series changes fail independently of equal IDs', () => {
  const { oracle, candidate } = fixture(); Object.assign(candidate.products[0], { catalog_product: 'wrong', series: '9.9' });
  assert.deepEqual(compare(oracle, candidate).products.changed[0].changes.map(c => c.field), ['catalog_product', 'series']);
});
test('addon hash, byte count and path differences all fail', () => {
  const { oracle, candidate } = fixture(); candidate.addon[0].sha256 = 'b'.repeat(64); candidate.addon[0].bytes = 2;
  candidate.addon.push({ path: 'addon/extra.lua', bytes: 0, sha256: 'c'.repeat(64) });
  oracle.addon.push({ path: 'addon/missing.lua', bytes: 0, sha256: 'd'.repeat(64) });
  const result = compare(oracle, candidate);
  assert.equal(result.addon.state, 'fail'); assert.deepEqual(result.addon.missing, ['addon/missing.lua']); assert.deepEqual(result.addon.extra, ['addon/extra.lua']);
  assert.deepEqual(result.addon.changed[0].changes.map(c => c.field), ['bytes', 'sha256']);
});
test('changed advertised-filter source becomes unverified, never an empty pass', () => {
  const result = extractAdvertised([definition()], { filter: Buffer.from('changed') });
  assert.equal(result.state, 'unverified'); assert.equal(result.commands, null);
  const { oracle, candidate } = fixture(); candidate.advertised = result;
  const report = compare(oracle, candidate); assert.equal(report.staticStatus, 'unverified'); assert.equal(report.parserFlagArity.state, 'unverified');
});
test('duplicates and malformed inventories fail closed', () => {
  assert.throws(() => validateCommands([definition(), definition()]), /duplicate/);
  assert.throws(() => validateCommands([{ ...definition(), flags: ['--x', '--x <n>'] }]), /duplicate/);
  assert.throws(() => validateCommands([{ ...definition(), mutates: 'false' }]), /invalid/);
  assert.throws(() => diffByKey([{ path: 'x' }, { path: 'x' }], [], 'path', []), /duplicate/);
});
test('literal parser accepts comments and quoted braces but only whitelisted helpers', () => {
  assert.deepEqual(JSON.parse(JSON.stringify(parseLiteral('Definition{Path: "a}b", /* fake Path: "x" */ Mutates: false}'))), { Path: 'a}b', Mutates: false });
  assert.equal(parseLiteral('approved("literal")', { approved: s => s }), 'literal');
  for (const source of ['execute("danger")', 'process.exit()', '{a: true, a: false}', '"unterminated', '/* unterminated', 'true false']) assert.throws(() => parseLiteral(source));
});
test('bounded parser rejects oversized and overnested input', () => {
  assert.throws(() => lex(' '.repeat(LIMITS.fileBytes + 1)), /byte limit/);
  assert.throws(() => parseLiteral('{a:'.repeat(LIMITS.depth + 1) + 'true' + '}'.repeat(LIMITS.depth + 1)), /depth limit/);
});
test('registration parser rejects computed entries and trailing statements', () => {
  assert.deepEqual(parseRegistrations('pub fn commands() -> &\'static [&\'static str] { &["a", // fake "evil"\n "b",] }'), ['a', 'b']);
  assert.throws(() => parseRegistrations('pub fn commands() { build() }'), /literal slice/);
  assert.throws(() => parseRegistrations('pub fn commands() { &["a"]; execute() }'), /nonliteral/);
  assert.throws(() => parseRegistrations('pub fn commands() { &["a", "a"] }'), /duplicate/);
});
test('Rust product literals preserve legacy IDs and unknown interface', () => {
  const source = 'pub static PRODUCTS: [Product; 1] = [Product { id: "cn-beta", display_name: "CN", catalog_product: "wow_cn_beta", series: "1.60.1", channel: "cn-beta", interface: None, },];';
  const [product] = extractRustProducts(source); assert.equal(product.id, 'cn-beta'); assert.equal(product.interface, null);
  assert.throws(() => extractRustProducts(source.replace('None', 'danger()')), /unsupported/);
});
test('source tracks are data; malformed catalogs require review', () => {
  const source = '[ ("wow-ui-source", "World of Warcraft UI Source Mirror", "https://github.com/Gethe/wow-ui-source.git", vec![("forever", "forever"),]), ]';
  assert.equal(extractSourceTracks(source).forever, 'forever');
  assert.throws(() => extractSourceTracks(source.replace('vec!', 'lookup!')));
});
test('Go extraction rejects source mutation instead of trusting a moving source', () => {
  assert.throws(() => extractPinnedGo(Buffer.from('not fixed Go'), Buffer.from('anything')), /hash/);
});
test('filesystem inventory never runs Lua and rejects traversal', () => {
  const temp = mkdtempSync(join(tmpdir(), 'lycheedev-static-parity-'));
  try {
    mkdirSync(join(temp, 'addon')); writeFileSync(join(temp, 'addon/evil.lua'), 'os.execute("must never run")');
    writeFileSync(join(temp, 'addon/new.txt'), 'extra'); assert.equal(addonInventory(temp).length, 2);
    assert.throws(() => readBounded(temp, '../outside'), /unsafe/);
    assert.throws(() => readBounded(temp, 'addon\\evil.lua'), /unsafe/);
  } finally { rmSync(temp, { recursive: true, force: true }); }
});
test('filesystem inventory and bounded read reject actual file symlinks', t => {
  const temp = mkdtempSync(join(tmpdir(), 'lycheedev-static-file-link-'));
  try {
    mkdirSync(join(temp, 'addon')); writeFileSync(join(temp, 'addon/evil.lua'), 'return "not executed"');
    try { symlinkSync('evil.lua', join(temp, 'addon/link.lua'), 'file'); }
    catch (error) {
      if (process.platform === 'win32' && ['EPERM', 'EACCES'].includes(error.code)) {
        t.skip(`Windows denied creating this fixture file symlink (${error.code}); rejection behavior is unmeasured`);
        return;
      }
      throw error;
    }
    assert.throws(() => addonInventory(temp), /symlink/);
    assert.throws(() => readBounded(temp, 'addon/link.lua'), /symlink/);
  } finally { rmSync(temp, { recursive: true, force: true }); }
});
test('Windows junction ancestor is rejected independently of file symlink support', { skip: process.platform !== 'win32' }, () => {
  const temp = mkdtempSync(join(tmpdir(), 'lycheedev-static-junction-'));
  try {
    mkdirSync(join(temp, 'addon')); mkdirSync(join(temp, 'target'));
    writeFileSync(join(temp, 'target/value.lua'), 'return "not executed"');
    symlinkSync(join(temp, 'target'), join(temp, 'addon/link'), 'junction');
    assert.throws(() => addonInventory(temp), /symlink/);
    assert.throws(() => readBounded(temp, 'addon/link/value.lua'), /symlink/);
  } finally { rmSync(temp, { recursive: true, force: true }); }
});
test('manifest regenerates byte-for-byte from fixed Go objects, with shared flags', () => {
  const oracle = buildFixture(root);
  assert.equal(JSON.stringify(oracle, null, 2) + '\n', readFileSync(join(root, 'tests/rust-parity/go-3.2.0.manifest.json'), 'utf8'));
  assert.equal(oracle.commands.length, 84); assert.equal(oracle.products.length, 7); assert.equal(oracle.addon.length, 55); assert.equal(oracle.goTestReference.files.length, 359);
  const byPath = Object.fromEntries(oracle.commands.map(c => [c.path, c]));
  assert.deepEqual(byPath['live doctor'].flags.slice(0, 5), ['--format text|json|jsonl', '--installation <client>', '--pid <pid>', '--session <CON-id>', '--project <directory>']);
  assert.ok(byPath['live doctor'].flags.includes('--wait-seconds <1-600> (default 30)'));
  assert.ok(byPath['live execute'].flags.includes('--policy <opaque|observation> (default opaque)'));
  assert.ok(byPath['data db2 schema'].flags.includes('--timeout-seconds <1-3600> (default 300; entire query)'));
  assert.ok(!byPath['data hotfix'].flags.some(f => f.startsWith('--timeout-seconds')));
  assert.ok(byPath['target add'].flags.includes('--product <retail|classic|classic-era|titan|forever-asia-beta|forever-cn-beta|anniversary> (legacy: forever)'));
  assert.deepEqual(byPath['source batch'].arguments, []); assert.deepEqual(byPath['source query'].arguments, ['<term>']);
  assert.equal(oracle.products.find(p => p.product === 'forever-cn-beta').family, 'forever');
});
test('CLI invalid invocation exits 2 rather than silently accepting gaps', () => {
  const result = spawnSync(process.execPath, ['tests/rust-parity/run.mjs', '--accept-gaps'], { cwd: root, encoding: 'utf8', timeout: 20000 });
  assert.equal(result.status, 2); assert.match(result.stderr, /Usage:/);
});

test('explicit arity differences fail independently of help prose', () => {
  const { oracle, candidate } = fixture(); candidate.arities['--format'] = false;
  assert.equal(compare(oracle, candidate).parserFlagArity.changed[0].changes[0].actual[0].value, false);
  delete candidate.arities['--format'];
  assert.throws(() => compare(oracle, candidate), /unmapped Rust flag arity/);
  assert.equal(extractArities(Buffer.from('changed source')), null);
});

test('reviewed historical source is exact and every frozen Go flag has matching explicit arity', () => {
  const source = reviewedRustContract();
  assert.equal(sha256(source), RUST_FILTER_HASH, 'contract.rs changed; review both semantic adapters');
  const arities = extractArities(source);
  const expected = Object.create(null);
  for (const command of buildFixture(root).commands) for (const flag of command.parserFlags) {
    if (Object.hasOwn(expected, flag.name)) assert.equal(expected[flag.name], flag.value, `${flag.name} has conflicting Go arities`);
    expected[flag.name] = flag.value;
    assert.equal(arities[flag.name], flag.value, `${command.path}: ${flag.name}`);
  }
  assert.equal(Object.keys(expected).length, 115);
  assert.deepEqual(arities, expected, 'extra or missing match arms require review');
});

test('reviewed advertised adapter preserves builtins, ordered family routing and inventory intersection', () => {
  const registrations = {
    source: ['source approved', 'registered only'],
    data: ['data approved', 'asset approved'],
    live: ['live approved'],
    delivery: ['addon approved', 'skill approved', 'update', 'update extra'],
    workspace: ['target approved', 'source', 'data', 'asset', 'live', 'addon', 'skill',
      'source wrong-family', 'data wrong-family', 'asset wrong-family', 'live wrong-family', 'addon wrong-family', 'skill wrong-family'],
    evidence: ['evidence approved'],
    cache: ['cache approved'],
  };
  const texts = { filter: reviewedRustContract() };
  for (const [key, names] of Object.entries(registrations)) {
    texts[key] = Buffer.from(`pub fn commands() -> &'static [&'static str] { &[${names.map(JSON.stringify).join(',')}] }`);
  }
  const accepted = ['version', 'describe', 'project init', 'project status', 'project lock', 'target available',
    'source approved', 'data approved', 'asset approved', 'live approved', 'addon approved', 'skill approved', 'update',
    'target approved', 'evidence approved', 'cache approved', 'source', 'data', 'asset', 'live', 'addon', 'skill'];
  const rejected = ['source wrong-family', 'data wrong-family', 'asset wrong-family', 'live wrong-family',
    'addon wrong-family', 'skill wrong-family', 'update extra', 'not registered'];
  const inventory = [...accepted, ...rejected].reverse().map(path => definition(path));
  const result = extractAdvertised(inventory, texts);
  assert.equal(result.state, 'static-extracted');
  assert.deepEqual(result.commands.map(c => c.path), [...accepted].reverse());
  assert.deepEqual(result.registrations, registrations);
  // `update` is routed to Delivery, not an unconditional builtin.
  texts.delivery = Buffer.from('pub fn commands() { &[] }');
  assert.ok(!extractAdvertised(inventory, texts).commands.some(c => c.path === 'update'));
});

test('arity, advertised, and test-only source mutations all invalidate both hash-locked adapters', () => {
  const source = reviewedRustContract().toString('utf8');
  const mutations = [
    source.replace('"--wait-seconds" => true,', '"--wait-seconds" => false,'),
    source.replace('"target available" => true,', '"target available" => false,'),
    source.replace('assert_eq!(seen.len(), 115);', 'assert_eq!(seen.len(), 114);'),
    `${source}\n// unreviewed trailing edit\n`,
  ];
  for (const changed of mutations) {
    assert.notEqual(changed, source, 'mutation must actually alter the source');
    const filter = Buffer.from(changed);
    assert.equal(extractArities(filter), null);
    const result = extractAdvertised([definition()], { filter });
    assert.equal(result.state, 'unverified');
    assert.equal(result.commands, null);
  }
});

test('candidate loading never substitutes reviewed historical bytes for a changed worktree', () => {
  const temp = mkdtempSync(join(tmpdir(), 'lycheedev-candidate-contract-'));
  try {
    for (const path of Object.values(RUST_FILES)) {
      mkdirSync(dirname(join(temp, path)), { recursive: true });
      writeFileSync(join(temp, path), readBounded(root, path));
    }
    const changed = Buffer.concat([reviewedRustContract(), Buffer.from('\n// unreviewed candidate edit\n')]);
    writeFileSync(join(temp, RUST_FILES.filter), changed);
    mkdirSync(join(temp, 'addon'));
    writeFileSync(join(temp, 'addon/fixture.lua'), 'return "not executed"');
    const candidate = loadCandidate(temp);
    assert.equal(candidate.sources.find(row => row.path === RUST_FILES.filter).sha256, sha256(changed));
    assert.equal(candidate.advertised.state, 'unverified');
    assert.equal(candidate.advertised.commands, null);
    assert.equal(candidate.arities, null);
  } finally { rmSync(temp, { recursive: true, force: true }); }
});

test('Rust Unicode scalar literals decode without reinterpreting escaped backslashes', () => {
  assert.equal(parseLiteral('"\\u{feff}"'), '\ufeff');
  assert.equal(parseLiteral('"\\u{1f600}"'), '😀');
  assert.equal(parseLiteral('"\\\\u{feff}"'), '\\u{feff}');
  for (const value of ['"\\u{d800}"', '"\\u{110000}"', '"\\u{zz}"']) assert.throws(() => parseLiteral(value));
});

test('Rust NUL escapes decode without reinterpreting escaped or raw backslashes', () => {
  assert.equal(parseLiteral('"\\0wow\\0"'), '\0wow\0');
  assert.equal(parseLiteral('"\\\\0wow\\\\0"'), '\\0wow\\0');
  assert.equal(parseLiteral('r"\\0wow\\0"'), '\\0wow\\0');
  assert.throws(() => parseLiteral('"\\1"'));
});

test('Rust raw strings preserve Windows paths and escape-like text verbatim', () => {
  for (const value of ['\\\\?\\UNC\\', '\\\\?\\', '\\n\\u{feff}\\u{zz}\\q', '', '原样😀']) {
    for (const hashes of ['', '#', '###']) {
      assert.equal(parseLiteral(`r${hashes}"${value}"${hashes}`), value);
    }
  }
  assert.deepEqual(lex('r"one"; r#"two"#;'), [{ string: 'one' }, ';', { string: 'two' }, ';']);
  assert.deepEqual(lex('r#type raw_name'), ['r', '#', 'type', 'raw_name']);
});

test('Rust raw string delimiters isolate quotes, comments and forged declarations', () => {
  const value = 'a"##b"#c"d // comment\n/* comment */ pub static PRODUCTS: [Product; 0] = [];';
  assert.equal(parseLiteral(`r###"${value}"###`), value);
  assert.deepEqual(lex('r#"value"##'), [{ string: 'value' }, '#']);
  assert.deepEqual(lex('// r#"unterminated\n/* r##"unterminated */ r"real"'), [{ string: 'real' }]);
  const source = 'pub static PRODUCTS: [Product; 1] = [Product { id: r#"real"#, },];';
  const forged = 'pub static PRODUCTS: [Product; 0] = [];';
  const input = `// ${forged}\n/* ${forged} */ const DECOY: &str = r#"${forged}"#; ${source}`;
  assert.deepEqual(extractRustProducts(input).map(p => p.id), ['real']);
});

test('Rust raw strings enforce delimiter, source and token bounds', () => {
  const hashes = '#'.repeat(255);
  assert.equal(parseLiteral(`r${hashes}"ok"${hashes}`), 'ok');
  assert.throws(() => lex(`r${hashes}#"invalid"${hashes}#`), /exceeds 255 hashes/);
  for (const value of ['r"unterminated', 'r#"unterminated"', 'r##"wrong"#', 'r###"wrong"##']) {
    assert.throws(() => lex(value), /unterminated Rust raw string/);
  }
  assert.equal(parseLiteral('r"line\r\nnext\nline"'), 'line\nnext\nline');
  assert.throws(() => lex('r"isolated\rcarriage return"'), /invalid carriage return/);
  assert.throws(() => lex('r"' + 'a'.repeat(LIMITS.fileBytes - 2) + '"'), /byte limit/);
  assert.throws(() => lex('r""'.repeat(LIMITS.tokens + 1)), /token limit/);
});

test('raw string support does not relax malformed ordinary strings or comments', () => {
  for (const value of ['"\\q"', '"\\U"', '"\\u{zz}"', '"unterminated', '"line\nnext"']) {
    assert.throws(() => lex(value));
  }
  assert.throws(() => lex('/* r"value"'), /unterminated comment/);
  assert.throws(() => lex('/* /* r"value" */ */'), /nested comments unsupported/);
  assert.throws(() => parseLiteral('r "value"'), /unsupported literal expression/);
});

test('real client source extracts products despite unrelated raw Windows path literals', () => {
  const source = readBounded(root, 'crates/lycheedev-core/src/client.rs').toString('utf8');
  assert.ok(lex(source).some(token => token?.string === '\\\\?\\UNC\\'));
  const products = extractRustProducts(source);
  assert.equal(products.length, 7);
  assert.deepEqual(products.map(p => p.id), ['retail', 'classic', 'classic-era', 'titan', 'forever-asia-beta', 'forever-cn-beta', 'anniversary']);
  // A malformed literal elsewhere in the containing source still fails closed.
  assert.throws(() => extractRustProducts(source + '\nconst BAD: &str = r##"unterminated"#;'), /unterminated Rust raw string/);
  assert.throws(() => extractRustProducts(source + '\nconst BAD: &str = "\\q";'));
});
