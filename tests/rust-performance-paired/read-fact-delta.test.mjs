import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { readFactDelta, readRustFact, rustFactPath, sha256 } from './read-fact-delta.mjs';

const readerPath = fileURLToPath(new URL('./read-fact-delta.mjs', import.meta.url));
const parser = 'fixture-parser';
const goJSON = value => JSON.stringify(value)
  .replace(/</g, '\\u003c').replace(/>/g, '\\u003e').replace(/&/g, '\\u0026')
  .replace(/\u2028/g, '\\u2028').replace(/\u2029/g, '\\u2029');
function sorted(value) {
  if (Array.isArray(value)) return value.map(sorted);
  if (value && typeof value === 'object') {
    return Object.fromEntries(Object.keys(value).sort().map(key => [key, sorted(value[key])]));
  }
  return value;
}

function fixture(rustLayout = 'v6') {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'lycheedev-fact-delta-'));
  const goHome = path.join(root, 'go');
  const rustHome = path.join(root, 'rust');
  const names = ['Test/a.lua', 'Blizzard_APIDocumentationGenerated/X.lua', 'Test/a.xml', 'Test/a.toc'];
  const documents = names.map((name, i) => ({
    kind: 'document', path: `Interface/AddOns/${name}`, sha256: sha256(`source-${i}`),
  }));
  const files = [];
  for (const [i, document] of documents.entries()) {
    const mode = path.extname(document.path) + (i === 1 ? ':generated-api' : '');
    const facts = {
      declarations: [{ name: 'Name<&>\u2028\u2029', category: 'function', line: 1, endLine: 2 }],
      relationships: [{ from: '<file>', to: 'Target', category: 'call', confidence: 'inferred', line: 2 }],
      loads: i === 2 ? [{ path: 'code.lua', line: 1, kind: 'script' }] : [],
      headers: i === 3 ? [{ key: 'Interface', value: '120100', line: 1 }] : [],
      diagnostics: [],
    };
    const goKey = sha256(`${parser}\0${mode}\0${document.sha256}`);
    const layoutKey = rustLayout === 'v11' ? 'facts-layout-v11-go323-generated-origins' : rustLayout === 'v10' ? 'facts-layout-v10-go323-xml-lexical' : rustLayout === 'v9' ? 'facts-layout-v9-go323-observed' : rustLayout === 'v8' ? 'facts-layout-v8-go323-malformed' : rustLayout === 'v7' ? 'facts-layout-v7-go323-public' : 'facts-layout-v6-xml-original-tags';
    const generator = ['v7', 'v8', 'v9', 'v10', 'v11'].includes(rustLayout) ? 'generator-v6' : 'generator-v5';
    const rustKey = sha256(`${layoutKey}\0tree-sitter-0.26.3\0`
      + `lua-0.5.0\0xml-0.38\0${generator}\0${document.path}\0${document.sha256}`);
    const goFile = path.join(goHome, 'source/v1/facts/files', parser, `${goKey}.json`);
    const rustFile = path.join(rustHome, 'cache/source-facts-rust', `${rustKey}.json`);
    for (const file of [goFile, rustFile]) fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(goFile, goJSON({
      schema: 'lycheedev.source-file-facts.v2', parser, mode,
      inputSHA256: document.sha256, factsSHA256: sha256(goJSON(facts)), facts,
    }));
    fs.writeFileSync(rustFile, JSON.stringify(sorted({
      schema: `lycheedev.source-facts.rust.${rustLayout}`, inputSHA256: document.sha256,
      factsSHA256: sha256(JSON.stringify(sorted(facts))), facts,
    })));
    files.push({ goFile, rustFile });
  }
  const records = path.join(goHome, 'source/v1/facts/snapshots/test/records.jsonl');
  fs.mkdirSync(path.dirname(records), { recursive: true });
  const raw = documents.map(row => JSON.stringify(row)).join('\n') + '\n';
  fs.writeFileSync(records, raw);
  const manifest = path.join(path.dirname(records), 'manifest.json');
  fs.writeFileSync(manifest, JSON.stringify({
    recordsHash: sha256(raw), recordBytes: Buffer.byteLength(raw),
    summary: { parser, documents: 4, declarations: 4, relationships: 4, diagnostics: 0 },
  }));
  return {
    root, goHome, rustHome, records, manifest, files, documents,
    read: () => readFactDelta(goHome, rustHome, records, rustLayout),
    changeRust(index, change) {
      const file = files[index].rustFile;
      const envelope = JSON.parse(fs.readFileSync(file));
      change(envelope.facts);
      envelope.factsSHA256 = sha256(JSON.stringify(sorted(envelope.facts)));
      fs.writeFileSync(file, JSON.stringify(sorted(envelope)));
    },
  };
}

