import assert from 'node:assert/strict';
export function verifySkip(events,request,blockedSlot){
 const published=events.filter(e=>e.kind==='slot_published'&&e.data?.operation?.request===request&&e.data?.transaction?.envelope?.action==='prepare');
 assert.equal(published.length,1,'one published prepare required');
 assert.ok(published[0].data.transaction.envelope.index>blockedSlot,'peer must skip reserved slot');
 return published[0].data.transaction.envelope.index;
}
export function verifyCommit(events,operation){
 const own=events.filter(e=>e.data?.operation?.id===operation);
 assert.ok(own.length>0,'operation evidence missing');
 for(const e of own)assert.equal(e.data.operation.attempt,1,'unexpected business retry');
 const commits=own.filter(e=>e.kind==='input_intent'&&e.data?.transaction?.envelope?.action==='commit'&&e.data?.input?.kind==='invoke');
 assert.equal(commits.length,1,'one commit input intent');
 const nonce=commits[0].data.transaction.envelope.nonce;
 assert.equal(commits[0].data.input.exchange,nonce,'commit input must match nonce');
 assert.ok(own.some(e=>e.kind==='input_observed'&&e.data?.input?.exchange===nonce&&e.data?.input?.outcome?.disposition==='submitted'),'commit submission evidence required');
 assert.ok(own.some(e=>e.data?.transaction?.envelope?.nonce===nonce&&e.data?.transaction?.receipt?.state==='accepted'),'accepted commit receipt required');
 return nonce;
}
