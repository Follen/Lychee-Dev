import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { readFactDelta, sha256 } from './read-fact-delta.mjs';

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
    const layoutKey = rustLayout === 'v8' ? 'facts-layout-v8-go323-malformed' : rustLayout === 'v7' ? 'facts-layout-v7-go323-public' : 'facts-layout-v6-xml-original-tags';
    const generator = ['v7', 'v8'].includes(rustLayout) ? 'generator-v6' : 'generator-v5';
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
    root, goHome, rustHome, records, manifest, files,
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