async function withFixture(context, callback, rustLayout = 'v6') {
  const value = fixture(rustLayout);
  let passed = false;
  try {
    await callback(value);
    passed = true;
  } finally {
    if (passed) fs.rmSync(value.root, { recursive: true });
    else context.diagnostic(`Failed fixture retained: ${value.root}`);
  }
}

test('maps ordinary/generated Lua, XML and TOC to exact existing fact files', async t => {
  await withFixture(t, async f => {
    assert.ok(fs.existsSync(readerPath));
    const report = await f.read();
    assert.equal(report.documents, 4);
    assert.equal(report.byExtension['.lua'].go.declarations, 2);
    assert.equal(report.totals.go.loads, 1);
    assert.equal(report.totals.go.headers, 1);
    assert.equal(report.differentFiles.length, 0);
  });
});

test('authenticates original Go HTML/U2028/U2029 escaping without inventing empty fields', async t => {
  await withFixture(t, async f => {
    const raw = fs.readFileSync(f.files[0].goFile, 'utf8');
    assert.match(raw, /\\u003c/);
    assert.match(raw, /\\u2028/);
    assert.match(raw, /\\u2029/);
    assert.ok(!Object.hasOwn(JSON.parse(raw).facts.declarations[0], 'scope'));
    assert.equal((await f.read()).differentFiles.length, 0);
  });
});

test('authenticates recursively sorted Rust Value regardless of envelope key order', async t => {
  await withFixture(t, async f => {
    const file = f.files[0].rustFile;
    const envelope = JSON.parse(fs.readFileSync(file));
    envelope.facts.declarations[0] = Object.fromEntries(
      Object.entries(envelope.facts.declarations[0]).reverse(),
    );
    fs.writeFileSync(file, JSON.stringify(Object.fromEntries(Object.entries(envelope).reverse())));
    assert.equal((await f.read()).differentFiles.length, 0);
  });
});

test('reports different array order even when the exact multiset is equal', async t => {
  await withFixture(t, async f => {
    const file = f.files[0].goFile;
    const envelope = JSON.parse(fs.readFileSync(file));
    envelope.facts.relationships.push({ from: 'B', to: 'C', category: 'call', confidence: 'inferred', line: 3 });
    envelope.factsSHA256 = sha256(goJSON(envelope.facts));
    fs.writeFileSync(file, goJSON(envelope));
    const manifest = JSON.parse(fs.readFileSync(f.manifest));
    manifest.summary.relationships++;
    fs.writeFileSync(f.manifest, JSON.stringify(manifest));
    f.changeRust(0, facts => { facts.relationships = [...envelope.facts.relationships].reverse(); });
    const report = await f.read();
    const difference = report.firstDifferences[0];
    assert.equal(difference.sameMultisetButDifferentOrder, true);
    assert.deepEqual(difference.goOnly, []);
    assert.deepEqual(difference.rustOnly, []);
    assert.equal(report.differentFiles[0].exactRowsDiffer[0].sequenceEqual, false);
  });
});

