// Nested only inside the non-breakaway owned Windows Job from the public entry.
import assert from 'node:assert/strict';
import {existsSync, readFileSync, readdirSync, writeFileSync} from 'node:fs';
import {join, resolve} from 'node:path';
import {DatabaseSync} from 'node:sqlite';
import os from 'node:os';
import {beginScratch} from '../../tools/rust-disk.mjs';
import {ownedSpawnSync} from '../../tools/rust-owned-process.mjs';
import {collectOwnedMemory} from '../../tools/rust-owned-memory.mjs';
import {hashFile, sourceDigest, verifyArtifact} from '../../tools/rust-build.mjs';
import {inspectEmmy} from '../../tools/rust-emmy-identity.mjs';
import {cloneSeed, inventory, verifyGoMetadata, verifyGoPackage} from './inputs.mjs';
import {GO_COMMIT, REPETITIONS, orderFor, elapsed} from './contract.mjs';
import {auditGenerationManifest} from './generation-evidence.mjs';
import {auditCache} from './data-contract.mjs';
import {blob, bounded, captureCanonical, decodedIdentity, digest, goJSON, parse, seedDataHome, CSV_COLUMNS, CSV_ROWS, CSV_SQL, numericRow} from './data-inputs.mjs';
import {csvEnvironment, evidenceFiles, loadCSVPreparation} from './data-csv-diagnostics.mjs';
import {parseLossless, auditSqlSemantics, compareSqlSemantics, auditGenerationProof, auditSqlCapture, auditPrepareEnvelope, verifyGenerationArtifact, semanticGeneration, summarizeGeneration} from './generation-contract.mjs';
assert.equal(process.platform,'win32'); assert.equal(process.argv.length,3);
const c=parse(bounded(process.argv[2],1<<20)), out=c.evidence, root=c.root, env=csvEnvironment(process.env);
let fatal=false;
const report={schema:'lycheedev.paired-generation.v1',state:'running',acceptance:c.acceptance,fullB04Acceptance:false,
  scope:'16,384 authenticated synthetic NumericSample rows; production static SQL only, not DataFusion or real-client acceptance',
  cacheDefinition:'Four independently initialized/cloned engine homes per pair: fresh WDC and prepare workflow. OS cache not flushed. Go first workflow query is cold; Rust prepare plus first query is its cold task equivalent. Subsequent query retains own verified state.',
  timingDefinition:'Native CreateProcess through tree reap/output drain. Every explicit Rust prepare is timed and included in prepare-inclusive-first. First-after-prepare is conditional diagnostic, not cold acceptance. No product cost is subtracted.',
  metrics:{memory:'sampled owned Job working set/private bytes, not unique physical RSS',cpu:'not_measured',io:'not_measured'},
  machine:{hostname:os.hostname(),platform:process.platform,arch:process.arch,node:process.version,release:os.release(),cpus:os.cpus(),totalMemory:os.totalmem(),conditions:c.conditions},inputs:c.inputs,producer:c.producer,preparation:[],preflight:[],pairs:[],groups:[]};
