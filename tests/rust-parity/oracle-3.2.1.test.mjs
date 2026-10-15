// Current oracle and fault tests. Synthetic comparisons are not product passes.
import test from 'node:test';
import {spawnSync} from 'node:child_process';
import assert from 'node:assert/strict';
import {readFileSync, mkdtempSync, mkdirSync, rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {dirname, join, resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import {loadCandidate, sha256, GO_COMMIT} from './oracle.mjs';
import {CURRENT_GO_COMMIT, CURRENT_FIXTURE, RETAINED_RELEASE_FIELDS, loadCurrentFixture, extractCurrentProducts, compareCurrent} from './oracle-3.2.1.mjs';
import {APPROVED_ENGINE_SUMMARIES, compareDescribeContract, compareCurrentDescribeContract} from './describe-contract.mjs';
import {runCliSmoke} from '../../tools/rust-cli-smoke.mjs';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const bytes = readFileSync(join(root, CURRENT_FIXTURE)), oracle = JSON.parse(bytes);
const clone = v => structuredClone(v);
function candidate() {
  const inventory = oracle.commands.map(({parserFlags, ...d}) => clone(d));
  for (const allowed of APPROVED_ENGINE_SUMMARIES) inventory.find(c => c.path === allowed.path).summary = allowed.actual;
  const arities = Object.fromEntries(oracle.commands.flatMap(c => c.parserFlags.map(f => [f.name, f.value])));
  const addon = oracle.addon.map(row => { const allowed = RETAINED_RELEASE_FIELDS.find(a => a.path === row.path); return allowed ? {path: row.path, ...allowed.retained} : clone(row); });
  return {inventory, arities, advertised: {commands: inventory}, products: oracle.products.map(p => ({id: p.product, family: p.family, catalog_product: p.productCode, folder: p.folder, series: p.buildSeries, reused_slot: p.reusedSlot})), addon, sourceTracks: {forever: 'forever'}};
}
const envelope = rows => ({schema: 'lycheedev.result.v1', ok: true, error: null, result: {commands: rows}});

test('current fixture independently authenticates 10b and preserves 3057 fixture identity', () => {
  const historicalBefore = readFileSync(join(root, 'tests/rust-parity/go-3.2.0.manifest.json'));
  const frozen = loadCurrentFixture(root);
  assert.deepEqual(frozen.oracle, oracle);
  assert.equal(frozen.fixtureSHA256, sha256(bytes));
  assert.equal(oracle.oracle.commit, CURRENT_GO_COMMIT);
  assert.equal(oracle.historicalReference.commit, GO_COMMIT);
  assert.equal(oracle.historicalReference.fixtureSHA256, sha256(historicalBefore));
  assert.deepEqual(readFileSync(join(root, 'tests/rust-parity/go-3.2.0.manifest.json')), historicalBefore);
  assert.equal(oracle.commands.length, 84); assert.equal(oracle.products.length, 7);
  assert.equal(oracle.addon.length, 55); assert.equal(oracle.goTestReference.files.length, 361);
  assert.equal(oracle.products[0].buildSeries, '12.1.5');
});
test('only exact approved engine and release identities yield accepted static equality', () => {
  const c = candidate(), before = clone(c), result = compareCurrent(oracle, c);
  assert.equal(result.rawExactStatus, 'fail'); assert.equal(result.acceptedStaticStatus, 'pass');
  assert.deepEqual(result.rawExact.commands.changed.map(x => x.path), ['doctor', 'source batch']);
  assert.deepEqual(result.accepted.addon.acceptedReleaseFields, RETAINED_RELEASE_FIELDS);
  assert.equal(result.accepted.commandMetadata.approvedEngineSummaryChanges.length, 2);
  assert.equal(result.behavioralParity, 'unverified'); assert.equal(result.execution, 'NOT_RUN');
  assert.deepEqual(c, before);
});
test('all current product fields are exact, including family folder and reused-slot semantics', () => {
  for (const [field, value] of [['series','12.1.0'],['family','wrong'],['folder','_wrong_'],['catalog_product','wrong'],['reused_slot',true]]) {
    const c = candidate(); c.products[0][field] = value;
    assert.equal(compareCurrent(oracle,c).acceptedStaticStatus, 'fail', field);
  }
});
test('release exceptions cannot mask additional bytes, missing/extra paths or forged rules', () => {
  for (const mutate of [
    c => {c.addon.find(x => x.path === 'addon/Core/Runtime.lua').sha256 = 'a'.repeat(64);},
    c => {c.addon.find(x => x.path === 'addon/Lychee Dev.toc').bytes++;},
    c => c.addon.pop(),
    c => c.addon.push({path:'addon/extra.lua',bytes:0,sha256:sha256('')}),
  ]) { const c = candidate(); mutate(c); assert.equal(compareCurrent(oracle,c).acceptedStaticStatus,'fail'); }
  const forged = clone(oracle), c = candidate(); c.addon[0].sha256 = 'f'.repeat(64);
  forged.retainedReleaseFields.push({path:c.addon[0].path,expected:oracle.addon[0],retained:c.addon[0]});
  assert.equal(compareCurrent(forged,c).acceptedStaticStatus,'fail');
});
test('unknown syntax/source identity and wrong oracle fail closed', () => {
  assert.throws(() => extractCurrentProducts(Buffer.from('arbitrary Go source')), /source hash mismatch/);
  assert.throws(() => compareCurrent({...oracle,oracle:{commit:GO_COMMIT}}, candidate()), /fixed 3.2.1/);
  assert.throws(() => compareCurrentDescribeContract({...oracle,oracle:{commit:GO_COMMIT}},envelope([])),/wrong_current_oracle/);
  assert.throws(() => compareDescribeContract(oracle,envelope([])),/wrong_oracle/);
});
test('current CLI retains all existing exact metadata/order and engine wording rejection boundaries', () => {
  for (const mutate of [rows=>rows.reverse(),rows=>rows.pop(),rows=>{rows[0].flags.reverse();},rows=>{rows.find(x=>x.path==='doctor').summary+=' ';}]) {
    const rows=candidate().inventory;mutate(rows);assert.equal(compareCurrentDescribeContract(oracle,envelope(rows)).acceptedContractMatch,false);
  }
});
test('actual current source report keeps every extra addon and does not relabel source as behavior', () => {
  const c=loadCandidate(root),r=compareCurrent(oracle,c);
  assert.equal(r.oracle.commit,CURRENT_GO_COMMIT);assert.equal(r.accepted.profiles.state,'pass');
  const names=new Set(oracle.addon.map(x=>x.path));
  assert.deepEqual(r.rawExact.addon.extra,c.addon.map(x=>x.path).filter(x=>!names.has(x)).sort());
  if(r.rawExact.addon.extra.length)assert.equal(r.acceptedStaticStatus,'fail');
  assert.equal(r.behavioralParity,'unverified');
});
test('default controlled CLI smoke authenticates current oracle and persists its identity', t => {
  const dir=mkdtempSync(join(tmpdir(),'lycheedev-current-describe-'));t.after(()=>rmSync(dir,{recursive:true,force:true}));
  const evidence=join(dir,'evidence'),directory=join(dir,'scratch');mkdirSync(evidence);mkdirSync(directory);
  const artifact={sourceBinary:join(dir,'not-executed.exe'),binarySHA256:'a'.repeat(64),inputHashes:{sourceTree:'b'.repeat(64)}};
  let sealed;
  const result=runCliSmoke(root,'controlled.json',{mode:'contract',verify:()=>artifact,
    begin:()=>({directory,evidence,seal:value=>{sealed=value;},cleanup:()=>{}}),
    execute:(binary,args)=>{assert.equal(binary,artifact.sourceBinary);assert.deepEqual(args,['describe','--format=json']);return {status:0,handlesReaped:true,logs:[],stdout:Buffer.from(JSON.stringify(envelope(candidate().inventory))),stderr:Buffer.alloc(0)};}});
  assert.equal(result.exitCode,0);assert.equal(sealed.state,'succeeded');
  assert.equal(result.contract.oracle.commit,CURRENT_GO_COMMIT);assert.equal(result.contract.fixtureSHA256,sha256(bytes));
  assert.equal(JSON.parse(readFileSync(result.reportPath)).oracle.commit,CURRENT_GO_COMMIT);
});

test('current runner rejects unsupported gap flags without writing any fixture', () => {
  const before = readFileSync(join(root, CURRENT_FIXTURE));
  for (const args of [['--accept-gaps'], ['--write-fixture', '--verify-fixture'], ['--json', '--json']]) {
    const run = spawnSync(process.execPath, [join(root, 'tests/rust-parity/run-3.2.1.mjs'), ...args], {cwd: root, encoding:'utf8', timeout:20000});
    assert.equal(run.status, 2); assert.match(run.stderr, /Usage/);
  }
  assert.deepEqual(readFileSync(join(root, CURRENT_FIXTURE)), before);
});