test('retains category additions and duplicate row multiplicity in counts and samples', async t => {
  await withFixture(t, async f => {
    f.changeRust(0, facts => {
      facts.relationships.push(facts.relationships[0]);
      facts.relationships.push({ from: '<file>', to: 'Resolved', category: 'alias-call', confidence: 'static-resolved', line: 2 });
    });
    const report = await f.read();
    assert.equal(report.totals.rust.relationships, 6);
    assert.equal(report.categories['relationships:alias-call'].rust, 1);
    assert.equal(report.firstDifferences[0].rustOnly.length, 2);
    assert.equal(report.differentFiles[0].delta.relationships, 2);
  });
});

test('reports same-count original line and scope differences without normalizing them away', async t => {
  await withFixture(t, async f => {
    f.changeRust(2, facts => { facts.declarations[0].line = 3; facts.declarations[0].scope = '<file>'; });
    const report = await f.read();
    assert.equal(report.differentFiles[0].delta.declarations, 0);
    assert.equal(report.firstDifferences[0].goOnly[0].line, 1);
    assert.equal(report.firstDifferences[0].rustOnly[0].line, 3);
    assert.equal(report.firstDifferences[0].rustOnly[0].scope, '<file>');
  });
});

test('rejects modified Go records against the original manifest', async t => {
  await withFixture(t, async f => {
    fs.appendFileSync(f.records, '\n');
    await assert.rejects(f.read(), /Go records authentication mismatch/);
  });
});

test('rejects a modified Go facts payload whose stored digest is unchanged', async t => {
  await withFixture(t, async f => {
    const file = f.files[0].goFile;
    const envelope = JSON.parse(fs.readFileSync(file));
    envelope.facts.declarations[0].line = 50;
    fs.writeFileSync(file, goJSON(envelope));
    await assert.rejects(f.read(), /Go facts digest mismatch/);
  });
});

test('rejects a modified Rust facts payload whose stored digest is unchanged', async t => {
  await withFixture(t, async f => {
    const file = f.files[0].rustFile;
    const envelope = JSON.parse(fs.readFileSync(file));
    envelope.facts.declarations[0].line = 50;
    fs.writeFileSync(file, JSON.stringify(envelope));
    await assert.rejects(f.read(), /Rust facts digest mismatch/);
  });
});

test('never changes input bytes and leaves original acceptance authoritative', async t => {
  await withFixture(t, async f => {
    const files = [f.records, f.manifest, ...f.files.flatMap(item => [item.goFile, item.rustFile])];
    const before = files.map(file => sha256(fs.readFileSync(file)));
    const report = await f.read();
    assert.deepEqual(files.map(file => sha256(fs.readFileSync(file))), before);
    assert.equal(report.diagnosticOnly, true);
    assert.match(report.acceptanceUnchanged, /original comparison/);
  });
});


test('authenticates explicit v7 public facts without relabeling historical v6', async t => {
  await withFixture(t, async f => {
    const report = await f.read();
    assert.equal(report.inputs.rustLayout, 'v7');
    assert.equal(report.differentFiles.length, 0);
    await assert.rejects(readFactDelta(f.goHome, f.rustHome, f.records), /ENOENT/);
  }, 'v7');
});

test('rejects an unknown layout instead of discovering or mixing cache identities', async () => {
  await assert.rejects(readFactDelta('', '', '', 'latest'), /Unsupported Rust facts layout/);
});


test('authenticates explicit v8 malformed-contract facts without accepting v7 cache', async t => {
  await withFixture(t, async f => {
    const report = await f.read();
    assert.equal(report.inputs.rustLayout, 'v8');
    assert.equal(report.differentFiles.length, 0);
    await assert.rejects(readFactDelta(f.goHome, f.rustHome, f.records, 'v7'), /ENOENT/);
    await assert.rejects(readFactDelta(f.goHome, f.rustHome, f.records), /ENOENT/);
  }, 'v8');
});


