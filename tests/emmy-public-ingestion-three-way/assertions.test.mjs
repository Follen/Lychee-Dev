import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {listedTests,suiteResult,traceRows,completeVariant,compareVariants} from './assertions.mjs';
const read=name=>JSON.parse(readFileSync(new URL(name,import.meta.url)));
const inputs=read('./inputs.json'),expected=inputs.tests;
function fixture(){return {suites:Object.fromEntries(Object.entries(expected).map(([name,names])=>[name,{passed:names.length,names}])),parity:read('./fixtures/A-parity.json'),outcomes:read('./fixtures/A-ingestion-outcomes.json'),checkpoints:read('./fixtures/A-constructor-checkpoints.json')};}
function complete(value){return completeVariant(value.suites,value.parity,value.outcomes,value.checkpoints,expected);}
test('frozen historical trace is a parser fixture, never a substitute Windows side',()=>{
 const value=complete(fixture());assert.equal(value.parity.length,450);assert.equal(value.outcomes.length,6);
 assert.throws(()=>compareVariants({A:value,B:structuredClone(value)}),/three_real_variants/);
});
test('Windows CRLF affects only record separators, not JSON strings',()=>{
 const names=expected.callback_contract;assert.deepEqual(listedTests(names.map(n=>`${n}: test`).join('\r\n'),names),names);
 const row={source:'a\r\nb',proof:'中文'},text=`DECLARATION_PARITY ${JSON.stringify(row)}\r\n`;
 assert.deepEqual(traceRows(text,'DECLARATION_PARITY '),[row]);
});
test('missing, duplicate or extra test names are rejected',()=>{
 const names=expected.callback_contract;
 for(const rows of [[names[0]],[...names,names[0]],[...names,'extra']])assert.throws(()=>listedTests(rows.map(n=>`${n}: test`).join('\n'),names),/exact_test_inventory/);
});
test('libtest ignored/filtered/wrong-count/duplicate summary never pass',()=>{
 const names=expected.callback_contract,good='test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.0s\r\n';
 assert.deepEqual(suiteResult(good,names),{passed:2,names});
 for(const text of [good.replace('2 passed','1 passed'),good.replace('0 ignored','1 ignored'),good.replace('0 filtered','1 filtered'),good+good,''])assert.throws(()=>suiteResult(text,names));
});
test('all 450 query/fault rows are required, with unique keys',()=>{
 const missing=fixture();missing.parity.pop();assert.throws(()=>complete(missing),/450_traces/);
 const duplicate=fixture();duplicate.parity[449]=duplicate.parity[448];assert.throws(()=>complete(duplicate),/unique_cases/);
});
test('persistent and single-shot check and step ordinals cannot be traded away',()=>{
 const value=fixture(),row=value.parity.find(r=>r.fault.axis==='step'&&r.fault.ordinal===1&&r.fault.persistent===true);
 assert(row);row.fault.ordinal=100000;assert.throws(()=>complete(value),/missing_fault/);
});
test('six constructor results and all actual checkpoint exhaustion sets are required',()=>{
 const missing=fixture();missing.outcomes.pop();assert.throws(()=>complete(missing),/6_constructor_outcomes/);
 const noPersistent=fixture();noPersistent.checkpoints[0].singleShotAndPersistent=false;assert.throws(()=>complete(noPersistent),/constructor_fault_exhaustion/);
 const zero=fixture();zero.checkpoints[0].checkpoints=0;assert.throws(()=>complete(zero),/constructor_fault_exhaustion/);
});
test('constructor callback counts may differ, complete outputs may not',()=>{
 const A=complete(fixture()),B=complete(fixture()),C=complete(fixture());B.checkpoints.forEach(row=>row.checkpoints++);complete(B);compareVariants({A,B,C});
 C.outcomes[1].outcome.rendered+='changed';assert.throws(()=>compareVariants({A,B,C}),/C.outcomes/);
});
test('query result values and trace order must match across all three sides',()=>{
 const A=complete(fixture()),B=complete(fixture()),C=complete(fixture());
 [B.parity[0],B.parity[1]]=[B.parity[1],B.parity[0]];assert.throws(()=>compareVariants({A,B,C}),/B.parity/);
});
test('contracts-only runner retains frozen owners, fixed platform and no resource command',()=>{
 const run=readFileSync(new URL('./run_windows_three_way.mjs',import.meta.url),'utf8');
 for(const text of ["process.platform!=='win32'","mode!=='contracts'","GITHUB_REPOSITORY:'Follen/Lychee-Dev'",'ownedSpawnSync(command,args',"acquireChannel(repository,'symbols'",'terminal.handlesReaped&&finalDependencyCheck',"resources:'NOT_RUN",'compareVariants(results)'])assert(run.includes(text),text);
 for(const text of ['localOwnedSpawn','prlimit','declaration_resource_probe','memoryObservation:true','spawnSync(',"import './rust-build",'channel.release();job.cleanup'])assert(!run.includes(text),text);
});

test('source inventories reject extra empty directories and Python bytecode is frozen',()=>{
 const run=readFileSync(new URL('./run_windows_three_way.mjs',import.meta.url),'utf8');
 assert(run.includes("windows-contract.unexpected_empty_directory"));
 assert(run.includes("rejectEmptyDirectories=true"));
 assert(!run.includes("part==='__pycache__'"));
 assert(run.includes("skip:p=>p.split('/').includes('site-packages')"));
});
