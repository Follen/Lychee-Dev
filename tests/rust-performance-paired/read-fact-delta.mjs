// Read-only diagnosis of existing Go/Rust fact caches. Never an acceptance filter.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import readline from 'node:readline';
import { fileURLToPath } from 'node:url';

const RUST_LAYOUTS = Object.freeze({
  // Same authenticated wire format as pack1; the caller must bind this new
  // grouping domain to the exact candidate source/artifact and index SHA.
  'v11-pack2': { key: 'facts-layout-v11-go323-generated-origins', generator: 'generator-v6', schema: 'lycheedev.source-facts.rust.v11', packed: true },
  'v11-pack1': { key: 'facts-layout-v11-go323-generated-origins', generator: 'generator-v6', schema: 'lycheedev.source-facts.rust.v11', packed: true },
  v11: { key: 'facts-layout-v11-go323-generated-origins', generator: 'generator-v6', schema: 'lycheedev.source-facts.rust.v11' },
  v6: { key: 'facts-layout-v6-xml-original-tags', generator: 'generator-v5', schema: 'lycheedev.source-facts.rust.v6' },
  v10: { key: 'facts-layout-v10-go323-xml-lexical', generator: 'generator-v6', schema: 'lycheedev.source-facts.rust.v10' },
  v9: { key: 'facts-layout-v9-go323-observed', generator: 'generator-v6', schema: 'lycheedev.source-facts.rust.v9' },
  v8: { key: 'facts-layout-v8-go323-malformed', generator: 'generator-v6', schema: 'lycheedev.source-facts.rust.v8' },
  v7: { key: 'facts-layout-v7-go323-public', generator: 'generator-v6', schema: 'lycheedev.source-facts.rust.v7' },
});
const SECTIONS = ['declarations', 'relationships', 'loads', 'headers', 'diagnostics'];
function exactFactSections(value, label) {
  if (!value || Array.isArray(value) || Object.keys(value).sort().join('\0') !== [...SECTIONS].sort().join('\0')) throw new Error(`Invalid exact five Facts arrays: ${label}`);
}
export const sha256 = value => crypto.createHash('sha256').update(value).digest('hex');

function stable(value) {
  if (Array.isArray(value)) return value.map(stable);
  if (value && typeof value === 'object') {
    return Object.fromEntries(Object.keys(value).sort().map(key => [key, stable(value[key])]));
  }
  return value;
}
const signature = value => JSON.stringify(stable(value));
const add = (object, key, amount = 1) => { object[key] = (object[key] || 0) + amount; };

// Same comparison request only: no path/mtime or persisted cache. A report is
// branded here only after its original full-byte hash, native JSON.parse and
// comparison checks succeed. Serialized/copied reports cannot supply a view.
const comparedIndexes = new WeakMap();
export function comparedRustIndex(comparison, source) {
  const index = comparedIndexes.get(comparison);
  if (!index) throw new Error('Authenticated comparison Rust index required');
  if (index.bytes !== source.bytes || index.sha256 !== source.sha256) throw new Error('Comparison Rust index identity mismatch');
  // Callers must authenticate the complete live file or archived parts at the
  // current custody boundary before borrowing these retained fields.
  return index;
}

function compareRows(go, rust) {
  const remaining = new Map();
  for (const row of go) {
    const key = signature(row);
    const entry = remaining.get(key) || { row, count: 0 };
    entry.count++;
    remaining.set(key, entry);
  }
  const rustOnly = [];
  for (const row of rust) {
    const entry = remaining.get(signature(row));
    if (entry && entry.count > 0) entry.count--;
    else rustOnly.push(row);
  }
  const goOnly = [...remaining.values()].flatMap(({ row, count }) => Array(count).fill(row));
  const sequenceEqual = signature(go) === signature(rust);
  return {
    goOnly, rustOnly, sequenceEqual,
    sameMultisetButDifferentOrder: !sequenceEqual && !goOnly.length && !rustOnly.length,
  };
}