test('authenticates v9 observed grammar corrections and rejects preceding v8 cache', async t => {
  await withFixture(t, async f => {
    const report = await f.read();
    assert.equal(report.inputs.rustLayout, 'v9');
    assert.equal(report.differentFiles.length, 0);
    await assert.rejects(readFactDelta(f.goHome, f.rustHome, f.records, 'v8'), /ENOENT/);
  }, 'v9');
});

test('reads explicit historical v10 without relabeling its key and schema', async t => {
  const key = 'facts-layout-v10-go323-xml-lexical\\0tree-sitter-0.26.3\\0lua-0.5.0\\0xml-0.38\\0generator-v6\\0{path}\\0{sha256}';
  await withFixture(t, async f => {
    for (const [index, document] of f.documents.entries()) {
      const preimage = key.replaceAll('\\0', '\0').replace('{path}', document.path).replace('{sha256}', document.sha256);
      assert.equal(path.basename(f.files[index].rustFile), `${sha256(preimage)}.json`);
    }
    const report = await f.read();
    assert.equal(report.inputs.rustLayout, 'v10');
    assert.equal(report.documents, 4);
    assert.equal(report.differentFiles.length, 0);
    for (const historical of ['v6', 'v7', 'v8', 'v9']) {
      await assert.rejects(readFactDelta(f.goHome, f.rustHome, f.records, historical), /ENOENT/);
    }
    await assert.rejects(readFactDelta(f.goHome, f.rustHome, f.records), /ENOENT/);
  }, 'v10');
});

test('never discovers or relabels a historical v9 cache as v10', async t => {
  await withFixture(t, async f => {
    await assert.rejects(readFactDelta(f.goHome, f.rustHome, f.records, 'v10'), /ENOENT/);
    const document = f.documents[0];
    const currentKey = sha256(`facts-layout-v10-go323-xml-lexical\0tree-sitter-0.26.3\0lua-0.5.0\0xml-0.38\0generator-v6\0${document.path}\0${document.sha256}`);
    const historical = fs.readFileSync(f.files[0].rustFile);
    fs.writeFileSync(path.join(f.rustHome, 'cache/source-facts-rust', `${currentKey}.json`), historical);
    await assert.rejects(readFactDelta(f.goHome, f.rustHome, f.records, 'v10'), /Fact input identity mismatch/);
    assert.deepEqual(fs.readFileSync(f.files[0].rustFile), historical);
    assert.equal((await f.read()).differentFiles.length, 0);
  }, 'v9');
});


test('shared Rust reader authenticates all four v10 modes without changing source or fact bytes', async t => {
  await withFixture(t, async f => {
    for (const [i, document] of f.documents.entries()) {
      const before = fs.readFileSync(f.files[i].rustFile), result = readRustFact(f.rustHome, document, 'v10');
      assert.equal(rustFactPath(f.rustHome, document, 'v10'), f.files[i].rustFile);
      assert.equal(result.fileSHA256, sha256(before)); assert.deepEqual(result.facts, JSON.parse(before).facts);
      assert.deepEqual(fs.readFileSync(f.files[i].rustFile), before);
    }
    assert.throws(() => rustFactPath(f.rustHome, f.documents[0], 'latest'), /Unsupported/);
    assert.throws(() => rustFactPath(f.rustHome, {...f.documents[0], sha256: 'bad'}, 'v10'), /identity/);
  }, 'v10');
});

test('shared Rust reader rejects wrong schema/input identity and unauthenticated Facts changes', async t => {
  await withFixture(t, async f => {
    const file = f.files[0].rustFile, original = JSON.parse(fs.readFileSync(file));
    for (const mutate of [v => v.schema = 'lycheedev.source-facts.rust.v9', v => v.inputSHA256 = '0'.repeat(64), v => v.facts.declarations[0].line++]) {
      const value = structuredClone(original); mutate(value); fs.writeFileSync(file, JSON.stringify(value));
      assert.throws(() => readRustFact(f.rustHome, f.documents[0], 'v10'), /identity mismatch|digest mismatch/);
    }
  }, 'v10');
});

