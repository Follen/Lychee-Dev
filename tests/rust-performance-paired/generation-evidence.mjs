// Final revalidation is independent of success strings/booleans in the child.
import assert from 'node:assert/strict';
import {verifyGenerationPhaseEvidence, verifyGenerationProfileProduct} from './generation-profile.mjs';
import {elapsed} from './contract.mjs';
import {parseLossless, summarizeGeneration} from './generation-contract.mjs';
export function verifyGenerationCompletion(run, observed, acceptance, {profiled=false,product=null}={}) {
  if(profiled){assert.equal(acceptance,false,'profiled lane cannot grant acceptance');assert.deepEqual(observed.product,product);assert.equal(observed.profiled,true);const e=observed.productEvidence;assert.deepEqual(Object.keys(e).sort(),['commit','sourceTreeSHA256','binarySHA256','status'].sort());verifyGenerationProfileProduct(product,{inputHashes:{sourceTree:e.sourceTreeSHA256},binarySHA256:e.binarySHA256},e.commit,e.status);}else{assert(observed.profiled===undefined||observed.profiled===false);assert(observed.product===undefined||observed.product===null);}
  assert(run.handlesReaped===true&&!run.error&&run.status===0,'successful outer owner required');
  elapsed(run.proof);
  assert.equal(observed?.schema,profiled?'lycheedev.paired-generation-profile.v1':'lycheedev.paired-generation.v1');assert.equal(observed.handlesReaped,true);assert.equal(observed.acceptance,acceptance);assert.equal(observed.fullB04Acceptance,false);
  assert.equal(observed.state,profiled?'equivalent_profiled_generation_observation_only':acceptance?'passed_selected_generation_paths':'equivalent_generation_observation_only');
  assert.equal(observed.preflight.length,acceptance?1:0,'formal preflight is separate from measured20; observation exactly one pair');
  for(const pair of [...observed.preflight,...observed.pairs])for(const engine of ['go','rust'])for(const phase of ['fresh',...(engine==='rust'?['prepare']:[]),'first','subsequent']){
    const s=pair[engine][phase];assert.equal(s.exit,0);assert(!s.error);assert.equal(s.handlesReaped,true);assert.equal(s.valid,true);
    if(profiled&&engine==='rust')verifyGenerationPhaseEvidence(s,phase);else assert.equal(s.phaseDiagnostics,undefined,'ordinary/Go samples cannot inherit profile evidence');
    const measured=elapsed(s.proof);for(const [key,value]of Object.entries(measured))assert.equal(s[key],value,'native sample timing must not be replaced/subtracted');
    assert.match(s.stdoutSHA256,/^[0-9a-f]{64}$/);assert.match(s.stderrSHA256,/^[0-9a-f]{64}$/);
  }
  if(acceptance)assert(summarizeGeneration(observed.preflight,{acceptance:false}).every(g=>g.correctAndComplete));
  const groups=summarizeGeneration(observed.pairs,{acceptance});assert.deepEqual(observed.groups,groups);assert(groups.every(g=>g.correctAndComplete&&!g.state.startsWith('failed')));
  assert.equal(observed.machine.powerAfter,observed.machine.powerBefore,'power scheme changed');
  return true;
}

// Manifest wire camelCase differs from the public prepare SHA256 field names.
export function auditGenerationManifest(m,p,fixture) {
assert.equal(m.schema,'lycheedev.data-static-generation.v1');assert.equal(m.identitySha256,p.generation);assert.deepEqual(m.object,p.object);
  assert.equal(m.sourceKind,'static');assert.deepEqual(m.data,fixture.identity.pin);assert.deepEqual(m.table,{name:'NumericSample',hash:0xf00d0194,db2FileDataId:404,dbcFileDataId:0});assert.equal(m.definitionSha256,fixture.identity.tables[0].dbdSHA256);assert.deepEqual(m.sourceContent,{sha256:fixture.identity.tables[0].sha256,bytes:fixture.identity.tables[0].bytes});
  assert.deepEqual(Object.keys(m).sort(),['schema','identitySha256','sourceKind','data','table','definitionSha256','typedSchemaSha256','object','representation','derivationVersion','availableRows','batchCount','complete','missing','unavailablePartitions','recordIdColumn','recordIdOrder','sourceContent','partialContent','definitionObject','definitionManifest'].sort());
  assert.equal(m.partialContent,null);
  const definitions=fixture.documents.filter(d=>d.key.startsWith('definition/')).map(d=>parseLossless(d.bytes));
  assert.deepEqual(m.definitionObject,definitions.find(d=>d.url.endsWith('/definitions/NumericSample.dbd')).blob);assert.deepEqual(m.definitionManifest,definitions.find(d=>d.url.endsWith('/manifest.json')).blob);
  assert.equal(m.typedSchemaSha256,p.typedSchemaSHA256);
  for(const key of ['representation','derivationVersion','availableRows','batchCount','complete','missing','unavailablePartitions','recordIdColumn','recordIdOrder'])assert.deepEqual(m[key],p[key],`manifest ${key}`);
  return true;
}
