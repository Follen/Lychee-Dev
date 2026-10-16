// Fixed historical semantic oracle followed by the unmodified-engine candidate.
// This is a test-only composition of the existing controlled Cargo/owner boundary.
import assert from 'node:assert/strict';
import {isDeepStrictEqual} from 'node:util';
import {existsSync,lstatSync,mkdirSync,readFileSync,readdirSync,writeFileSync} from 'node:fs';
import {dirname,join,relative,resolve} from 'node:path';
import {beginScratch,contained,sha256} from '../../../tools/rust-disk.mjs';
import {hashFile,runCargo,sourceDigest} from '../../../tools/rust-build.mjs';
import {inspectEmmy} from '../../../tools/rust-emmy-identity.mjs';
import {ownedSpawnSync} from '../../../tools/rust-owned-process.mjs';

const HERE=dirname(import.meta.filename);
const SCHEMA='lycheedev.emmy-fixed-oracle.v1';
const OWNER='ci-emmy-selected-oracle';
const CLEAN='375d92c5d5e503d77232fabf22a21a82f504c2b38dc5def8fcca1d6ecf06e104';
const HISTORY='cc0a3496bad44f2dfede0799b86175dc3b3332587097bacf6ed691648130fcff';
const MAX_TRACE=16<<20;
const readJSON=path=>{assert(lstatSync(path).isFile()&&!lstatSync(path).isSymbolicLink()&&lstatSync(path).size<=MAX_TRACE,'oracle.file_budget');return JSON.parse(readFileSync(path,'utf8'));};
const save=(path,value)=>writeFileSync(path,JSON.stringify(value,null,2)+'\n');

export function validateFrames(frames,{target,test,role}){
  assert(Array.isArray(frames)&&frames.length>=2,'oracle.missing_frames');
  assert.deepEqual(frames[0],{kind:'header',schema:SCHEMA,role,target,test},'oracle.header');
  const rows=frames.slice(1,-1);
  assert(rows.length>0,'oracle.empty_test');
  assert.deepEqual(frames.at(-1),{kind:'end',rows:rows.length,complete:true},'oracle.incomplete');
  for(const [ordinal,row] of rows.entries()){
    assert.deepEqual(Object.keys(row).sort(),['kind','label','ordinal','value'],'oracle.row_fields');
    assert(row.kind==='row'&&row.ordinal===ordinal&&typeof row.label==='string'&&typeof row.value==='string','oracle.row');
  }
  return rows;
}

export function readTraces(directory,targets,role){
  const expected=targets.flatMap(({target,tests})=>tests.map(test=>({target,test,file:`${target}--${test}.jsonl`})));
  assert.deepEqual(readdirSync(directory).sort(),expected.map(e=>e.file).sort(),'oracle.test_inventory');
  return expected.map(entry=>{
    const path=join(directory,entry.file),info=lstatSync(path);
    assert(info.isFile()&&!info.isSymbolicLink()&&info.size<=MAX_TRACE,'oracle.trace_budget');
    const text=readFileSync(path,'utf8');
    assert(text.endsWith('\n'),'oracle.truncated_line');
    const rows=validateFrames(text.slice(0,-1).split('\n').map(line=>JSON.parse(line)),{...entry,role});
    return {...entry,path,bytes:info.size,sha256:hashFile(path,MAX_TRACE),rows};
  });
}

// Continue all original targets after a semantic failure, preserving the final
// nonzero Cargo status on each side. This is Cargo's switch, not a libtest flag.
export function oracleCargoArgs(targets){
  return ['test','--locked','--offline','--no-fail-fast','-p','lycheedev-source',
    ...targets.flatMap(t=>['--test',t.target]),'-j','1','--','--test-threads=1'];
}

// Full raw rows remain in their separately sealed bounded traces. Summarize all
// differing ordinals while bounding duplicate report data; no field allowlist.
export function traceDifferences(reference,candidate){
  assert.equal(candidate.length,reference.length,'oracle.trace_pair_inventory');
  const differences=[];
  for(let i=0;i<reference.length;i++){
    const old=reference[i],actual=candidate[i];
    assert.equal(actual.file,old.file,'oracle.trace_pair_identity');
    let differingRows=0;const first=[];
    for(let ordinal=0;ordinal<Math.max(old.rows.length,actual.rows.length);ordinal++){
      const expected=old.rows[ordinal],value=actual.rows[ordinal];
      if(isDeepStrictEqual(value,expected))continue;
      differingRows++;
      if(first.length<32)first.push({ordinal,referenceLabel:expected?.label??null,candidateLabel:value?.label??null,
        referenceSHA256:expected?sha256(Buffer.from(JSON.stringify(expected))):null,
        candidateSHA256:value?sha256(Buffer.from(JSON.stringify(value))):null});
    }
    if(differingRows)differences.push({file:old.file,referenceRows:old.rows.length,candidateRows:actual.rows.length,
      differingRows,first,moreDifferingRows:differingRows-first.length});
  }
  return differences;
}