test('shared Rust reader rejects invalid UTF8, nonarrays, missing and oversized cache files', async t => {
  await withFixture(t, async f => {
    const file = f.files[0].rustFile, original = fs.readFileSync(file);
    fs.writeFileSync(file, Buffer.from([0xff])); assert.throws(() => readRustFact(f.rustHome, f.documents[0], 'v10'));
    fs.writeFileSync(file, original); f.changeRust(0, facts => { facts.loads = null; }); assert.throws(() => readRustFact(f.rustHome, f.documents[0], 'v10'), /Invalid fact array loads/);
    fs.truncateSync(file, (32 << 20) + 1); assert.throws(() => readRustFact(f.rustHome, f.documents[0], 'v10'), /Invalid Rust fact file/);
    fs.unlinkSync(file); assert.throws(() => readRustFact(f.rustHome, f.documents[0], 'v10'), /ENOENT/);
  }, 'v10');
});

test('shared Rust reader rejects a directory in place of a cache file', async t => {
  await withFixture(t, async f => {
    const file = f.files[0].rustFile;
    fs.unlinkSync(file); fs.mkdirSync(file);
    assert.throws(() => readRustFact(f.rustHome, f.documents[0], 'v10'), {message: 'Invalid Rust fact file'});
  }, 'v10');
});

function fileSymlinkOrSkip(t, target, file, {platform = process.platform, create = fs.symlinkSync} = {}) {
  try { create(target, file, 'file'); }
  catch (error) {
    if (platform !== 'win32' || !['EPERM', 'EACCES'].includes(error.code)) throw error;
    t.skip(`Windows file symlink creation unavailable (${error.code}); this file-symlink assertion was not executed`);
    return false;
  }
  assert(fs.lstatSync(file).isSymbolicLink());
  return true;
}

test('Rust reader file symlink skip only covers Windows creation EPERM and EACCES', async t => {
  await withFixture(t, async f => {
    const target = f.files[0].rustFile, file = target + '.link';
    for (const platform of ['win32', 'linux']) for (const code of ['EPERM', 'EACCES', 'ENOENT', 'EIO']) {
      const error = Object.assign(new Error('creation denied'), {code}), skipped = [], context = {skip: reason => skipped.push(reason)};
      const create = (_target, _file, type) => { assert.equal(type, 'file'); throw error; };
      if (platform === 'win32' && ['EPERM', 'EACCES'].includes(code)) {
        assert.equal(fileSymlinkOrSkip(context, target, file, {platform, create}), false);
        assert.equal(skipped.length, 1); assert(skipped[0].includes(code));
      } else {
        assert.throws(() => fileSymlinkOrSkip(context, target, file, {platform, create}), actual => actual === error);
        assert.equal(skipped.length, 0);
      }
    }
    const skipped = [];
    assert.throws(() => fileSymlinkOrSkip({skip: reason => skipped.push(reason)}, target, file, {platform: 'win32', create(_target, file) { fs.writeFileSync(file, 'not a link'); }}), {code: 'ERR_ASSERTION'});
    assert.equal(skipped.length, 0);
  });
});

test('shared Rust reader rejects a file symlink to an authenticated cache file', async t => {
  await withFixture(t, async f => {
    const file = f.files[0].rustFile, saved = file + '.original', original = fs.readFileSync(file);
    assert.equal(readRustFact(f.rustHome, f.documents[0], 'v10').fileSHA256, sha256(original));
    fs.renameSync(file, saved);
    if (!fileSymlinkOrSkip(t, saved, file)) return;
    assert.deepEqual(fs.readFileSync(file), original);
    assert.throws(() => readRustFact(f.rustHome, f.documents[0], 'v10'), {message: 'Invalid Rust fact file'});
    assert.deepEqual(fs.readFileSync(saved), original);
  }, 'v10');
});


