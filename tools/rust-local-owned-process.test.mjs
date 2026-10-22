import test from 'node:test';
import assert from 'node:assert/strict';
import {writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {beginScratch} from './rust-disk.mjs';
import {localOwnedSpawn} from './rust-local-owned-process.mjs';

async function fixture(script,options={}) {
  const job=beginScratch(process.cwd(),'local-owned-process-fixture',{command:['node',script]});
  const path=join(job.directory,'fixture.cjs');writeFileSync(path,script);
  let result;
  try {
    result=await localOwnedSpawn(process.execPath,[path],{cwd:job.directory,evidence:job.evidence,writable:[job.directory],timeout:2000,...options});
    job.seal({state:result.handlesReaped?(result.status===0&&!result.error?'succeeded':'failed'):'interrupted',handlesReaped:result.handlesReaped,logs:result.logs});
    return result;
  }catch(error){if(!result)job.seal({state:'interrupted',handlesReaped:false});throw error;}
}
test('normal exit requires root, namespace and output-reader terminal proof',async()=>{
  const r=await fixture('console.log("owned");console.error("stderr");');
  assert.equal(r.status,0);assert.equal(r.error,undefined);assert.equal(r.stdout.toString(),'owned\n');assert.equal(r.handlesReaped,true);
  assert.equal(r.proof.treeReaped,true);assert.equal(r.proof.outputReadersComplete,true);
});
test('nonzero root is preserved rather than hidden by successful cleanup',async()=>{
  const r=await fixture('process.exit(7)');assert.equal(r.status,7);assert.equal(r.handlesReaped,true);
});
test('detached grandchild holding inherited pipes is reaped with namespace',async()=>{
  const r=await fixture('require("node:child_process").spawn(process.execPath,["-e","setInterval(()=>{},1000)"],{detached:true,stdio:"inherit"}).unref();');
  assert.equal(r.status,0);assert.equal(r.handlesReaped,true);assert.equal(r.error,undefined);
});
test('deadline kills only this namespace and still reports failure',async()=>{
  const r=await fixture('setInterval(()=>{},1000)',{timeout:150});
  assert.equal(r.proof.forced,true);assert.equal(r.handlesReaped,true);assert.match(r.error.message,/local.timeout/);
});
test('output budget cannot become successful terminal evidence',async()=>{
  const r=await fixture('for(let i=0;i<10000;i++)console.log("abcdefgh");setInterval(()=>{},1000)',{maxOutput:128});
  assert.equal(r.proof.forced,true);assert.equal(r.handlesReaped,true);assert.match(r.error.message,/local.output_budget/);
});
test('a prior timeout never hides failed namespace inspection or releases ownership',async()=>{
  const r=await fixture('setInterval(()=>{},1000)',{timeout:150,inspectNamespace(){throw new Error('EACCES');}});
  assert.equal(r.proof.forced,true);assert.equal(r.proof.namespaceVerified,false);assert.equal(r.proof.treeReaped,false);assert.equal(r.handlesReaped,false);
  assert.match(r.error.message,/local.timeout/);assert.match(r.proof.namespaceError,/EACCES/);
});
test('the command cannot forge the bwrap status descriptor',async()=>{
  const r=await fixture('try{require("node:fs").writeSync(3,"forged")}catch(e){console.log(e.code)}');
  assert.equal(r.status,0);assert.equal(r.stdout.toString(),'EINVAL\n');assert.equal(r.handlesReaped,true);
});
