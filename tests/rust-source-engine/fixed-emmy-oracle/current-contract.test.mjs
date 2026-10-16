import test from 'node:test';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {cpSync,mkdtempSync,mkdirSync,readFileSync,rmSync,symlinkSync,writeFileSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {dirname,join,resolve} from 'node:path';
import {spawnSync} from 'node:child_process';
import {evidenceRelative,expectedCoverageRows,readSealedOwner,runCurrentContract,verifyCurrentRows} from './current-contract.mjs';
import {traceDifferences} from './run.mjs';
import * as strict from './run.mjs';
import * as readOnly from './current-contract-traces.mjs';

const HERE=dirname(import.meta.filename),ROOT=resolve(HERE,'../../..');
const pins=JSON.parse(readFileSync(join(HERE,'current-contract.json')));
const targets=JSON.parse(readFileSync(join(HERE,'inputs.json'))).targets;
const target='object_carrier_native_oracle';
const name='actual_unary_tuple_logical_declaration_and_observed_return_transports_keep_nested_carriers';
const file=`${target}--${name}.jsonl`;
const sha=data=>createHash('sha256').update(data).digest('hex');
const clone=value=>structuredClone(value);

// Deliberately synthetic parser/contract fixtures. They are NEVER sealed as, or
// passed to the executable as, the ac8 runtime evidence.
function fixture(){
  const reference=targets.flatMap(({target,tests})=>tests.map(test=>({target,test,file:`${target}--${test}.jsonl`,
    rows:Array.from({length:target==='object_carrier_native_oracle'&&test===name?24:1},(_,ordinal)=>
      ({kind:'row',ordinal,label:'synthetic.same',value:'synthetic unit-test value'}))})));
  const required=4137-reference.reduce((n,t)=>n+t.rows.length,0);
  for(let i=0;i<required;i++)reference[0].rows.push({kind:'row',ordinal:reference[0].rows.length,label:'synthetic.same',value:'unit-test-only'});
  const candidate=clone(reference),left=reference.find(trace=>trace.file===file),right=candidate.find(trace=>trace.file===file);
  for(const ordinal of [18,22]){
    const row=expectedCoverageRows(ordinal);left.rows[ordinal]=row.reference;right.rows[ordinal]=row.candidate;
  }
  return {reference,candidate,left,right,verify:()=>verifyCurrentRows(reference,candidate,targets)};
}

test('the exact two full current receipt rows pass separately while historical raw stays false',()=>{
  const f=fixture(),before=clone(f.reference),actualBefore=clone(f.candidate),result=f.verify();
  assert.equal(result.state,'passed');assert.equal(result.totalRows,4137);assert.equal(result.unchangedRows,4135);
  assert.deepEqual(result.exactChangedRows.map(row=>row.reference.ordinal),[18,22]);
  const differences=traceDifferences(f.reference,f.candidate);
  assert.equal(differences.length,1);assert.equal(differences[0].differingRows,2);
  assert.deepEqual(differences[0].first.map(row=>row.ordinal),[18,22]);
  assert.notEqual(differences.length,0,'the historical raw allTraceFieldsMatch expression remains false');
  assert.deepEqual(f.reference,before);assert.deepEqual(f.candidate,actualBefore);
});

test('read-only parser functions are identical to the unchanged historical runner without process-tool imports',()=>{
  for(const name of ['validateFrames','readTraces','assertTracesUnchanged','oracleCargoArgs','traceDifferences'])
    assert.equal(readOnly[name].toString(),strict[name].toString(),name);
  const source=readFileSync(join(HERE,'current-contract-traces.mjs'),'utf8');
  assert(!/from ['"](?!node:)/.test(source),'reader imports only Node builtins');
  const entry=readFileSync(join(HERE,'current-contract.mjs'),'utf8');
  assert(!/from ['"][^'"]*(?:tools\/|\/run\.mjs|\.\/run\.mjs)/.test(entry));
});

test('identical readers parse the same full values and reject the same malformed multipart input',t=>{
  const root=mkdtempSync(join(tmpdir(),'emmy-readonly-trace-test-'));t.after(()=>rmSync(root,{recursive:true,force:true}));
  const filename='case--sample.jsonl',inventory=[{target:'case',tests:['sample']}];
  const header={kind:'header',schema:'lycheedev.emmy-fixed-oracle.v1',role:'reference',target:'case',test:'sample'};
  const row={kind:'row',ordinal:0,label:'full.value',value:expectedCoverageRows(18).reference.value};
  const terminal={kind:'end',rows:1,complete:true};
  const write=(name,frames)=>writeFileSync(join(root,name),frames.map(frame=>JSON.stringify(frame)).join('\n')+'\n');
  write(filename,[header,row]);write(filename+'.part1',[terminal]);
  assert.deepEqual(readOnly.readTraces(root,inventory,'reference'),strict.readTraces(root,inventory,'reference'));
  const traces=readOnly.readTraces(root,inventory,'reference');readOnly.assertTracesUnchanged(root,traces);
  for(const bad of [{...terminal,complete:false},{...terminal,rows:2},{...row,ordinal:2}]){
    write(filename+'.part1',[bad]);
    for(const reader of [strict,readOnly])assert.throws(()=>reader.readTraces(root,inventory,'reference'));
  }
  write(filename+'.part1',[terminal]);write(filename+'.part3',[terminal]);
  for(const reader of [strict,readOnly])assert.throws(()=>reader.readTraces(root,inventory,'reference'),/test_inventory/);
});

test('the four historical entries and the fifth current entry include every site and optional field',()=>{
  for(const [ordinal,range] of [[18,'30..33'],[22,'42..45']]){
    const row=expectedCoverageRows(ordinal);
    assert.equal((row.reference.value.match(/BindingCoverage \{/g)||[]).length,4);
    assert.equal((row.candidate.value.match(/BindingCoverage \{/g)||[]).length,5);
    assert.equal((row.candidate.value.match(new RegExp(`range: ${range.replaceAll('.','\\.')}`, 'g'))||[]).length,5);
    assert.equal((row.candidate.value.match(/reason: None, parameter: None, parameter_index: None, argument: None/g)||[]).length,5);
    assert(row.candidate.value.endsWith('operation: "call_result", state: "native_projected", proof: "permit", reason: None, parameter: None, parameter_index: None, argument: None }]'));
  }
  assert.throws(()=>expectedCoverageRows(19),/unsupported_ordinal/);
});

for(const ordinal of [18,22]){
  for(const [description,mutate] of [
    ['missing receipt',(row,old)=>({...row,value:old.value})],
    ['duplicated receipt',row=>({...row,value:row.value.slice(0,-1)+', '+row.value.slice(row.value.lastIndexOf('BindingCoverage {'),-1)+']'})],
    ['additional coverage entry',row=>({...row,value:row.value.slice(0,-1)+', BindingCoverage { extra: true }] '})],
    ['wrong file',row=>({...row,value:row.value.replaceAll('FileId { id: 0 }','FileId { id: 1 }')})],
    ['wrong syntax kind',row=>({...row,value:row.value.replaceAll('Syntax(CallExpr)','Syntax(NameExpr)')})],
    ['wrong site',row=>({...row,value:row.value.replaceAll(ordinal===18?'30..33':'42..45','31..34')})],
    ['wrong receipt proof',row=>({...row,value:row.value.replace('state: "native_projected", proof: "permit"','state: "native_projected", proof: "other"')})],
    ['wrong completion state',row=>({...row,value:row.value.replace('native_projected','unavailable')})],
    ['wrong operation',row=>({...row,value:row.value.replace('"call_result"','"return_check"')})],
    ['present reason',row=>({...row,value:row.value.replaceAll('reason: None','reason: Some("not-observed")')})],
    ['present parameter',row=>({...row,value:row.value.replaceAll('parameter: None','parameter: Some("x")')})],
    ['present parameter index',row=>({...row,value:row.value.replaceAll('parameter_index: None','parameter_index: Some(0)')})],
    ['present argument',row=>({...row,value:row.value.replaceAll('argument: None','argument: Some(other)')})],
    ['altered historical receipt',row=>({...row,value:row.value.replace('nil-slot','new-nil-slot')})],
    ['changed row label',row=>({...row,label:'object.query.type'})],
    ['changed row ordinal',row=>({...row,ordinal:ordinal+1})],
    ['extra row field',row=>({...row,allowed:true})],
    ['format normalization',row=>({...row,value:row.value.replaceAll(', ', ',')})],
  ])test(`ordinal ${ordinal} rejects ${description}`,()=>{
    const f=fixture();f.right.rows[ordinal]=mutate(f.right.rows[ordinal],f.left.rows[ordinal]);assert.throws(f.verify);
  });
}

test('reordered old receipts or an early current receipt fail even with the same fields',()=>{
  for(const ordinal of [18,22]){
    const f=fixture();
    const entries=f.right.rows[ordinal].value.slice(1,-1).split(/, (?=BindingCoverage \{)/);
    [entries[0],entries[3]]=[entries[3],entries[0]];
    f.right.rows[ordinal].value=`[${entries.join(', ')}]`;
    assert.throws(f.verify);
    const g=fixture(),row=g.right.rows[ordinal],start=row.value.lastIndexOf('BindingCoverage {');
    row.value='['+row.value.slice(start,-1)+', '+row.value.slice(1,start-2)+']';assert.throws(g.verify);
  }
});

test('historical rows cannot be changed on both sides or be backfilled with the current receipt',()=>{
  for(const ordinal of [18,22]){
    const f=fixture();f.left.rows[ordinal].value=f.right.rows[ordinal].value;assert.throws(f.verify,/reference_coverage/);
    const g=fixture();g.left.rows[ordinal].value=g.left.rows[ordinal].value.replace('nil-slot','changed');
    g.right.rows[ordinal].value=g.right.rows[ordinal].value.replace('nil-slot','changed');assert.throws(g.verify,/reference_coverage/);
  }
});

test('a provider/type/other coverage difference anywhere else always fails',()=>{
  for(const label of ['object.query.provider','object.query.type','call.coverage']){
    const f=fixture();f.reference[0].rows[0].label=label;f.candidate[0].rows[0].label=label;
    f.candidate[0].rows[0].value='changed';assert.throws(f.verify,/unexpected_difference/);
  }
  const f=fixture();f.right.rows[19].value='extra provider observation';assert.throws(f.verify,/unexpected_difference/);
});

test('same two changes at another test or ordinal cannot become the allowed pair',()=>{
  const f=fixture();[f.right.rows[18],f.right.rows[22]]=[f.right.rows[22],f.right.rows[18]];assert.throws(f.verify);
  const g=fixture();g.candidate.find(t=>t.file!==file).rows[0].value=g.right.rows[18].value;assert.throws(g.verify);
});

test('missing additional duplicate reordered tests and rows fail',()=>{
  for(const mutate of [f=>f.candidate.pop(),f=>f.candidate.push(clone(f.candidate[0])),
    f=>{f.candidate[1]=clone(f.candidate[0]);},f=>f.candidate.reverse(),
    f=>f.right.rows.pop(),f=>f.right.rows.push({...f.right.rows[0],ordinal:24}),
    f=>{f.reference[0].rows.pop();f.candidate[0].rows.pop();}]){
    const f=fixture();mutate(f);assert.throws(f.verify);
  }
});

function ownerFixture(t){
  const root=mkdtempSync(join(tmpdir(),'emmy-current-contract-test-'));t.after(()=>rmSync(root,{recursive:true,force:true}));
  const id='unit-test-owner',directory=join(root,id);mkdirSync(directory);
  const log='unit-test-only.json',original=`C:\\repo\\.tmp\\rust-disk\\evidence\\${id}\\${log}`;
  writeFileSync(join(directory,log),'synthetic evidence\n');
  const command=['unit-test-only'];
  const receipt={schema:'lycheedev.disk-owner.v1',id,owner:'ci-emmy-selected-oracle',category:'scratch',state:'retired',
    activePid:null,handlesReaped:true,sealedAt:'2026-10-16T00:00:00Z',command,commandSHA256:sha(JSON.stringify(command)),
    sealedLogs:[{original,path:original,sha256:sha(readFileSync(join(directory,log)))}]};
  const save=()=>{writeFileSync(join(directory,'receipt.json'),JSON.stringify(receipt));return sha(readFileSync(join(directory,'receipt.json')));};
  return {root,id,directory,log,receipt,save,read:()=>readSealedOwner(root,id,save())};
}

test('sealed owner consumes the full exact log hash from a relocated intact owner directory',t=>{
  const f=ownerFixture(t),record=f.read();assert.equal(record.seals.size,1);assert.equal(record.receipt.id,f.id);
  assert.equal(evidenceRelative(`D:/original/.tmp/rust-disk/evidence/${f.id}/reference/trace.jsonl`,f.id),'reference/trace.jsonl');
});

test('a self-resealed mutation cannot replace the independently pinned owner receipt',t=>{
  const f=ownerFixture(t),pin=f.save();writeFileSync(join(f.directory,f.log),'changed evidence\n');
  f.receipt.sealedLogs[0].sha256=sha(readFileSync(join(f.directory,f.log)));f.save();
  assert.throws(()=>readSealedOwner(f.root,f.id,pin),/receipt_hash/);
});

test('unsealed changed missing duplicate foreign and nonterminal evidence fails',t=>{
  const f=ownerFixture(t),pin=f.save();writeFileSync(join(f.directory,f.log),'changed');
  assert.throws(()=>readSealedOwner(f.root,f.id,pin),/sealed_hash/);
  writeFileSync(join(f.directory,f.log),'synthetic evidence\n');
  for(const [key,value] of [['state','active'],['handlesReaped',false],['activePid',123],['owner','foreign']]){
    const old=f.receipt[key];f.receipt[key]=value;assert.throws(f.read,/owner_identity_or_terminal/);f.receipt[key]=old;
  }
  f.receipt.sealedLogs.push({...f.receipt.sealedLogs[0]});assert.throws(f.read,/sealed_inventory/);f.receipt.sealedLogs.pop();
  rmSync(join(f.directory,f.log));assert.throws(f.read);
});

test('evidence remapping rejects traversal foreign owners and arbitrary absolute paths',()=>{
  for(const path of ['C:/elsewhere/trace.jsonl','C:/repo/.tmp/rust-disk/evidence/foreign/file',
    'C:/repo/.tmp/rust-disk/evidence/owner/../file','C:/repo/.tmp/rust-disk/evidence/owner//file',
    'C:/repo/.tmp/rust-disk/evidence/owner/trace.','C:/repo/.tmp/rust-disk/evidence/owner/trace:stream'])
    assert.throws(()=>evidenceRelative(path,'owner'),/evidence_path/);
});

test('symlink evidence cannot bypass a seal or redirect reads',t=>{
  const f=ownerFixture(t),other=join(f.root,'other');writeFileSync(other,'synthetic evidence\n');rmSync(join(f.directory,f.log));
  try{symlinkSync(other,join(f.directory,f.log));}catch(error){if(error.code==='EPERM'){t.skip('platform does not permit symlink creation');return;}throw error;}
  assert.throws(f.read,/symlink/);
});

test('read-only entry rejects missing real evidence and records failure outside the original owner',t=>{
  const root=mkdtempSync(join(tmpdir(),'emmy-current-output-test-'));t.after(()=>rmSync(root,{recursive:true,force:true}));
  const evidence=join(root,'evidence');mkdirSync(evidence);const sentinel=join(evidence,'retained.json');writeFileSync(sentinel,'original');
  const output=join(root,'new-report');const result=runCurrentContract({evidenceRoot:evidence,output,contractRoot:ROOT});
  assert.equal(result.currentContract.state,'failed');assert.equal(result.historicalRaw.state,'not_verified');
  assert.equal(readFileSync(sentinel,'utf8'),'original');
  assert.throws(()=>runCurrentContract({evidenceRoot:evidence,output}),/fresh_separate_output_required/);
  assert.throws(()=>runCurrentContract({evidenceRoot:evidence,output:join(evidence,'new')}),/fresh_separate_output_required/);
});

test('CLI has no accept-update ignore-coverage or source-identity override',()=>{
  const result=spawnSync(process.execPath,[join(HERE,'current-contract.mjs'),'--receipt-sha256','0'.repeat(64),'--output','ignored'],{encoding:'utf8'});
  assert.notEqual(result.status,0);assert.match(result.stderr,/current_contract.usage/);
});

// Exercise the complete executable with a private COPY of its manifest and
// explicitly synthetic artifacts. No production test-hook or pin override is
// added; the real executable rejects these different owner/receipt identities.
function completeSyntheticFixture(t){
  const root=mkdtempSync(join(tmpdir(),'emmy-synthetic-custody-test-'));t.after(()=>rmSync(root,{recursive:true,force:true}));
  const checker=join(root,'checker');cpSync(HERE,checker,{recursive:true});
  const evidence=join(root,'.tmp/rust-disk/evidence');mkdirSync(evidence,{recursive:true});
  const fake={...clone(pins),measuredCommit:'SYNTHETIC-UNIT-TEST-NOT-RUNTIME',measuredSourceTree:'a'.repeat(64),
    historicalSourceTree:'b'.repeat(64),oracleOwner:'synthetic-main',referenceCargoOwner:'synthetic-reference',candidateCargoOwner:'synthetic-candidate'};
  for(const id of [fake.oracleOwner,fake.referenceCargoOwner,fake.candidateCargoOwner])mkdirSync(join(evidence,id));
  const main=join(evidence,fake.oracleOwner),f=fixture(),traces={};
  for(const role of ['reference','candidate']){
    const directory=join(main,role);mkdirSync(directory);
    for(const [index,trace] of f[role].entries()){
      const frames=[{kind:'header',schema:'lycheedev.emmy-fixed-oracle.v1',role,target:trace.target,test:trace.test},
        ...trace.rows,{kind:'end',rows:trace.rows.length,complete:true}];
      const chunks=index<2?[[frames[0]],frames.slice(1)]:[frames];
      chunks.forEach((chunk,part)=>writeFileSync(join(directory,trace.file+(part?`.part${part}`:'')),chunk.map(row=>JSON.stringify(row)).join('\n')+'\n'));
    }
    traces[role]=readOnly.readTraces(directory,targets,role);
  }
  const write=(path,value)=>writeFileSync(path,JSON.stringify(value,null,2)+'\n');
  const seal=(id,command,inputHashes,paths)=>{
    const receipt={schema:'lycheedev.disk-owner.v1',id,owner:'ci-emmy-selected-oracle',category:'scratch',
      state:'retired',handlesReaped:true,activePid:null,sealedAt:'2026-10-16T00:00:00Z',command,inputHashes,
      commandSHA256:sha(JSON.stringify(command)),sealedLogs:paths.map(path=>({original:path,path,sha256:sha(readFileSync(path))}))};
    const path=join(evidence,id,'receipt.json');write(path,receipt);return sha(readFileSync(path));
  };
  const args=readOnly.oracleCargoArgs(targets),childInputs=[
    {sourceTree:fake.historicalSourceTree,emmyPatch:fake.historicalTree},
    {sourceTree:fake.measuredSourceTree,emmyPatch:fake.cleanTree}];
  for(const [index,id] of [fake.referenceCargoOwner,fake.candidateCargoOwner].entries()){
    const path=join(evidence,id,'command.json');
    write(path,{command:'cargo',args,inputHashes:childInputs[index],exit:index?101:0,error:index?'owned.forced_tree_termination':null});
    fake[index?'candidateCargoReceiptSHA256':'referenceCargoReceiptSHA256']=seal(id,['cargo',...args],childInputs[index],[path]);
  }
  const traceMetadata=role=>traces[role].map(({rows,...trace})=>({...trace,rows:rows.length}));
  const raw={schema:'lycheedev.emmy-fixed-run.v1',state:'failed',referenceCommit:JSON.parse(readFileSync(join(HERE,'inputs.json'))).referenceCommit,
    fixedInputsSHA256:sha(readFileSync(join(HERE,'inputs.json'))),sourceTree:fake.measuredSourceTree,tests:36,cargoInvoked:true,
    production:{identity:'public-upstream-unmodified-v1',treeSHA256:fake.cleanTree,files:848,bytes:8382174,cargoSucceeded:false,
      traces:traceMetadata('candidate'),totalRows:4137},
    reference:{treeSHA256:fake.historicalTree,sourceTree:fake.historicalSourceTree,sourceIdentity:'fixed-source-plus-compile-compatibility',
      cargoSucceeded:true,traces:traceMetadata('reference'),totalRows:4137},
    invocations:[{role:'historical-reference',state:'succeeded',buildEvidence:join(evidence,fake.referenceCargoOwner,'receipt.json'),inputHashes:childInputs[0]},
      {role:'clean-candidate',state:'failed',buildEvidence:join(evidence,fake.candidateCargoOwner,'receipt.json'),error:'build.cargo_failed:owned.forced_tree_termination'}],
    differences:readOnly.traceDifferences(traces.reference,traces.candidate),allTraceFieldsMatch:false,error:'SYNTHETIC oracle.all_fields_mismatch'};
  const save=()=>{
    const reportPath=join(main,'fixed-oracle-report.json');write(reportPath,raw);fake.reportSHA256=sha(readFileSync(reportPath));
    const paths=[reportPath,...Object.values(traces).flatMap(side=>side.flatMap(trace=>trace.parts.map(part=>part.path)))];
    fake.receiptSHA256=seal(fake.oracleOwner,['node','tests/rust-source-engine/fixed-emmy-oracle/run.mjs'],
      {sourceTree:fake.measuredSourceTree,fixedInputs:raw.fixedInputsSHA256,cleanTree:fake.cleanTree,referenceTree:fake.historicalTree},paths);
    write(join(checker,'current-contract.json'),fake);
  };
  save();let serial=0;
  const run=()=>{
    const output=join(root,`result-${serial++}`);
    const child=spawnSync(process.execPath,[join(checker,'current-contract.mjs'),'--evidence-root',evidence,'--output',output,'--contract-root',ROOT],{encoding:'utf8'});
    return {child,report:JSON.parse(readFileSync(join(output,'current-contract-report.json')))};
  };
  return {root,evidence,main,fake,raw,traces,save,run};
}

test('complete synthetic three-owner executable preserves raw false and exit101 beside the narrow pass',t=>{
  const f=completeSyntheticFixture(t),before=readFileSync(join(f.main,'fixed-oracle-report.json'));
  const {child,report}=f.run();assert.equal(child.status,0,child.stderr||JSON.stringify(report));
  assert.equal(report.currentContract.state,'passed');assert.equal(report.measuredCommit,'SYNTHETIC-UNIT-TEST-NOT-RUNTIME');
  assert.equal(report.historicalRaw.state,'failed');assert.equal(report.historicalRaw.allTraceFieldsMatch,false);
  assert.deepEqual(report.historicalRaw.invocations.map(row=>row.exit),[0,101]);
  assert.equal(report.historicalRaw.invocations[1].error,'owned.forced_tree_termination');
  assert.deepEqual(readFileSync(join(f.main,'fixed-oracle-report.json')),before);
});

test('authenticated historical failed status survives a later missing-child failure',t=>{
  const f=completeSyntheticFixture(t);rmSync(join(f.evidence,f.fake.candidateCargoOwner,'command.json'));
  const {child,report}=f.run();assert.equal(child.status,1);
  assert.equal(report.currentContract.state,'failed');assert.equal(report.historicalRaw.state,'failed');
  assert.equal(report.historicalRaw.allTraceFieldsMatch,false);
});

test('authenticated full report rejects trace metadata drift and extra physical trace parts',t=>{
  const f=completeSyntheticFixture(t);f.raw.production.traces[0].rows++;f.save();
  let actual=f.run();assert.equal(actual.child.status,1);assert.match(actual.report.currentContract.error,/reported_trace_metadata/);
  assert.equal(actual.report.historicalRaw.allTraceFieldsMatch,false);
  f.raw.production.traces[0].rows--;f.save();
  writeFileSync(join(f.main,'candidate','unexpected.jsonl'),'{}\n');
  actual=f.run();assert.equal(actual.child.status,1);assert.match(actual.report.currentContract.error,/test_inventory/);
  assert.equal(actual.report.historicalRaw.state,'failed');
});

test('reviewed current contract source and unchanged historical runner remain exact ac8 bytes',()=>{
  assert.equal(pins.measuredCommit,'ac8bfbe656e81501e64ec572fc2c5c2be909a758');
  assert.equal(pins.measuredSourceTree,'73ebb0f797b20187dd8c135812e3d64c26a94410e39b3a4828014f31adf7b5b4');
  for(const key of ['receiptSHA256','reportSHA256','referenceCargoReceiptSHA256','candidateCargoReceiptSHA256'])
    assert.match(pins[key],/^[a-f0-9]{64}$/,key);
  for(const file of pins.sourceFiles){
    const bytes=readFileSync(join(ROOT,file.path));assert.equal(bytes.length,file.bytes,file.path);assert.equal(sha(bytes),file.sha256,file.path);
  }
  const entry=readFileSync(join(HERE,'current-contract.mjs'),'utf8');
  assert(!/runCargo\(|ownedSpawnSync\(|beginScratch\(|sourceDigest\(|inspectEmmy\(/.test(entry));
  assert(!/writeFileSync\([^\n]*(?:receipt\.json|fixed-oracle-report\.json)/.test(entry));
});