test('reads explicit v11 using the exact current generated-origin production key and schema', async t => {
  const production = fs.readFileSync(new URL('../../crates/lycheedev-source/src/index.rs', import.meta.url), 'utf8');
  const key = /fn facts_cache_key[\s\S]*?"([^"\n]+)"/.exec(production)?.[1];
  assert.equal(key, 'facts-layout-v11-go323-generated-origins\\0tree-sitter-0.26.3\\0lua-0.5.0\\0xml-0.38\\0generator-v6\\0{path}\\0{sha256}');
  assert.match(production, /envelope\["schema"\] == "lycheedev\.source-facts\.rust\.v11"/);
  assert.match(production, /rust-index-v15\\0assets-v2-go323-metadata\\0facts-pack-v2-path-radix512\\0facts-layout-v11-go323-generated-origins/);
  await withFixture(t, async f => {
    for (const [index, document] of f.documents.entries()) {
      const preimage = key.replaceAll('\\0', '\0').replace('{path}', document.path).replace('{sha256}', document.sha256);
      assert.equal(path.basename(f.files[index].rustFile), `${sha256(preimage)}.json`);
      const before = fs.readFileSync(f.files[index].rustFile);
      assert.equal(readRustFact(f.rustHome, document, 'v11').fileSHA256, sha256(before));
      assert.deepEqual(fs.readFileSync(f.files[index].rustFile), before);
    }
    const report = await f.read();
    assert.equal(report.inputs.rustLayout, 'v11');
    assert.equal(report.documents, 4);
    assert.equal(report.differentFiles.length, 0);
    for (const historical of ['v6', 'v7', 'v8', 'v9', 'v10']) {
      await assert.rejects(readFactDelta(f.goHome, f.rustHome, f.records, historical), /ENOENT/);
    }
    await assert.rejects(readFactDelta(f.goHome, f.rustHome, f.records), /ENOENT/);
  }, 'v11');
});

test('never discovers or relabels authenticated historical v10 Facts as v11', async t => {
  await withFixture(t, async f => {
    await assert.rejects(readFactDelta(f.goHome, f.rustHome, f.records, 'v11'), /ENOENT/);
    const document = f.documents[0];
    const current = rustFactPath(f.rustHome, document, 'v11');
    const historical = fs.readFileSync(f.files[0].rustFile);
    fs.writeFileSync(current, historical);
    assert.throws(() => readRustFact(f.rustHome, document, 'v11'), /Fact input identity mismatch/);
    await assert.rejects(readFactDelta(f.goHome, f.rustHome, f.records, 'v11'), /Fact input identity mismatch/);
    assert.deepEqual(fs.readFileSync(f.files[0].rustFile), historical);
    assert.equal((await f.read()).differentFiles.length, 0);
  }, 'v10');
});

function packFixture(t) {
  const f = fixture('v11');
  t.after(() => fs.rmSync(f.root, {recursive: true, force: true}));
  const group = sha256('fixed bounded cohort input identity');
  const packFile = path.join(f.rustHome, 'cache/source-facts-rust', `${group}.json`);
  const parts = [Buffer.from('LYCFPK01')];
  let offset = 8;
  const documents = f.documents.map((document, i) => {
    const raw = fs.readFileSync(f.files[i].rustFile);
    const header = Buffer.alloc(68);
    header.writeUInt32LE(raw.length);
    Buffer.from(path.basename(f.files[i].rustFile, '.json'), 'hex').copy(header, 4);
    Buffer.from(sha256(raw), 'hex').copy(header, 36);
    const factsLocator = {group, offset, bytes: raw.length, sha256: sha256(raw)};
    offset += header.length + raw.length;
    parts.push(header, raw);
    fs.unlinkSync(f.files[i].rustFile);
    return {...document, factsLocator};
  });
  fs.writeFileSync(packFile, Buffer.concat(parts));
  const indexFile = path.join(f.root, 'same-run-index.json');
  const index = {documents, summary: {parser}};
  const writeIndex = () => {
    const raw = Buffer.from(JSON.stringify(index));
    fs.writeFileSync(indexFile, raw);
    return sha256(raw);
  };
  const indexSHA = writeIndex();
  return {...f, packFile, indexFile, indexSHA, index, writeIndex,
    readPack: () => readFactDelta(f.goHome, f.rustHome, f.records, 'v11-pack1', indexFile, indexSHA)};
}