const save=()=>writeFileSync(join(out,'report.json'),JSON.stringify(report,null,2)+'\n');
function call(binary,args,evidence,measured=false,environment=env) {
  assert(!fatal,'prior owned handles unresolved'); fatal=true;
  const run=ownedSpawnSync(binary,args,{cwd:root,env:environment,evidence,timeout:330000,orphanMs:30000,memoryObservation:measured});
  fatal=run.handlesReaped!==true;
  const sample={binary,args,exit:run.status,error:run.error?.message,handlesReaped:run.handlesReaped,proof:run.proof,logs:run.logs,stdoutSHA256:digest(run.stdout),stderrSHA256:digest(run.stderr),valid:false};
  if(measured)sample.memory=collectOwnedMemory(run,{evidence,sourceBinary:binary,runState:run.status===0&&!run.error?'passed_invocation':'failed_invocation'});
  return {run,sample};
}
function preparation(binary,args,label,environment=env){const {run,sample}=call(binary,args,out,false,environment);report.preparation.push({label,...sample});save();assert(run.handlesReaped&&!run.error&&run.status===0,label);return run.stdout;}
const gitEnv={...env,GIT_CONFIG_NOSYSTEM:'1',GIT_CONFIG_GLOBAL:'NUL',GIT_TERMINAL_PROMPT:'0',GCM_INTERACTIVE:'never',GIT_NO_REPLACE_OBJECTS:'1'};
for(const key of Object.keys(gitEnv))if(/^GIT_/i.test(key)&&!['GIT_CONFIG_NOSYSTEM','GIT_CONFIG_GLOBAL','GIT_TERMINAL_PROMPT','GIT_NO_REPLACE_OBJECTS'].includes(key))delete gitEnv[key];
function identities(){
  assert.equal(preparation('git',['--no-optional-locks','-c','core.fsmonitor=false','-C',c.request.goSource,'rev-parse','HEAD'],'Go commit',gitEnv).toString().trim(),GO_COMMIT);
  assert.equal(preparation('git',['--no-optional-locks','-c','core.fsmonitor=false','-C',c.request.goSource,'status','--porcelain','--untracked-files=all'],'Go clean',gitEnv).toString().trim(),'');
  assert.equal(verifyGoPackage(c.request.goPackage,c.request.luaLSArchive).packageSHA256,c.go.packageSHA256);
  assert.equal(hashFile(c.request.artifact,65536),c.artifactSHA256);const artifact=verifyArtifact(root,c.request.artifact);assert.deepEqual(artifact,c.rust);assert.equal(sourceDigest(root),c.rust.inputHashes.sourceTree);
  const emmy=inspectEmmy(root);verifyGenerationArtifact(artifact,{sourceRoot:root,sourceTree:sourceDigest(root),binarySHA256:c.rust.binarySHA256,emmy,channel:parse(bounded(join(root,'release/npm-channel.json')))});
  for(const [path,sha] of Object.entries(c.inputs))assert.equal(hashFile(join(c.runnerRoot,path)),sha);
  assert.deepEqual(loadCSVPreparation(c.request.fixturePreparation).provenance,c.producer);
}
const generationDirectory=home=>join(home,'indexes/data-derived-v1/generations');
function noGeneration(home){const path=generationDirectory(home);assert(!existsSync(path)||readdirSync(path).length===0,'fresh home must not have a generation');}
function generationBytes(home,prepared){
  const p=prepared.result;assert.match(p.generation,/^[0-9a-f]{64}$/);assert.match(p.object.sha256,/^[0-9a-f]{64}$/);
  const manifest=bounded(join(generationDirectory(home),p.generation+'.json'),1<<20), object=bounded(join(home,'cache/data-derived-v1/objects',p.object.sha256.slice(0,2),p.object.sha256.slice(2)),64<<20);
  assert.equal(digest(object),p.object.sha256);assert.equal(object.length,p.object.bytes);
  auditGenerationManifest(parse(manifest),p,fixture);
  return {manifestSHA256:digest(manifest),objectSHA256:digest(object),manifestBytes:manifest.length,objectBytes:object.length};
}
let fixture;
function audit(envelope,home,evidence,label,engine,phase,prepared){
  const expected={sql:CSV_SQL,columns:CSV_COLUMNS,rows:Array.from({length:CSV_ROWS},(_,i)=>numericRow(i)),snapshot:fixture.identity.snapshot,pin:fixture.identity.pin,engine};
  auditSqlSemantics(envelope,expected);
  const source=envelope.result.sources[0],file=source.file,table=fixture.identity.tables[0],body=fixture.objects[table.sha256],rootObject=fixture.objects[file.root?.sha256];assert(rootObject&&body);
  assert.deepEqual(file.content,{sha256:table.sha256,bytes:table.bytes});assert.equal(file.entry.fileDataID,404);assert.equal(file.entry.contentKey,body.ckey);assert.equal(file.payloadEncodingKey,body.ekey);assert.equal(file.root.bytes,rootObject.decodedBytes);
  for(const ref of [file.root,file.content])blob(home,ref);
  const cacheMode=phase==='subsequent-hit'||engine==='rust'&&phase==='first-after-prepare'?'warm':'cold';
  const checked=semanticGeneration({kind:'sql',...expected,phase,generation:prepared?.result.generation,object:prepared?.result.object},envelope,engine,cacheMode);
  const reuse=auditCache(file.decodedCache,cacheMode,{encodedBytes:rootObject.encodedBytes+body.encodedBytes,decodedBytes:rootObject.decodedBytes+body.decodedBytes});
  const generationProof=auditGenerationProof(source.typedGenerationCache,{phase:engine==='rust'?phase:'fresh-wdc',generation:prepared?.result.generation,object:prepared?.result.object});
  const cap=envelope.captures[0],db=new DatabaseSync(join(home,'state/toolkit.sqlite'),{readOnly:true});let stored;
  try {const row=db.prepare('SELECT value FROM documents WHERE key=?').get('capture/'+cap.id);assert(row);stored=parseLossless(Buffer.from(row.value));}finally{db.close();}
  assert(cap.blob.bytes<=16<<20,'original SQL capture budget');const bytes=blob(home,cap.blob);auditSqlCapture(envelope,{capture:cap,canonicalBytes:Buffer.from(goJSON(captureCanonical(cap))),blobBytes:bytes,storedCapture:stored});
  assert.equal(cap.provenance.locator,'sql:sha256:'+digest(goJSON({sql:CSV_SQL})));assert.equal(cap.provenance.dataBuild,fixture.identity.pin.fullBuild);
  writeFileSync(join(evidence,label+'-capture.json'),bytes,{flag:'wx'});
  const normalized=checked.value.captures[0];normalized.id='<verified-capture>';normalized.capturedAt='<verified-time>';normalized.blob={sha256:'<verified-full-query>',bytes:'<verified-full-query-bytes>'};
  return {checked:checked.value,observations:checked.observations,generationProof,reuse,capture:{id:cap.id,blob:cap.blob,verified:true},generation:source.typedGenerationCache??null};
}
save();
try {
  identities();report.goBuild=verifyGoMetadata(preparation('go',['version','-m',c.go.binary],'Go executable metadata').toString());
  report.machine.powerBefore=preparation('powercfg.exe',['/getactivescheme'],'power before').toString();
  report.machine.rustc=preparation('rustc',['-Vv'],'Rust toolchain').toString();assert.match(report.machine.rustc,/^rustc 1\.99\.0\b/);
  fixture=loadCSVPreparation(c.request.fixturePreparation).fixture;
  const seeds={};for(const engine of ['go','rust']){const home=join(c.scratch,engine+'-seed'),binary=engine==='go'?c.go.binary:c.rust.sourceBinary;assert(!existsSync(home));assert.equal(parse(preparation(binary,['init','--home',home,'--format','json'],engine+' init')).ok,true);seeds[engine]={home,identity:seedDataHome(fixture,home)};assert.deepEqual(decodedIdentity(home),[]);noGeneration(home);}
  async function pair(iteration,preflight){
    const job=beginScratch(root,`paired-generation-${preflight?'preflight':iteration}`,{command:['data','sql','--sql',CSV_SQL],inputs:{rust:c.rust.binarySHA256,go:c.go.binarySHA256,fixture:fixture.receipt.inventory.sha256},admissionOptions:{requiredBytes:256<<20,scratchReserveBytes:256<<20}});
    const row={iteration,order:orderFor(iteration),preflight,owner:job.receipt,valid:false,ownerRetired:false,fixtureVerified:false,finalIntegrityVerified:false,goCommit:GO_COMMIT,go:{},rust:{}};(preflight?report.preflight:report.pairs).push(row);save();
    const values={},homes={},captures=[];let prepared;
    try {
      for(const engine of ['go','rust'])for(const lane of ['fresh','workflow']){const home=join(job.directory,engine+'-'+lane);cloneSeed(seeds[engine].home,home,seeds[engine].identity);assert.deepEqual(decodedIdentity(home),[]);noGeneration(home);homes[engine+'-'+lane]=home;}
      assert.equal(inventory(fixture.directory,{maxFiles:256,maxBytes:64<<20}).sha256,fixture.receipt.inventory.sha256);row.fixtureVerified=true;
      for(const engine of row.order){
        const binary=engine==='go'?c.go.binary:c.rust.sourceBinary;values[engine]={};
        for(const phase of ['fresh-wdc',...(engine==='rust'?['prepare']:[]),'first-after-prepare','subsequent-hit']){
          const key=({'fresh-wdc':'fresh',prepare:'prepare','first-after-prepare':'first','subsequent-hit':'subsequent'})[phase],home=homes[engine+'-'+(phase==='fresh-wdc'?'fresh':'workflow')];
          const before=decodedIdentity(home),generationBefore=prepared&&engine==='rust'&&phase!=='fresh-wdc'?generationBytes(home,prepared):null;
          const args=[...(phase==='prepare'?['data','cache','prepare','NumericSample']:['data','sql','--sql',CSV_SQL]),'--snapshot',fixture.identity.snapshot,'--installation',fixture.identity.installation,'--home',home,'--format','json','--offline'];
          const {run,sample}=call(binary,args,job.evidence,true);row[engine][key]={...sample,home,snapshot:fixture.identity.snapshot};save();
          assert(run.handlesReaped&&!run.error&&run.status===0,`${engine} ${phase}`);assert.equal(run.stderr.length,0);Object.assign(row[engine][key],elapsed(run.proof),{terminalVerified:true});
          const envelope=parseLossless(run.stdout);
          if(phase==='prepare'){
            row[engine][key].prepareAudit=auditPrepareEnvelope(envelope,{snapshot:fixture.identity.snapshot,table:'NumericSample',rows:CSV_ROWS,definitionSHA256:fixture.identity.tables[0].dbdSHA256});
            assert.equal(envelope.ok,true);assert.equal(envelope.error,null);const p=envelope.result;assert.equal(p.generationPublished,true);assert.equal(p.sourceKind,'static');assert.equal(p.snapshot,fixture.identity.snapshot);assert.equal(p.table,'NumericSample');assert.equal(p.availableRows,CSV_ROWS);assert.equal(p.complete,true);assert.deepEqual(p.missing,[]);assert.deepEqual(p.unavailablePartitions,[]);assert.equal(p.recordIdOrder,'ascending');assert.equal(p.definitionSHA256,fixture.identity.tables[0].dbdSHA256);assert.equal(p.representation,'arrow-ipc-file-v5/arrow-58.3.0/uncompressed');assert.equal(p.derivationVersion,'lycheedev.table-arrow-import.v1');assert.equal(p.generationReused,undefined);prepared=envelope;
            row[engine][key].generation=generationBytes(home,prepared);row[engine][key].receiptVerified=true;
            writeFileSync(join(job.evidence,'generation-manifest.json'),bounded(join(generationDirectory(home),p.generation+'.json'),1<<20),{flag:'wx'});
            writeFileSync(join(job.evidence,'generation-object.arrow'),bounded(join(home,'cache/data-derived-v1/objects',p.object.sha256.slice(0,2),p.object.sha256.slice(2)),64<<20),{flag:'wx'});
          }else{
            const audited=audit(envelope,home,job.evidence,engine+'-'+key,engine,phase,prepared);values[engine][key]=audited.checked;captures.push({home,cap:envelope.captures[0]});
            Object.assign(row[engine][key],{reuse:audited.reuse,capture:audited.capture,generation:audited.generation,generationProof:audited.generationProof,captureVerified:true,observations:audited.observations});
            if(generationBefore)assert.deepEqual(generationBytes(home,prepared),generationBefore,'query changed immutable generation');else noGeneration(home);
          }
          const after=decodedIdentity(home);assert.equal(after.length,2);if(before.length)assert.deepEqual(after,before);row[engine][key].valid=true;save();
        }
      }
      for(const key of ['fresh','first','subsequent']){compareSqlSemantics(values.go[key],values.rust[key]);row.go[key].resultEquivalent=true;row.rust[key].resultEquivalent=true;}
      for(const engine of ['go','rust']){compareSqlSemantics(values[engine].fresh,values[engine].first);compareSqlSemantics(values[engine].first,values[engine].subsequent);}
      for(const {home,cap} of captures){blob(home,cap.blob);const db=new DatabaseSync(join(home,'state/toolkit.sqlite'),{readOnly:true});try{const stored=db.prepare('SELECT value FROM documents WHERE key=?').get('capture/'+cap.id);assert(stored);assert.deepEqual(parseLossless(Buffer.from(stored.value)),cap);}finally{db.close();}}
      assert.deepEqual(generationBytes(homes['rust-workflow'],prepared),row.rust.prepare.generation);
      assert.equal(inventory(fixture.directory,{maxFiles:256,maxBytes:64<<20}).sha256,fixture.receipt.inventory.sha256);identities();row.finalIntegrityVerified=true;row.valid=true;
    }catch(error){row.failure=error.message.slice(0,4096);}finally{
      writeFileSync(join(job.evidence,'pair.json'),JSON.stringify(row,null,2)+'\n');
      job.seal({state:fatal?'interrupted':row.valid?'succeeded':'failed',handlesReaped:!fatal,logs:evidenceFiles(job.evidence)});
      if(!fatal&&row.valid){job.cleanup();row.ownerRetired=true;}save();
    }
    return row.valid&&row.ownerRetired;
  }
  if(c.acceptance)assert(await pair(0,true),'preflight failed');
  for(let i=0;i<(c.acceptance?REPETITIONS:1);i++)assert(await pair(i,false),'pair failed');
  identities();report.machine.powerAfter=preparation('powercfg.exe',['/getactivescheme'],'power after').toString();
  assert.equal(report.machine.powerAfter,report.machine.powerBefore,'active power scheme changed');
  report.groups=summarizeGeneration(report.pairs,{acceptance:c.acceptance});
  const valid=report.groups.every(g=>!g.state.startsWith('failed')&&g.state!=='not_run');
  report.state=valid?(c.acceptance?'passed_selected_generation_paths':'equivalent_generation_observation_only'):'failed';
}catch(error){report.state='failed';report.failure=error.message.slice(0,4096);}finally{report.handlesReaped=!fatal;save();}
if(report.state==='failed')process.exitCode=1;
