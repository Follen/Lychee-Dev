import test from 'node:test';
import assert from 'node:assert/strict';
import {verifyGenerationCompletion} from './generation-evidence.mjs';
import {summarizeGeneration,GO_COMMIT} from './generation-contract.mjs';
import {elapsed} from './contract.mjs';
const hash='a'.repeat(64),object={sha256:'b'.repeat(64),bytes:100};
function proof(ms){return {schema:'lycheedev.owned-process.v1',rootReaped:true,treeReaped:true,handlesClosed:true,toolIdentityLocksClosed:true,outputReadersComplete:true,activeProcesses:0,forced:false,timing:{schema:'lycheedev.owned-timing.v1',frequency:1000000,startTicks:'1000000',rootExitTicks:String(1000000+ms*1000),treeReapedTicks:String(1000000+ms*1000)}};}
function sample(engine,phase){const p=proof(engine==='go'?100:phase==='prepare'?5:20);return {proof:p,...elapsed(p),exit:0,handlesReaped:true,valid:true,terminalVerified:true,captureVerified:true,resultEquivalent:true,home:engine+'/'+(phase==='fresh'?'fresh':'workflow'),binary:engine+'.exe',snapshot:'PIN-fixed',stdoutSHA256:hash,stderrSHA256:hash,...(phase==='prepare'?{receiptVerified:true}:{}),...(engine==='rust'?{generationProof:{verified:true,reused:phase!=='fresh',...(phase!=='fresh'?{generation:hash,object}:{})}}:{})};}
function valid(){const pair={iteration:0,order:['go','rust'],valid:true,ownerRetired:true,fixtureVerified:true,finalIntegrityVerified:true,goCommit:GO_COMMIT};for(const engine of ['go','rust'])pair[engine]=Object.fromEntries(['fresh',...(engine==='rust'?['prepare']:[]),'first','subsequent'].map(phase=>[phase,sample(engine,phase)]));const observed={schema:'lycheedev.paired-generation.v1',handlesReaped:true,acceptance:false,fullB04Acceptance:false,state:'equivalent_generation_observation_only',preflight:[],pairs:[pair],groups:summarizeGeneration([pair]),machine:{powerBefore:'balanced',powerAfter:'balanced'}};return {run:{status:0,handlesReaped:true,proof:proof(500)},observed};}
test('final verifier accepts only complete native-clock observation evidence',()=>{const {run,observed}=valid();assert(verifyGenerationCompletion(run,observed,false));});
for(const [name,mutate] of Object.entries({
  outerUnreaped:x=>x.run.handlesReaped=false,outerFailure:x=>x.run.status=1,outerForced:x=>x.run.proof.forced=true,
  nestedUnreaped:x=>x.observed.pairs[0].rust.first.proof.treeReaped=false,
  readers:x=>x.observed.pairs[0].rust.first.proof.outputReadersComplete=false,
  handles:x=>x.observed.pairs[0].rust.first.proof.handlesClosed=false,
  locks:x=>x.observed.pairs[0].rust.first.proof.toolIdentityLocksClosed=false,
  active:x=>x.observed.pairs[0].rust.first.proof.activeProcesses=1,
  deadline:x=>x.observed.pairs[0].rust.first.proof.deadlineHit=true,
  outputLimit:x=>x.observed.pairs[0].rust.first.proof.outputLimit=true,
  subtractedTime:x=>x.observed.pairs[0].rust.prepare.controllableMillis=0.1,
  resultMissing:x=>x.observed.pairs[0].rust.first.stdoutSHA256='',
  cleanup:x=>x.observed.pairs[0].ownerRetired=false,
  power:x=>x.observed.machine.powerAfter='performance',
  fakeAcceptance:x=>x.observed.acceptance=true,
  fakeGroups:x=>x.observed.groups=[],
  missingPrepare:x=>delete x.observed.pairs[0].rust.prepare,
  extraPair:x=>x.observed.pairs.push(structuredClone(x.observed.pairs[0])),
}))test(`final verifier rejects ${name}`,()=>{const x=valid();mutate(x);assert.throws(()=>verifyGenerationCompletion(x.run,x.observed,false));});

import {auditGenerationManifest} from './generation-evidence.mjs';
function manifestFixture(){
  const p={generation:hash,object,typedSchemaSHA256:'c'.repeat(64),representation:'arrow-ipc-file-v5/arrow-58.3.0/uncompressed',derivationVersion:'lycheedev.table-arrow-import.v1',availableRows:16384,batchCount:16,complete:true,missing:[],unavailablePartitions:[],recordIdColumn:5,recordIdOrder:'ascending'};
  const pin={product:'retail',fullBuild:'12.1.0.69933'},definition={sha256:'d'.repeat(64),bytes:100},catalog={sha256:'e'.repeat(64),bytes:200};
  const fixture={identity:{pin,tables:[{dbdSHA256:definition.sha256,sha256:'f'.repeat(64),bytes:327933}]},documents:[{key:'definition/a',bytes:Buffer.from(JSON.stringify({url:'https://fixture/definitions/NumericSample.dbd',blob:definition}))},{key:'definition/b',bytes:Buffer.from(JSON.stringify({url:'https://fixture/manifest.json',blob:catalog}))}]};
  const m={schema:'lycheedev.data-static-generation.v1',identitySha256:hash,sourceKind:'static',data:pin,table:{name:'NumericSample',hash:0xf00d0194,db2FileDataId:404,dbcFileDataId:0},definitionSha256:definition.sha256,typedSchemaSha256:p.typedSchemaSHA256,object,...Object.fromEntries(['representation','derivationVersion','availableRows','batchCount','complete','missing','unavailablePartitions','recordIdColumn','recordIdOrder'].map(k=>[k,p[k]])),sourceContent:{sha256:'f'.repeat(64),bytes:327933},partialContent:null,definitionObject:definition,definitionManifest:catalog};return {m,p,fixture};
}
test('manifest serde Sha256 maps explicitly to public prepare SHA256',()=>{const {m,p,fixture}=manifestFixture();assert.equal(p.typedSchemaSha256,undefined);assert(auditGenerationManifest(m,p,fixture));});
for(const [name,change]of Object.entries({
 casing:x=>{x.m.typedSchemaSHA256=x.m.typedSchemaSha256;delete x.m.typedSchemaSha256;},
 schemaDigest:x=>x.m.typedSchemaSha256='d'.repeat(64),source:x=>x.m.sourceContent.bytes++,
 definition:x=>x.m.definitionObject.sha256='a'.repeat(64),catalog:x=>x.m.definitionManifest.bytes++,
 coverage:x=>x.m.complete=false,missing:x=>x.m.missing=['key'],partial:x=>x.m.partialContent={},
 table:x=>x.m.table.db2FileDataId++,pin:x=>x.m.data={product:'classic'},
 generation:x=>x.m.identitySha256='b'.repeat(64),batch:x=>x.m.batchCount++,
 extra:x=>x.m.unreviewed=true,rows:x=>x.m.availableRows--,
}))test(`manifest rejects ${name}`,()=>{const x=manifestFixture();x.m=structuredClone(x.m);change(x);assert.throws(()=>auditGenerationManifest(x.m,x.p,x.fixture));});