test('explicit pack layout binds the same-run index and reads complete ordered Facts by range', async t => {
  const f = packFixture(t);
  const before = fs.readFileSync(f.packFile);
  const report = await f.readPack();
  assert.equal(report.documents, 4);
  assert.equal(report.differentFiles.length, 0);
  assert.equal(report.inputs.rustIndexSHA256, f.indexSHA);
  for (const document of f.index.documents) {
    const read = readRustFact(f.rustHome, document, 'v11-pack1');
    assert.equal(read.fileSHA256, null);
    assert.equal(read.recordSHA256, document.factsLocator.sha256);
  }
  assert.deepEqual(fs.readFileSync(f.packFile), before);
  await assert.rejects(readFactDelta(f.goHome, f.rustHome, f.records, 'v11'), /ENOENT/);
});

test('pack layout rejects absent or wrong index authentication and different input identity', async t => {
  const f = packFixture(t);
  await assert.rejects(readFactDelta(f.goHome, f.rustHome, f.records, 'v11-pack1'), /same-run Rust index/);
  await assert.rejects(readFactDelta(f.goHome, f.rustHome, f.records, 'v11-pack1', f.indexFile, '0'.repeat(64)), /index authentication/);
  f.index.documents[0].sha256 = '1'.repeat(64);
  await assert.rejects(readFactDelta(f.goHome, f.rustHome, f.records, 'v11-pack1', f.indexFile, f.writeIndex()), /index input identity/);
});

test('pack locator rejects overflow and every header key length digest mismatch', async t => {
  const f = packFixture(t);
  const document = f.index.documents[0];
  const original = fs.readFileSync(f.packFile);
  for (const offset of [0, -1, Number.MAX_SAFE_INTEGER, 8.5, 32 << 20]) {
    assert.throws(() => readRustFact(f.rustHome, {...document, factsLocator: {...document.factsLocator, offset}}, 'v11-pack1'), /locator bounds/);
  }
  for (const at of [0, 8, 12, 44, 76]) {
    const changed = Buffer.from(original); changed[at] ^= 0xff; fs.writeFileSync(f.packFile, changed);
    assert.throws(() => readRustFact(f.rustHome, document, 'v11-pack1'), /header|identity mismatch|digest mismatch/);
  }
  fs.writeFileSync(f.packFile, original.subarray(0, 76));
  assert.throws(() => readRustFact(f.rustHome, document, 'v11-pack1'), /locator bounds/);
});

test('pack reader never reads an unrelated record body to answer a selected locator', t => {
  const f = packFixture(t);
  const first = f.index.documents[0];
  const next = f.index.documents[1].factsLocator;
  const body = fs.readFileSync(f.packFile);
  body[next.offset + 68] ^= 0xff;
  fs.writeFileSync(f.packFile, body);
  assert.equal(readRustFact(f.rustHome, first, 'v11-pack1').recordSHA256, first.factsLocator.sha256);
  assert.throws(() => readRustFact(f.rustHome, f.index.documents[1], 'v11-pack1'), /digest mismatch/);
});

test('pack layout keeps explicit per-file fallback records and validates complete envelopes', async t => {
  const f = packFixture(t);
  const document = f.index.documents[0];
  const locator = document.factsLocator;
  const packed = fs.readFileSync(f.packFile);
  const raw = packed.subarray(locator.offset + 68, locator.offset + 68 + locator.bytes);
  const fallback = rustFactPath(f.rustHome, document, 'v11');
  fs.writeFileSync(fallback, raw);
  delete document.factsLocator;
  const digest = f.writeIndex();
  const report = await readFactDelta(f.goHome, f.rustHome, f.records, 'v11-pack1', f.indexFile, digest);
  assert.equal(report.differentFiles.length, 0);
  const envelope = JSON.parse(raw);
  envelope.inputSHA256 = '0'.repeat(64);
  fs.writeFileSync(fallback, JSON.stringify(envelope));
  await assert.rejects(readFactDelta(f.goHome, f.rustHome, f.records, 'v11-pack1', f.indexFile, digest), /input identity mismatch/);
});

