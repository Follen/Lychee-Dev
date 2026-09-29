import {test} from 'node:test';
import assert from 'node:assert/strict';
import {verifySkip,verifyCommit} from './slot-skip-evidence.mjs';
const publication=index=>({kind:'slot_published',data:{operation:{request:'b'},transaction:{envelope:{action:'prepare',index}}}});
const commit=(kind,outcome,receipt)=>({kind,data:{operation:{id:'op',attempt:1},input:{kind:'invoke',exchange:'nonce',outcome},transaction:{envelope:{action:'commit',nonce:'nonce'},receipt}}});
const valid=()=>[commit('input_intent'),commit('input_observed',{disposition:'submitted'}),commit('slot_received',null,{state:'accepted'})];
test('skip requires exactly one forward publication',()=>{assert.equal(verifySkip([publication(8)],'b',7),8);for(const e of [[],[publication(7)],[publication(6)],[publication(8),publication(9)]])assert.throws(()=>verifySkip(e,'b',7));});
test('commit proof rejects missing, rejected, uncertain and duplicated input',()=>{assert.equal(verifyCommit(valid(),'op'),'nonce');for(const e of [[],valid().slice(0,2),[...valid(),commit('input_intent')],[commit('input_intent'),commit('input_observed',{disposition:'uncertain'}),commit('receipt',null,{state:'accepted'})],[commit('input_intent'),commit('input_observed',{disposition:'submitted'}),commit('receipt',null,{state:'rejected'})]])assert.throws(()=>verifyCommit(e,'op'));const retry=valid();retry[2].data.operation.attempt=2;assert.throws(()=>verifyCommit(retry,'op'));});
