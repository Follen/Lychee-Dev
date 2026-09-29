import assert from 'node:assert/strict';

// Wait only for the already-created probe to reach its fault-injection window.
// The callback resumes the original CON; it must never execute a new request.
export async function observeRunning(target, envelope, resume, now=()=>performance.now()) {
  const operation=envelope.result?.operation;
  assert.ok(operation,'missing original operation');
  let deadline=now()+600000;
  for(let attempt=0;;attempt++) {
    const r=envelope.result;
    assert.equal(r.session,target.session,'running observation changed connection');
    assert.equal(r.operation,operation,'running observation changed operation');
    assert.equal(r.identity.guid,target.guid,'running observation changed actor');
    assert.equal(r.complete,false,'probe completed before the required running observation');
    assert.ok(!['confirm_ready','result_verified','release_ready','complete','cancelled','execution_unknown'].includes(r.operationState),
      'probe left the running window before fault injection');
    const continuation=r.continuation;
    assert.ok(continuation && Number.isFinite(continuation.remainingBudgetMs),'missing durable recovery budget');
    deadline=Math.min(deadline,now()+continuation.remainingBudgetMs);
    assert.ok(now()<deadline,'original running-observation budget exhausted');
    assert.equal(continuation.kind,'continue','resolve the original blocker before awaiting running');
    if(r.operationState==='running')return envelope;
    const wait=Math.max(1,Math.min(5,Math.floor((deadline-now())/1000)));
    envelope=await resume(attempt,wait);
  }
}