test('explicit pack2 preserves complete five-array ranges and authenticates its supplied index', async t => {
  const f = packFixture(t);
  const historical = await f.readPack();
  const current = await readFactDelta(f.goHome, f.rustHome, f.records, 'v11-pack2', f.indexFile, f.indexSHA);
  assert.equal(current.inputs.rustLayout, 'v11-pack2');
  assert.equal(historical.inputs.rustLayout, 'v11-pack1');
  assert.deepEqual(current.totals, historical.totals);
  assert.deepEqual(current.differentFiles, []);
  for (const document of f.index.documents) {
    const before = readRustFact(f.rustHome, document, 'v11-pack1');
    const after = readRustFact(f.rustHome, document, 'v11-pack2');
    assert.deepEqual(after, before);
    for (const section of ['declarations', 'relationships', 'loads', 'headers', 'diagnostics']) {
      assert(Array.isArray(after.facts[section]));
      assert.deepEqual(after.facts[section], before.facts[section]);
    }
  }
  await assert.rejects(readFactDelta(f.goHome, f.rustHome, f.records, 'v11-pack2'), /same-run Rust index/);
  await assert.rejects(readFactDelta(f.goHome, f.rustHome, f.records, 'v11-pack2', f.indexFile, '0'.repeat(64)), /index authentication/);
  const production = fs.readFileSync(new URL('../../crates/lycheedev-source/src/index/facts_cache.rs', import.meta.url), 'utf8');
  assert.match(production, /facts-pack-v2-path-radix512\\0facts-layout-v11-go323-generated-origins/);
  assert.match(production, /const LEAF: usize = 512;/);
  assert.match(production, /const GROUP: usize = 64;/);
  assert.match(production, /b"LYCFPK01"/);
});

test('pack2 keeps locator corruption, exact envelope identity and per-file fallback checks', async t => {
  const f = packFixture(t);
  const document = f.index.documents[0];
  const original = fs.readFileSync(f.packFile);
  for (const offset of [0, -1, Number.MAX_SAFE_INTEGER, 8.5, 32 << 20]) {
    assert.throws(() => readRustFact(f.rustHome, {...document, factsLocator: {...document.factsLocator, offset}}, 'v11-pack2'), /locator bounds/);
  }
  for (const at of [0, 8, 12, 44, 76]) {
    const changed = Buffer.from(original); changed[at] ^= 0xff; fs.writeFileSync(f.packFile, changed);
    assert.throws(() => readRustFact(f.rustHome, document, 'v11-pack2'), /header|identity mismatch|digest mismatch/);
  }
  fs.writeFileSync(f.packFile, original);
  const locator = document.factsLocator;
  const raw = original.subarray(locator.offset + 68, locator.offset + 68 + locator.bytes);
  const fallback = rustFactPath(f.rustHome, document, 'v11');
  fs.writeFileSync(fallback, raw);
  delete document.factsLocator;
  const digest = f.writeIndex();
  assert.equal((await readFactDelta(f.goHome, f.rustHome, f.records, 'v11-pack2', f.indexFile, digest)).differentFiles.length, 0);
  const envelope = JSON.parse(raw); envelope.inputSHA256 = '0'.repeat(64);
  fs.writeFileSync(fallback, JSON.stringify(envelope));
  await assert.rejects(readFactDelta(f.goHome, f.rustHome, f.records, 'v11-pack2', f.indexFile, digest), /input identity mismatch/);
});