// Shared authenticated Rust reader for same-run candidate diagnostics. This is
// read-only and keeps the historical Go/Rust reader's default layout unchanged.
export function rustFactPath(home, document, rustLayout = 'v6') {
  const layout = RUST_LAYOUTS[rustLayout];
  if (!layout) throw new Error(`Unsupported Rust facts layout: ${rustLayout}`);
  if (typeof document.path !== 'string' || !document.path || !/^[a-f0-9]{64}$/.test(document.sha256)) {
    throw new Error('Invalid Rust fact document identity');
  }
  if (layout.packed && document.factsLocator) {
    if (!/^[a-f0-9]{64}$/.test(document.factsLocator.group)) throw new Error('Invalid Rust Facts group identity');
    return path.join(home, 'cache/source-facts-rust', `${document.factsLocator.group}.json`);
  }
  const key = sha256(`${layout.key}\0tree-sitter-0.26.3\0`
    + `lua-0.5.0\0xml-0.38\0${layout.generator}\0${document.path}\0${document.sha256}`);
  return path.join(home, 'cache/source-facts-rust', `${key}.json`);
}
export function readRustFact(home, document, rustLayout = 'v6') {
  const file = rustFactPath(home, document, rustLayout);
  const stat = fs.lstatSync(file);
  if (!stat.isFile() || stat.isSymbolicLink() || stat.size > 32 << 20) throw new Error('Invalid Rust fact file');
  const locator = RUST_LAYOUTS[rustLayout].packed ? document.factsLocator : null;
  let raw;
  if (locator) {
    const {offset, bytes, sha256: recordSHA} = locator;
    if (!Number.isSafeInteger(offset) || offset < 8 || !Number.isSafeInteger(bytes) || bytes < 1
      || bytes > 32 << 20 || offset + 68 + bytes > stat.size || !/^[a-f0-9]{64}$/.test(recordSHA)) {
      throw new Error('Invalid Rust Facts locator bounds');
    }
    const descriptor = fs.openSync(file, 'r');
    const read = (at, length) => {
      const value = Buffer.alloc(length);
      for (let used = 0; used < length;) {
        const n = fs.readSync(descriptor, value, used, Math.min(65536, length - used), at + used);
        if (!n) throw new Error('Truncated Rust Facts record');
        used += n;
      }
      return value;
    };
    try {
      if (!read(0, 8).equals(Buffer.from('LYCFPK01'))) throw new Error('Invalid Rust Facts pack header');
      const header = read(offset, 68);
      const expectedKey = path.basename(rustFactPath(home, document, 'v11'), '.json');
      if (header.readUInt32LE(0) !== bytes || header.subarray(4, 36).toString('hex') !== expectedKey
        || header.subarray(36).toString('hex') !== recordSHA) throw new Error('Rust Facts record identity mismatch');
      raw = read(offset + 68, bytes);
      if (sha256(raw) !== recordSHA) throw new Error('Rust Facts record digest mismatch');
    } finally { fs.closeSync(descriptor); }
  } else raw = fs.readFileSync(file);
  const envelope = JSON.parse(new TextDecoder('utf-8', {fatal: true}).decode(raw));
  if (envelope.schema !== RUST_LAYOUTS[rustLayout].schema || envelope.inputSHA256 !== document.sha256) {
    throw new Error(`Fact input identity mismatch: ${document.path}`);
  }
  if (sha256(signature(envelope.facts)) !== envelope.factsSHA256) {
    throw new Error(`Rust facts digest mismatch: ${document.path}`);
  }
  exactFactSections(envelope.facts, document.path);
  for (const section of SECTIONS) if (!Array.isArray(envelope.facts[section])) {
    throw new Error(`Invalid fact array ${section}: ${document.path}`);
  }
  return {file, bytes: raw.length, fileSHA256: locator ? null : sha256(raw), ...(locator ? {record: locator, recordSHA256: sha256(raw)} : {}), factsSHA256: envelope.factsSHA256, facts: envelope.facts};
}

