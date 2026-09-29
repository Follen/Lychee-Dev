// Runner policy only. Observed dependency changes permit another ordinary
// resume; they never grant input authority or retire another owner's payload.
export function publicationRetry(result, target, consumers) {
  const c=result?.continuation,b=c?.blocker;
  if(result?.session!==target.session||c?.session!==target.session||
    c.kind!=='wait_external'||!Number.isFinite(c.remainingBudgetMs)||c.remainingBudgetMs<=0||
    result.operationState==='execution_unknown'||!['slot_reservation','publication_lock'].includes(b?.kind))return null;
  const normalize=s=>String(s).replaceAll('\\','/').replace(/\/$/,'').toLowerCase();
  if(normalize(b.installation)!==normalize(target.installation+'/Interface/AddOns'))return null;
  if(b.kind==='slot_reservation'&&(!consumers.has(b.consumer)||!b.nonce||!b.runtime||!Number.isInteger(b.slot)||b.slot<1||b.slot>64))return null;
  return b;
}

export function dependencyChanged(blocker, pool, beforePool, peerFinished) {
  if(!Array.isArray(pool?.files)||pool.files.length!==64)return false;
  if(blocker.kind==='publication_lock')return peerFinished||JSON.stringify(pool)!==beforePool;
  const slot=pool.files[blocker.slot-1];
  return !!slot&&(slot.nonce!==blocker.nonce||slot.consumer!==blocker.consumer||
    slot.runtime!==blocker.runtime||slot.consumed===true||!!slot.retiredRuntime||slot.retiredProcess===true);
}
