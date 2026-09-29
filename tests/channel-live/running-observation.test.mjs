import {test} from 'node:test';
import assert from 'node:assert/strict';
import {observeRunning} from './running-observation.mjs';

const target={session:'CON-original',guid:'actor'};
const envelope=(stage='prepared',remaining=600000)=>({result:{session:target.session,operation:'LMO-original',identity:{guid:target.guid},operationState:stage,complete:false,continuation:{kind:'continue',remainingBudgetMs:remaining}}});

test('slow preparation resumes original operation beyond eight waits without changing fixture',async()=>{
  let calls=0,clock=0;
  const result=await observeRunning(target,envelope(),async(attempt,wait)=>{
    assert.equal(attempt,calls++);assert.equal(wait,5);clock+=5000;
    return envelope(calls===10?'running':'commit_ready',600000-clock);
  },()=>clock);
  assert.equal(result.result.operationState,'running');assert.equal(calls,10);
});

test('completed or reporting probe never counts as interruption coverage',async()=>{
  for(const stage of ['confirm_ready','result_verified','release_ready','complete','cancelled','execution_unknown']){
    const e=envelope(stage);if(stage==='complete')e.result.complete=true;
    await assert.rejects(observeRunning(target,e,()=>assert.fail('must not resume'),()=>0));
  }
});

test('fixed original deadline is not renewed by subsequent envelopes',async()=>{
  let clock=0,calls=0;
  await assert.rejects(observeRunning(target,envelope('prepared',10000),async()=>{
    clock+=5000;calls++;return envelope('prepared',600000);
  },()=>clock),/budget exhausted/);
  assert.equal(calls,2);
});

test('identity changes and external or exhausted blockers stop without driving',async()=>{
  for(const change of [e=>e.result.session='CON-other',e=>e.result.operation='LMO-other',e=>e.result.identity.guid='other',e=>e.result.continuation.kind='wait_external',e=>e.result.continuation.remainingBudgetMs=0]){
    const first=envelope();let calls=0;
    await assert.rejects(observeRunning(target,first,async()=>{calls++;const next=envelope();change(next);return next;},()=>0));
    assert.equal(calls,1);
  }
});
