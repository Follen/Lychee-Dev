import test from 'node:test';
import assert from 'node:assert/strict';
import {existsSync,mkdtempSync,readFileSync,rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {ORACLE_TIMEOUT_MS,runControlledOracle} from './controlled-run.mjs';

function fixture(t){
  const root=mkdtempSync(join(tmpdir(),'emmy-orchestrator-'));
  t.after(()=>rmSync(root,{recursive:true,force:true}));
  return root;
}
test('whole oracle has one bounded owner without changing nested product budgets',t=>{
  const root=fixture(t);
  const result=runControlledOracle(root,(command,args,options)=>{
    assert.equal(command,process.execPath);
    assert.deepEqual(args,[join(root,'tests/rust-source-engine/fixed-emmy-oracle/run.mjs')]);
    assert.equal(options.timeout,ORACLE_TIMEOUT_MS);
    assert.equal(options.orphanMs,30000);
    assert.equal(options.cwd,root);
    assert.equal(options.env,process.env);
    return {status:0,handlesReaped:true,logs:[]};
  });
  assert.equal(result.state,'passed');
  const receipt=JSON.parse(readFileSync(result.receipt));
  assert.equal(receipt.state,'retired');
  assert.equal(receipt.handlesReaped,true);
  assert(!existsSync(receipt.directory));
  assert.equal(receipt.sealedLogs.length,1);
});
test('semantic exit and forced descendant cleanup remain failures after complete retirement',t=>{
  const root=fixture(t);
  for(const outcome of [{status:1},{status:0,error:new Error('owned.forced_tree_termination')}]){
    const result=runControlledOracle(root,()=>({...outcome,handlesReaped:true,logs:[]}));
    assert.equal(result.state,'failed');
    assert.equal(result.handlesReaped,true);
    const receipt=JSON.parse(readFileSync(result.receipt));
    assert.equal(receipt.state,'failed');
    assert.equal(receipt.handlesReaped,true);
    assert(existsSync(receipt.directory));
  }
});
test('unknown process retirement and execution exceptions preserve scratch and fail closed',t=>{
  const root=fixture(t);
  for(const execute of [()=>({status:0,handlesReaped:false,logs:[]}),()=>{throw new Error('spawn failed');}]){
    const result=runControlledOracle(root,execute);
    assert.equal(result.state,'interrupted');
    assert.equal(result.handlesReaped,false);
    const receipt=JSON.parse(readFileSync(result.receipt));
    assert.equal(receipt.state,'interrupted');
    assert(existsSync(receipt.directory));
  }
});