export async function readFactDelta(goHome, rustHome, goRecords, rustLayout = 'v6', rustIndexFile, expectedIndexSHA256, retainedGo = undefined, {captureSources = false, retainIndex = false} = {}) {
  const layout = RUST_LAYOUTS[rustLayout];
  if (!layout) throw new Error(`Unsupported Rust facts layout: ${rustLayout}`);
  const manifestFile = path.join(path.dirname(goRecords), 'manifest.json');
  // The formal sequential runner supplies authenticated immutable custody streams.
  // CLI calls use the original on-disk inputs; no configuration injects readers.
  const manifestRaw = retainedGo?.manifestRaw ?? fs.readFileSync(manifestFile);
  const manifest = JSON.parse(manifestRaw);
  const sourceHash = crypto.createHash('sha256');
  let sourceBytes = 0;
  const input = retainedGo?.recordsStream ?? fs.createReadStream(goRecords);
  input.on('data', chunk => { sourceHash.update(chunk); sourceBytes += chunk.length; });
  const documents = [];
  for await (const line of readline.createInterface({ input, crlfDelay: Infinity })) {
    if (!line) continue;
    const row = JSON.parse(line);
    if (row.kind === 'document') documents.push(row);
  }
  if (sourceHash.digest('hex') !== manifest.recordsHash || sourceBytes !== manifest.recordBytes) {
    throw new Error('Go records authentication mismatch');
  }
  let rustDocuments;
  let indexIdentity;
  let indexProjection;
  if (layout.packed) {
    if (!rustIndexFile || !/^[a-f0-9]{64}$/.test(expectedIndexSHA256 || '')) throw new Error('Packed Facts require the same-run Rust index blob and expected SHA-256');
    const stat = fs.lstatSync(rustIndexFile);
    if (!stat.isFile() || stat.isSymbolicLink() || stat.size > 128 << 20) throw new Error('Invalid Rust index file');
    const indexRaw = fs.readFileSync(rustIndexFile);
    if (sha256(indexRaw) !== expectedIndexSHA256) throw new Error('Rust index authentication mismatch');
    const index = JSON.parse(new TextDecoder('utf-8', {fatal: true}).decode(indexRaw));
    if (!Array.isArray(index.documents) || index.documents.length !== documents.length
      || index.summary?.parser !== manifest.summary.parser) throw new Error('Rust index document identity mismatch');
    for (const field of ['repository', 'product', 'commit']) {
      if (typeof manifest.summary[field] === 'string' && index.summary?.[field] !== manifest.summary[field]) throw new Error('Rust index snapshot identity mismatch');
    }
    if (index.documents.some((doc, i) => doc.path !== documents[i].path)) throw new Error('Rust index document order mismatch');
    rustDocuments = new Map(index.documents.map(doc => [doc.path, doc]));
    if (rustDocuments.size !== documents.length) throw new Error('Duplicate Rust index document');
    for (const document of documents) {
      const indexed = rustDocuments.get(document.path);
      if (!indexed || indexed.sha256 !== document.sha256 || (Number.isSafeInteger(document.bytes) && indexed.bytes !== document.bytes)) throw new Error('Rust index input identity mismatch');
    }
    indexIdentity = {rustIndexFile, rustIndexSHA256: expectedIndexSHA256, rustIndexBytes: indexRaw.length};
    if (retainIndex) {
      // Keep only the existing consumers' document/summary subtrees. In
      // particular, the full parsed symbols/assets tree is not retained.
      // Locator copies cannot be changed through the report's source tuples.
      indexProjection = Object.freeze({bytes: indexRaw.length, sha256: expectedIndexSHA256,
        documents: Object.freeze(index.documents.map(doc => Object.freeze({...doc,
          ...(Object.hasOwn(doc, 'factsLocator') ? {factsLocator: doc.factsLocator && typeof doc.factsLocator === 'object' ? Object.freeze({...doc.factsLocator}) : doc.factsLocator} : {}),
        }))), summary: Object.freeze({...index.summary})});
    }
  } else if (rustIndexFile !== undefined || expectedIndexSHA256 !== undefined) throw new Error('Historical per-file layout does not accept an index binding');
  const report = {
    diagnosticOnly: true,
    acceptanceUnchanged: 'original comparison and failure remain authoritative',
    goSummary: manifest.summary,
    inputs: { rustLayout, goRecords, goManifestSHA256: sha256(manifestRaw), goHome, rustHome, ...indexIdentity },
    documents: documents.length,
    totals: { go: {}, rust: {} },
    categories: {},
    byExtension: {},
    differentFiles: [],
    firstDifferences: [],
  };
  const goSources = new Map(), rustSources = [];
  for (const document of documents) {
    const extension = path.extname(document.path).toLowerCase();
    const generated = extension === '.lua'
      && document.path.includes('/Blizzard_APIDocumentationGenerated/');
    const mode = extension + (generated ? ':generated-api' : '');
    const parser = manifest.summary.parser;
    const goKey = sha256(`${parser}\0${mode}\0${document.sha256}`);
    const goFile = path.join(goHome, 'source/v1/facts/files', parser, `${goKey}.json`);
    const rust = readRustFact(rustHome, rustDocuments?.get(document.path) ?? document, rustLayout), rustFile = rust.file;
    const goRaw = retainedGo ? retainedGo.readFact(goFile) : fs.readFileSync(goFile);
    if (captureSources) {
      const source = {file: goFile, bytes: goRaw.length, sha256: sha256(goRaw)}, previous = goSources.get(goFile);
      if (previous && signature(previous) !== signature(source)) throw new Error('Go comparison source changed between duplicate references');
      goSources.set(goFile, source);
      rustSources.push({path: document.path, file: rustFile, bytes: rust.bytes, sha256: rust.recordSHA256 ?? rust.fileSHA256, ...(rust.record ? {record: rust.record} : {})});
    }
    const go = JSON.parse(goRaw);
    if (go.schema !== 'lycheedev.source-file-facts.v2'
      || go.inputSHA256 !== document.sha256 || go.mode !== mode || go.parser !== parser) {
      throw new Error(`Fact input identity mismatch: ${document.path}`);
    }
    // Fixed Go factEnvelope puts facts last. Preserve its exact bytes, including
    // encoding/json's HTML and U+2028/U+2029 escaping, rather than reserializing.
    const goText = goRaw.toString('utf8');
    const marker = ',"facts":';
    const offset = goText.indexOf(marker);
    if (offset < 0 || sha256(goText.slice(offset + marker.length, -1)) !== go.factsSHA256) {
      throw new Error(`Go facts digest mismatch: ${document.path}`);
    }
    exactFactSections(go.facts, document.path);
    const difference = {
      path: document.path, sha256: document.sha256,
      counts: {}, delta: {}, exactRowsDiffer: [],
    };
    for (const section of SECTIONS) {
      const a = go.facts[section];
      const b = rust.facts[section];
      if (!Array.isArray(a) || !Array.isArray(b)) {
        throw new Error(`Invalid fact array ${section}: ${document.path}`);
      }
      difference.counts[section] = { go: a.length, rust: b.length };
      difference.delta[section] = b.length - a.length;
      add(report.totals.go, section, a.length);
      add(report.totals.rust, section, b.length);
      report.byExtension[extension] ??= { go: {}, rust: {} };
      add(report.byExtension[extension].go, section, a.length);
      add(report.byExtension[extension].rust, section, b.length);
      for (const [side, rows] of [['go', a], ['rust', b]]) {
        for (const row of rows) {
          const key = `${section}:${row.category || row.kind || '<none>'}`;
          report.categories[key] ??= { go: 0, rust: 0 };
          add(report.categories[key], side);
        }
      }
      const compared = compareRows(a, b);
      if (!compared.sequenceEqual) {
        difference.exactRowsDiffer.push({
          section, goOnly: compared.goOnly.length, rustOnly: compared.rustOnly.length,
          sequenceEqual: false,
          sameMultisetButDifferentOrder: compared.sameMultisetButDifferentOrder,
        });
        if (report.firstDifferences.length < 10) {
          report.firstDifferences.push({
            path: document.path, sha256: document.sha256, section,
            goOnly: compared.goOnly.slice(0, 10), rustOnly: compared.rustOnly.slice(0, 10),
            sameMultisetButDifferentOrder: compared.sameMultisetButDifferentOrder,
            goFirstRows: a.slice(0, 10), rustFirstRows: b.slice(0, 10),
            goFactFile: goFile, rustFactFile: rustFile,
            goFactFileSHA256: sha256(goRaw), rustFactFileSHA256: rust.fileSHA256,
            ...(rust.record ? {rustFactRecord: rust.record, rustFactRecordSHA256: rust.recordSHA256} : {}),
          });
        }
      }
    }
    if (difference.exactRowsDiffer.length) report.differentFiles.push(difference);
  }
  for (const field of ['declarations', 'relationships', 'diagnostics']) {
    if (report.totals.go[field] !== manifest.summary[field]) {
      throw new Error(`Go facts total differs from sealed summary: ${field}`);
    }
  }
  if (documents.length !== manifest.summary.documents) {
    throw new Error('Go document total differs from sealed summary');
  }
  if (captureSources) report.factSources = {go: [...goSources.values()], rust: rustSources};
  if (indexProjection) comparedIndexes.set(report, indexProjection);
  return report;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [goHome, rustHome, goRecords, rustLayout, rustIndexFile, expectedIndexSHA256] = process.argv.slice(2);
  if (!goRecords || ![5, 6, 8].includes(process.argv.length)) {
    throw new Error('usage: node read-fact-delta.mjs GO_HOME RUST_HOME GO_RECORDS_JSONL [v6|v7|v8|v9|v10|v11|v11-pack1|v11-pack2] [RUST_INDEX_JSON EXPECTED_INDEX_SHA256]');
  }
  console.log(JSON.stringify(await readFactDelta(goHome, rustHome, goRecords, rustLayout, rustIndexFile, expectedIndexSHA256), null, 2));
}