function harnessInputs(){
  const manifest=readJSON(join(HERE,'inputs.json'));
  assert.equal(manifest.schema,'lycheedev.emmy-fixed-inputs.v1');
  assert.equal(manifest.referenceCommit,'cd87118123fcba3d26f8ee161166629a4570bdaa');
  assert.equal(manifest.targets.length,10);
  assert.equal(manifest.targets.flatMap(t=>t.tests).length,36);
  assert.equal(manifest.files.length,31,'oracle.input_inventory');
  assert.equal(new Set(manifest.files.map(f=>f.path)).size,manifest.files.length);
  assert.equal(manifest.historicalCompileCompatibility.schema,'lycheedev.emmy-fixed-compile-compatibility.v1');
  assert.equal(manifest.historicalCompileCompatibility.identity,'cd871181-compile-compatibility-r1');
  assert.equal(manifest.historicalCompileCompatibility.files.length,10);
  for(const row of manifest.files){
    const path=contained(HERE,resolve(HERE,row.path));
    assert.equal(lstatSync(path).size,row.bytes,`oracle.input_bytes:${row.path}`);
    assert.equal(hashFile(path,8<<20),row.sha256,`oracle.input_hash:${row.path}`);
  }
  return {manifest,sha256:hashFile(join(HERE,'inputs.json'),2<<20)};
}

function childReaped(root,receipt){
  const record=readJSON(contained(root,resolve(receipt)));
  assert.equal(record.owner,OWNER,'oracle.foreign_cargo_owner');
  return record.handlesReaped===true&&['succeeded','failed','retired'].includes(record.state);
}

