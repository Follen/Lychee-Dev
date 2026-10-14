// Independent archive audit. This does not run or accept the product CLI.
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {resolve} from 'node:path';
import {readTarGz} from '../../tools/tar.mjs';
import {assertCorpus, sha256} from './assertions.mjs';

export function verifyArchive(bytes, oracle) {
  assert.equal(sha256(bytes), oracle.archiveSHA256, 'fixed archive identity');
  const entries = readTarGz(bytes);
  const files = new Map([...entries].map(([name, raw]) => [name.split('/').slice(1).join('/'), raw]));
  assert.equal(files.size, entries.size, 'no duplicate flattened archive entries');
  const rows = [...files].map(([path, raw]) => ({path, bytes: raw.length, sha256: sha256(raw)}));
  assertCorpus(rows, oracle);
  for (const anchor of oracle.anchors) {
    const raw = files.get(anchor.path); assert(raw, 'anchor original file');
    assert.equal(sha256(raw), anchor.sha256);
    const lines = raw.toString('utf8').match(/[^\n]*\n|[^\n]+$/g) ?? [];
    assert.equal(lines.length, anchor.totalLines);
    assert.equal(lines[anchor.line - 1].replace(/[\r\n]+$/, ''), anchor.declarationLine);
    assert.equal(lines.slice(anchor.line - 1, anchor.line - 1 + anchor.count).join(''), anchor.text);
    if (anchor.queryLine) assert.equal(lines[anchor.queryLine - 1].trim(), '{', 'API query coordinate is original table start');
  }
  const call = oracle.staticCall, callLines = files.get(call.path).toString('utf8').split('\n');
  assert.equal(callLines[call.line - 1].replace(/\r$/, ''), call.text);
  assert.equal(callLines[call.callerLine - 1].replace(/\r$/, ''), call.callerText);
  for (const [path, raw] of files) if (/\.(lua|xml|toc)$/i.test(path)) assert(!/[\u4e00-\u9fff]/u.test(raw.toString('utf8')), 'Chinese literal no-result oracle');
  return {files: rows.length, documents: oracle.documents, documentBytes: oracle.documentBytes, anchors: oracle.anchors.length};
}
if (process.argv[1] && resolve(process.argv[1]) === resolve(import.meta.filename)) {
  if (process.argv.length !== 3) throw new Error('usage: node tests/rust-source-full-corpus/verify-oracle.mjs <fixed-retail.tar.gz>');
  const oracle = JSON.parse(readFileSync(new URL('./retail-oracle.json', import.meta.url)));
  console.log(JSON.stringify(verifyArchive(readFileSync(process.argv[2]), oracle), null, 2));
}
