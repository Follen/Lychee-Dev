import test from 'node:test';
import assert from 'node:assert/strict';
import {publicationRetry,dependencyChanged} from './shared-publication-retry.mjs';

const target={session:'CON-a',installation:'D:/Game'};
const consumers=new Set(['1/10','2/20']);
const pending=()=>({session:'CON-a',operationState:'prepared',continuation:{session:'CON-a',kind:'wait_external',remainingBudgetMs:1000,blocker:{kind:'slot_reservation',installation:'D:\\Game\\Interface\\AddOns',consumer:'2/20',runtime:'r',nonce:'n',slot:1}}});
const pool=()=>({files:Array.from({length:64},()=>({consumer:'2/20',runtime:'r',nonce:'n',consumed:false}))});

test('only exact owned temporary publication blockers qualify',()=>{
  assert.ok(publicationRetry(pending(),target,consumers));
  for(const mutate of [
    r=>r.session='CON-other',r=>r.continuation.session='CON-other',
    r=>r.operationState='execution_unknown',r=>r.continuation.kind='budget_exhausted',
    r=>r.continuation.kind='needs_decision',r=>r.continuation.remainingBudgetMs=0,
    r=>delete r.continuation.remainingBudgetMs,r=>r.continuation.blocker.consumer='3/30',
    r=>r.continuation.blocker.kind='active_driver',r=>r.continuation.blocker.installation='D:/Other',
    r=>r.continuation.blocker.slot=65,
  ]){const r=pending();mutate(r);assert.equal(publicationRetry(r,target,consumers),null);}
});

test('unchanged reservation or unrelated owner progress never unlocks resume',()=>{
  const b=pending().continuation.blocker,p=pool();
  assert.equal(dependencyChanged(b,p,JSON.stringify(p),true),false);
  p.files[1].consumed=true;
  assert.equal(dependencyChanged(b,p,'older',true),false);
  p.files[0].consumed=true;
  assert.equal(dependencyChanged(b,p,'older',false),true);
  p.files[0]={nonce:'new',consumer:'1/10',runtime:'new-runtime'};
  assert.equal(dependencyChanged(b,p,'older',false),true);
});

test('short lease requires observed installation or peer-call progress',()=>{
  const b={kind:'publication_lock'},p=pool(),before=JSON.stringify(p);
  assert.equal(dependencyChanged(b,p,before,false),false);
  assert.equal(dependencyChanged(b,p,before,true),true);
  p.files[0].consumed=true;
  assert.equal(dependencyChanged(b,p,before,false),true);
  assert.equal(dependencyChanged(b,{files:[]},before,true),false);
});
