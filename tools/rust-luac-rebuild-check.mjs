// Candidate-only reproducibility and language checks. Never updates the pin.
import assert from 'node:assert/strict';
import {readFileSync, writeFileSync} from 'node:fs';
import {join, resolve} from 'node:path';
import {createHash} from 'node:crypto';
import {fileURLToPath} from 'node:url';
import {beginScratch, contained} from './rust-disk.mjs';
import {ownedSpawnSync} from './rust-owned-process.mjs';
import {verifySystemImports, luaSource} from './luac.mjs';
const root = process.cwd();
const [first, second] = process.argv.slice(2);
if (!first || !second || process.argv.length !== 4) throw new Error('usage: rust-luac-rebuild-check.mjs <first-build> <second-build>');
const roots = [first, second].map(path => contained(root, resolve(path)));
assert.notEqual(roots[0], roots[1]);
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const builds = roots.map(path => {
  const report = JSON.parse(readFileSync(join(path, 'build-report.json')));
  assert.deepEqual(JSON.parse(readFileSync(join(path, 'runtime.json'))), report.manifest.runtime);
  const archive = readFileSync(join(path, 'lua-5.1.5.tar.gz'));
  assert.equal(archive.length, report.manifest.source.bytes);
  assert.equal(hash(archive), report.manifest.source.sha256);
  assert.equal(report.manifest.source.url, luaSource.url);
  const exe = readFileSync(join(path, 'luac.exe')), license = readFileSync(join(path, 'LICENSE'));
  assert.equal(hash(exe), report.manifest.runtime.sha256);
  assert.equal(exe.length, report.manifest.runtime.bytes);
  assert.equal(hash(license), report.manifest.runtime.licenseSHA256);
  assert.equal(hash(readFileSync(join(path, 'lua-5.1.5.tar.gz'))), luaSource.sha256);
  assert.deepEqual(verifySystemImports(exe), report.manifest.imports);
  return report;
});
assert.deepEqual(builds[0].manifest, builds[1].manifest);
const job = beginScratch(root, 'luac-rebuild-check', {command: process.argv, inputs: {runnerSHA256: hash(readFileSync(fileURLToPath(import.meta.url)))}});
let state = 'failed', handlesReaped = true;
const logs = [], cases = [];
try {
  const inputs = [
    ['valid', 'local value = 1\nreturn value\n', 0],
    ['invalid', 'local =\n', 1],
    ['lua52-goto-rejected', 'goto finished\n::finished::\n', 1],
    ['lua53-floor-div-rejected', 'return 7 // 2\n', 1],
    ['varargs', 'local function f(...) return ... end\nreturn f(1,2)\n', 0],
    ['utf8-comment-string', '-- 中文注释\nreturn "中文"\n', 0],
    ['long-string', 'return [==[line\ntext]==]\n', 0],
  ];
  for (const [name, source, expected] of inputs) {
    const path = join(job.directory, `${name}.lua`); writeFileSync(path, source);
    handlesReaped = false;
    const result = ownedSpawnSync(join(roots[0], 'luac.exe'), ['-p', path], {cwd: job.directory, env: process.env, evidence: job.evidence, timeout: 10000, cargoBuild: false});
    handlesReaped = result.handlesReaped === true; logs.push(...result.logs);
    cases.push({name, source, sourceBytes: Buffer.byteLength(source), sourceSHA256: hash(Buffer.from(source)), expected, actual: result.status, handlesReaped, error: result.error?.message ?? result.error ?? null});
    if (!handlesReaped || result.error || result.status !== expected) throw new Error(`luac.acceptance_failed: ${name}`);
  }
  state = 'succeeded';
} finally {
  const report = join(job.evidence, 'rebuild-check.json');
  writeFileSync(report, JSON.stringify({schema: 'lycheedev.luac-rebuild-check.v1', state, builds: roots, runtime: builds[0].manifest.runtime, imports: builds[0].manifest.imports, cases, pinUpdated: false}, null, 2)+'\n');
  logs.push(report); job.seal({state: handlesReaped ? state : 'interrupted', handlesReaped, logs});
  if (handlesReaped) job.cleanup();
}