export function run(root=process.cwd()){
  assert.equal(process.platform,'win32','oracle.windows_controlled_owner_required');
  root=resolve(root);
  const clean=inspectEmmy(root),inputs=harnessInputs();
  assert.equal(clean.identity,'public-upstream-unmodified-v1');
  assert.equal(clean.patchedTreeSHA256,CLEAN);
  // A caller-provided single-tree override cannot silently leak into the other side.
  assert.equal(process.env.LYCHEEDEV_EMMY_CONTEXT_PATCH_SHA256,undefined,'oracle.external_tree_override');
  const initialSource=sourceDigest(root);
  const job=beginScratch(root,OWNER,{command:['node',relative(root,import.meta.filename)],
    inputs:{sourceTree:initialSource,fixedInputs:inputs.sha256,cleanTree:CLEAN,referenceTree:HISTORY},
    admissionOptions:{requiredBytes:2*1024**3,cacheReserveBytes:2*1024**3}});
  const stage=join(job.directory,'reference-source'),referenceDir=join(job.evidence,'reference'),candidateDir=join(job.evidence,'candidate');
  mkdirSync(referenceDir);mkdirSync(candidateDir);
  const reportPath=join(job.evidence,'fixed-oracle-report.json');
  const report={schema:'lycheedev.emmy-fixed-run.v1',state:'running',referenceCommit:inputs.manifest.referenceCommit,
    fixedInputsSHA256:inputs.sha256,production:{identity:clean.identity,treeSHA256:CLEAN,files:clean.files.length,bytes:clean.totalBytes},
    reference:{treeSHA256:HISTORY,sourceIdentity:'fixed-source-plus-compile-compatibility',
      compileCompatibility:{...inputs.manifest.historicalCompileCompatibility,
        files:inputs.manifest.historicalCompileCompatibility.files.map(({edits,...row})=>row)}},
    sourceTree:initialSource,tests:36,cargoInvoked:false,invocations:[]};
  let handlesReaped=true,succeeded=false;
  const logs=[reportPath];
  const stable=()=>{assert.equal(harnessInputs().sha256,inputs.sha256,'oracle.harness_changed');assert.equal(sourceDigest(root),initialSource,'oracle.production_changed');};
  save(reportPath,report);
  try{
    handlesReaped=false;
    const prepared=ownedSpawnSync('python',['-I','-B','-S',join(HERE,'materialize.py'),'--root',root,'--stage',stage,'--receipt',job.receipt],
      {cwd:root,env:process.env,evidence:job.evidence,timeout:300000,maxBuffer:8<<20});
    logs.push(...(prepared.logs||[]));handlesReaped=prepared.handlesReaped===true;
    assert(handlesReaped&&!prepared.error&&prepared.status===0,'oracle.reference_materialize_failed');
    const historical=inspectEmmy(stage);
    assert.equal(historical.identity,'public-base-binding-context-reconstruction-v1');
    assert.equal(historical.patchedTreeSHA256,HISTORY);
    report.reference.recipeSHA256=historical.recipeSHA256;
    report.reference.sourceTree=sourceDigest(stage);
    stable();
    const args=oracleCargoArgs(inputs.manifest.targets);
    const invoke=(sourceRoot,environment,role)=>{
      handlesReaped=false;
      try{
        const result=runCargo(root,args,{sourceRoot,owner:OWNER,oneShot:true,environment});
        handlesReaped=childReaped(root,result.buildEvidence);
        assert(handlesReaped,'oracle.cargo_reap_unverified');
        report.invocations.push({role,state:'succeeded',buildEvidence:result.buildEvidence,inputHashes:result.inputHashes});
        report.cargoInvoked=true;save(reportPath,report);
        return true;
      }catch(error){
        if(!error.buildEvidence)throw error;
        handlesReaped=childReaped(root,error.buildEvidence);
        report.invocations.push({role,state:'failed',error:error.message,buildEvidence:error.buildEvidence});
        report.cargoInvoked=true;save(reportPath,report);
        if(!handlesReaped)throw error;
        return false;
      }
    };
    const referenceSucceeded=invoke(stage,{LYCHEEDEV_EMMY_ORACLE_CAPTURE:referenceDir},'historical-reference');
    report.reference.cargoSucceeded=referenceSucceeded;
    // A nonzero historical run stays failed. If all independently produced
    // traces are nevertheless complete, run clean as useful additional evidence.
    // Missing or failed-test traces still reject comparison below.
    const reference=readTraces(referenceDir,inputs.manifest.targets,'reference');
    report.reference.traces=reference.map(({rows,path,...row})=>({...row,rows:rows.length,path}));
    report.reference.totalRows=reference.reduce((n,r)=>n+r.rows.length,0);
    save(reportPath,report);stable();
    assert.equal(sourceDigest(stage),report.reference.sourceTree,'oracle.reference_changed');
    const candidateSucceeded=invoke(root,{LYCHEEDEV_EMMY_ORACLE_CAPTURE:candidateDir,LYCHEEDEV_EMMY_ORACLE_REFERENCE:referenceDir},'clean-candidate');
    report.production.cargoSucceeded=candidateSucceeded;
    const candidate=readTraces(candidateDir,inputs.manifest.targets,'candidate');
    for(let i=0;i<reference.length;i++){
      assert.equal(hashFile(reference[i].path,MAX_TRACE),reference[i].sha256,'oracle.reference_trace_changed');
    }
    stable();
    assert.equal(inspectEmmy(root).patchedTreeSHA256,CLEAN,'oracle.production_tree_changed');
    assert.equal(inspectEmmy(stage).patchedTreeSHA256,HISTORY,'oracle.reference_tree_changed');
    report.production.traces=candidate.map(({rows,path,...row})=>({...row,rows:rows.length,path}));
    report.production.totalRows=candidate.reduce((n,r)=>n+r.rows.length,0);
    report.differences=traceDifferences(reference,candidate);
    report.allTraceFieldsMatch=report.differences.length===0;
    save(reportPath,report);
    assert(report.allTraceFieldsMatch,'oracle.all_fields_mismatch');
    assert(referenceSucceeded&&candidateSucceeded,'oracle.cargo_side_failed');
    report.state='passed';succeeded=true;
    return report;
  }catch(error){report.state='failed';report.error=error.stack||error.message;throw error;}
  finally{
    // Preserve partial failed traces as evidence as well; never reuse them as expected.
    for(const directory of [referenceDir,candidateDir])for(const name of readdirSync(directory)){
      const path=join(directory,name),info=lstatSync(path);
      if(info.isFile()&&!info.isSymbolicLink()&&info.size<=MAX_TRACE)logs.push(path);
      else {report.state='failed';report.evidenceError='oracle.trace_budget';handlesReaped=false;}
    }
    save(reportPath,report);
    job.seal({state:handlesReaped?(succeeded?'succeeded':'failed'):'interrupted',handlesReaped,logs});
    if(handlesReaped)job.cleanup();
  }
}
if(process.argv[1]&&resolve(process.argv[1])===resolve(import.meta.filename)){
  console.log(JSON.stringify(run(),null,2));
}
