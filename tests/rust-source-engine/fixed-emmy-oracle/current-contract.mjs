// A parallel, read-only interpretation of ONE measured oracle run. The original
// runner, assertions, raw rows and failed Cargo/strict verdict are never changed.
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {existsSync,lstatSync,mkdirSync,readFileSync,realpathSync,writeFileSync} from 'node:fs';
import {basename,dirname,join,resolve,sep} from 'node:path';
import {isDeepStrictEqual} from 'node:util';
import {assertTracesUnchanged,oracleCargoArgs,readTraces,traceDifferences} from './current-contract-traces.mjs';

const HERE=dirname(import.meta.filename),ROOT=resolve(HERE,'../../..');
const MAX_FILE=16<<20;
const sha256=data=>createHash('sha256').update(data).digest('hex');
const hex=value=>typeof value==='string'&&/^[a-f0-9]{64}$/.test(value);
const fail=(condition,reason)=>assert(condition,`current_contract.${reason}`);
const TARGET='object_carrier_native_oracle';
const TEST='actual_unary_tuple_logical_declaration_and_observed_return_transports_keep_nested_carriers';
const FILE=`${TARGET}--${TEST}.jsonl`;

function noLinks(path){
  path=resolve(path);
  for(let current=path;;current=dirname(current)){
    fail(!lstatSync(current).isSymbolicLink(),'symlink');
    if(dirname(current)===current)break;
  }
  return realpathSync(path);
}
function readBounded(path,max=MAX_FILE){
  noLinks(path);
  const info=lstatSync(path);
  fail(info.isFile()&&info.size<=max,'file_budget');
  const data=readFileSync(path);
  fail(data.length===info.size,'file_changed');
  return data;
}
const readJSON=path=>JSON.parse(new TextDecoder('utf-8',{fatal:true}).decode(readBounded(path)));
const hashFile=path=>sha256(readBounded(path));
const inside=(parent,path)=>path===parent||path.startsWith(parent+sep);

// Relocation is limited to an intact evidence/<original-owner>/... tree. Never
// follow an absolute path embedded in a report or search for a matching basename.
export function evidenceRelative(original,owner){
  fail(typeof original==='string','evidence_path');
  const value=original.replaceAll('\\','/'),anchor=`/.tmp/rust-disk/evidence/${owner}/`;
  const at=value.indexOf(anchor);
  fail(at>=0&&at===value.lastIndexOf(anchor),'evidence_path');
  const tail=value.slice(at+anchor.length);
  fail(tail.length>0&&tail.split('/').every(part=>part&&part!=='.'&&part!=='..'&&
    !/[\\:<>"|?*\x00-\x1f]/.test(part)&&!/[. ]$/.test(part)),'evidence_path');
  return tail;
}

// Unit-testable custody check. The executable always supplies the immutable
// measured owner/hash from current-contract.json; there is no CLI hash override.
export function readSealedOwner(evidenceRoot,id,expectedSHA256){
  fail(hex(expectedSHA256),'missing_measured_receipt_pin');
  const root=join(evidenceRoot,id),path=join(root,'receipt.json');
  fail(hashFile(path)===expectedSHA256,'receipt_hash');
  const receipt=readJSON(path);
  fail(receipt.schema==='lycheedev.disk-owner.v1'&&receipt.id===id&&
    receipt.owner==='ci-emmy-selected-oracle'&&receipt.category==='scratch'&&
    receipt.state==='retired'&&receipt.handlesReaped===true&&receipt.activePid===null&&
    typeof receipt.sealedAt==='string','owner_identity_or_terminal');
  fail(Array.isArray(receipt.command)&&receipt.commandSHA256===sha256(JSON.stringify(receipt.command)),
    'owner_command');
  fail(Array.isArray(receipt.sealedLogs)&&receipt.sealedLogs.length>0&&
    receipt.sealedLogs.length<=256,'sealed_inventory');
  const seals=new Map();
  for(const log of receipt.sealedLogs){
    const name=evidenceRelative(log.path,id);
    fail(!seals.has(name)&&hex(log.sha256),'sealed_inventory');
    fail(hashFile(join(root,name))===log.sha256,'sealed_hash');
    seals.set(name,log.sha256);
  }
  // The receipt is an external trust root; rechecking it prevents a later edit
  // from being mistaken for the same preserved owner.
  fail(hashFile(path)===expectedSHA256,'receipt_changed');
  return {root,path,receipt,seals,sha256:expectedSHA256};
}

function coverageEntry(range,operation,state,proof){
  return `BindingCoverage { site: BindingSite { file_id: FileId { id: 0 }, syntax: LuaSyntaxId { kind: Syntax(CallExpr), range: ${range} } }, operation: "${operation}", state: "${state}", proof: "${proof}", reason: None, parameter: None, parameter_index: None, argument: None }`;
}
// Exact full Debug values, not a parser, normalization, suffix match, count-only
// allowance, or general coverage exception. These are the original two fixtures.
export function expectedCoverageRows(ordinal){
  fail(ordinal===18||ordinal===22,'unsupported_ordinal');
  const range=ordinal===18?'30..33':'42..45';
  const old=[['call_return','known_absent','nil-slot'],['object_shape','bound','shape:object-1'],
    ['call_return','bound','selected:object-1'],['call','bound','permit']]
    .map(args=>coverageEntry(range,...args));
  const row=values=>({kind:'row',ordinal,label:'object.query.coverage',value:`[${values.join(', ')}]`});
  return {reference:row(old),candidate:row([...old,coverageEntry(range,'call_result','native_projected','permit')])};
}

export function verifyCurrentRows(reference,candidate,targets){
  const expected=targets.flatMap(({target,tests})=>tests.map(test=>({target,test,file:`${target}--${test}.jsonl`})));
  fail(expected.length===36&&new Set(expected.map(row=>row.file)).size===36,'test_inventory');
  fail(reference.length===36&&candidate.length===36,'test_inventory');
  const exactChangedRows=[];let unchangedRows=0,totalRows=0;
  for(const [index,identity] of expected.entries()){
    const left=reference[index],right=candidate[index];
    for(const trace of [left,right]){
      for(const key of ['file','target','test'])fail(trace[key]===identity[key],'trace_identity');
      fail(Array.isArray(trace.rows)&&trace.rows.length>0,'rows');
    }
    fail(left.rows.length===right.rows.length,'row_count');
    if(identity.file===FILE)fail(left.rows.length===24,'object_trace_rows');
    for(const [ordinal,old] of left.rows.entries()){
      const actual=right.rows[ordinal];totalRows++;
      for(const row of [old,actual])fail(isDeepStrictEqual(Object.keys(row).sort(),['kind','label','ordinal','value'])&&
        row.kind==='row'&&row.ordinal===ordinal&&typeof row.label==='string'&&typeof row.value==='string','row_schema');
      if(identity.file===FILE&&(ordinal===18||ordinal===22)){
        const expectedRows=expectedCoverageRows(ordinal);
        fail(isDeepStrictEqual(old,expectedRows.reference),`reference_coverage_${ordinal}`);
        fail(isDeepStrictEqual(actual,expectedRows.candidate),`candidate_coverage_${ordinal}`);
        exactChangedRows.push({file:FILE,...expectedRows});
      }else{
        fail(isDeepStrictEqual(old,actual),`unexpected_difference:${identity.file}:${ordinal}`);
        unchangedRows++;
      }
    }
  }
  fail(totalRows===4137&&unchangedRows===4135&&exactChangedRows.length===2,'complete_measured_inventory');
  return {state:'passed',totalRows,unchangedRows,exactChangedRows,
    meaning:'Only the two fully specified completed object-call projection receipts differ. Historical raw comparison and candidate Cargo remain failed.'};
}

function assertTraceReport(traces,record,owner,role,seals){
  fail(record.totalRows===4137&&record.traces?.length===36,'reported_trace_inventory');
  fail(traces.reduce((n,trace)=>n+trace.parts.length,0)===38,'physical_part_inventory');
  for(const [index,trace] of traces.entries()){
    const recorded=record.traces[index];
    const {path,parts,rows,...rest}=trace;
    const canonical={...rest,rows:rows.length,path:`${role}/${trace.file}`,
      parts:parts.map(part=>({...part,path:`${role}/${part.file}`}))};
    const observed={...recorded,path:evidenceRelative(recorded.path,owner),
      parts:recorded.parts?.map(part=>({...part,path:evidenceRelative(part.path,owner)}))};
    fail(isDeepStrictEqual(observed,canonical),'reported_trace_metadata');
    for(const part of parts)fail(seals.get(`${role}/${part.file}`)===part.sha256,'unsealed_trace');
  }
}

export function inspectCurrentContract(evidenceRoot,contractRoot=ROOT){
  evidenceRoot=noLinks(evidenceRoot);contractRoot=noLinks(contractRoot);
  const pin=readJSON(join(HERE,'current-contract.json'));
  fail(pin.schema==='lycheedev.emmy-current-object-contract.v1'&&pin.identity==='ac8-observed-object-call-result-r1','identity');
  for(const file of pin.sourceFiles){
    const path=join(contractRoot,file.path),data=readBounded(path);
    fail(data.length===file.bytes&&sha256(data)===file.sha256,`contract_source_changed:${file.path}`);
  }
  // --contract-root may point at the preserved ac8 tree. Authenticate the
  // original runner beside THIS checker as well as the reviewed source root.
  const runnerPin=pin.sourceFiles.find(file=>file.path==='tests/rust-source-engine/fixed-emmy-oracle/run.mjs');
  fail(runnerPin&&hashFile(join(HERE,'run.mjs'))===runnerPin.sha256,'checker_runner_changed');
  const inputs=readJSON(join(HERE,'inputs.json'));
  const inputsSHA256=hashFile(join(HERE,'inputs.json'));
  // Authenticate the untouched 32-file historical harness as well as its manifest.
  fail(inputs.files.length===32,'harness_inventory');
  for(const file of inputs.files){
    const path=resolve(HERE,file.path);
    fail(inside(HERE,path),'harness_path');
    const data=readBounded(path);
    fail(data.length===file.bytes&&sha256(data)===file.sha256,'harness_hash');
  }
  const main=readSealedOwner(evidenceRoot,pin.oracleOwner,pin.receiptSHA256);
  fail(main.seals.get('fixed-oracle-report.json')===pin.reportSHA256,'report_seal');
  const raw=readJSON(join(main.root,'fixed-oracle-report.json'));
  fail(raw.schema==='lycheedev.emmy-fixed-run.v1'&&raw.state==='failed'&&raw.allTraceFieldsMatch===false&&
    typeof raw.error==='string'&&raw.error.includes('oracle.all_fields_mismatch')&&
    !raw.evidenceError&&!raw.invalidEvidence,'historical_raw_verdict');
  fail(raw.sourceTree===pin.measuredSourceTree&&raw.fixedInputsSHA256===inputsSHA256&&
    raw.referenceCommit===inputs.referenceCommit&&raw.reference.sourceTree===pin.historicalSourceTree&&
    raw.tests===36&&raw.cargoInvoked===true,'measured_identity');
  assert.deepEqual(main.receipt.inputHashes,{sourceTree:pin.measuredSourceTree,fixedInputs:inputsSHA256,
    cleanTree:pin.cleanTree,referenceTree:pin.historicalTree},'current_contract.owner_inputs');
  fail(raw.production.identity==='public-upstream-unmodified-v1'&&raw.production.treeSHA256===pin.cleanTree&&
    raw.production.files===848&&raw.production.bytes===8382174&&raw.production.cargoSucceeded===false&&
    raw.reference.treeSHA256===pin.historicalTree&&raw.reference.cargoSucceeded===true&&
    raw.reference.sourceIdentity==='fixed-source-plus-compile-compatibility','engine_or_cargo_identity');
  const historicalRaw={state:raw.state,allTraceFieldsMatch:raw.allTraceFieldsMatch,error:raw.error,
    referenceCargoSucceeded:raw.reference.cargoSucceeded,candidateCargoSucceeded:raw.production.cargoSucceeded,
    differences:raw.differences,invocations:raw.invocations};
  // Once the pinned original report is authenticated, a later contract/custody
  // failure must still retain its FAILED raw verdict in the parallel report.
  try{
    const historical=readSealedOwner(evidenceRoot,pin.referenceCargoOwner,pin.referenceCargoReceiptSHA256);
    const current=readSealedOwner(evidenceRoot,pin.candidateCargoOwner,pin.candidateCargoReceiptSHA256);
    fail(raw.invocations?.length===2,'invocation_inventory');
    const summaries=[];
    for(const [index,child] of [historical,current].entries()){
      const role=index===0?'historical-reference':'clean-candidate',invocation=raw.invocations[index];
      fail(invocation.role===role&&invocation.state===(index===0?'succeeded':'failed')&&
        evidenceRelative(invocation.buildEvidence,child.receipt.id)==='receipt.json','invocation_identity');
      assert.deepEqual(child.receipt.command,['cargo',...oracleCargoArgs(inputs.targets)],'current_contract.cargo_command');
      fail(child.receipt.commandSHA256===pin.cargoCommandSHA256,'cargo_command_identity');
      const tree=index===0?raw.reference.sourceTree:pin.measuredSourceTree;
      fail(hex(tree)&&child.receipt.inputHashes.sourceTree===tree&&
        child.receipt.inputHashes.emmyPatch===(index===0?pin.historicalTree:pin.cleanTree),'cargo_source_identity');
      fail(child.seals.has('command.json'),'unsealed_cargo_command');
      const command=readJSON(join(child.root,'command.json'));
      assert.deepEqual(command.inputHashes,child.receipt.inputHashes,'current_contract.cargo_inputs');
      if(index===0)assert.deepEqual(invocation.inputHashes,child.receipt.inputHashes,'current_contract.reference_invocation_inputs');
      else fail(!Object.hasOwn(invocation,'inputHashes'),'failed_invocation_identity_not_inferred');
      fail(command.command==='cargo','cargo_command');
      assert.deepEqual(command.args,child.receipt.command.slice(1),'current_contract.cargo_command_args');
      fail(command.exit===(index===0?0:101),'cargo_exit');
      summaries.push({role,owner:child.receipt.id,receiptSHA256:child.sha256,state:invocation.state,
        exit:command.exit,error:command.error??null,invocationError:invocation.error??null,
        handlesReaped:child.receipt.handlesReaped});
    }
    const referenceDir=join(main.root,'reference'),candidateDir=join(main.root,'candidate');
    const reference=readTraces(referenceDir,inputs.targets,'reference');
    const candidate=readTraces(candidateDir,inputs.targets,'candidate');
    assertTraceReport(reference,raw.reference,pin.oracleOwner,'reference',main.seals);
    assertTraceReport(candidate,raw.production,pin.oracleOwner,'candidate',main.seals);
    const differences=traceDifferences(reference,candidate);
    assert.deepEqual(differences,raw.differences,'current_contract.raw_difference_report');
    const currentContract=verifyCurrentRows(reference,candidate,inputs.targets);
    // No write has occurred. Recheck both full inventories and all source bytes.
    assertTracesUnchanged(referenceDir,reference);assertTracesUnchanged(candidateDir,candidate);
    for(const owner of [main,historical,current]){
      fail(hashFile(owner.path)===owner.sha256,'receipt_changed');
      for(const [name,digest] of owner.seals)fail(hashFile(join(owner.root,name))===digest,'sealed_file_changed');
    }
    fail(hashFile(join(main.root,'fixed-oracle-report.json'))===pin.reportSHA256,'report_changed');
    fail(hashFile(join(HERE,'run.mjs'))===runnerPin.sha256,'checker_runner_changed');
    for(const file of pin.sourceFiles)fail(hashFile(join(contractRoot,file.path))===file.sha256,'contract_source_changed');
    return {schema:'lycheedev.emmy-current-contract-report.v1',state:'current-contract-passed-historical-raw-failed',
      contract:pin.identity,measuredCommit:pin.measuredCommit,measuredSourceTree:raw.sourceTree,
      evidenceSource:pin.evidenceSource,custodyPinSource:pin.custodyPinSource,
      oracleOwner:pin.oracleOwner,receiptSHA256:main.sha256,reportSHA256:pin.reportSHA256,
      checker:{sha256:hashFile(import.meta.filename),manifestSHA256:hashFile(join(HERE,'current-contract.json')),
        traceReaderSHA256:hashFile(join(HERE,'current-contract-traces.mjs')),
        contractSourceFiles:pin.sourceFiles},
      historicalRaw:{state:raw.state,allTraceFieldsMatch:raw.allTraceFieldsMatch,error:raw.error,
        referenceCargoSucceeded:raw.reference.cargoSucceeded,candidateCargoSucceeded:raw.production.cargoSucceeded,
        differences,invocations:summaries},currentContract,
      limitations:['This is a read-only check of the original ac8 evidence, not a rerun or acceptance of a later HEAD.',
        'Candidate Cargo exit 101 and any forced-cleanup error are preserved; this report does not turn execution into success.',
        'This proves only the specified 36-test trace contract, not full Source/Retail parity, performance, or final audits.']};
  }catch(error){error.historicalRaw=historicalRaw;throw error;}
}

export function runCurrentContract({evidenceRoot,output,contractRoot=ROOT}){
  fail(evidenceRoot&&output,'usage');
  const evidence=noLinks(evidenceRoot),parent=noLinks(dirname(resolve(output))),destination=join(parent,basename(resolve(output)));
  fail(!inside(evidence,destination)&&!inside(destination,evidence)&&!existsSync(destination),'fresh_separate_output_required');
  mkdirSync(destination); // Exclusive directory creation: never overwrite a run.
  let report;
  try{report=inspectCurrentContract(evidence,contractRoot);}
  catch(error){report={schema:'lycheedev.emmy-current-contract-report.v1',state:'failed',
    historicalRaw:error.historicalRaw??{state:'not_verified',meaning:'The original report was not modified; no historical success is inferred.'},
    currentContract:{state:'failed',error:error.stack||error.message}};}
  writeFileSync(join(destination,'current-contract-report.json'),JSON.stringify(report,null,2)+'\n',{flag:'wx'});
  return report;
}

if(process.argv[1]&&resolve(process.argv[1])===resolve(import.meta.filename)){
  const args=process.argv.slice(2),options={};
  fail(args.length===4||args.length===6,'usage');
  for(let i=0;i<args.length;i+=2){
    const key={'--evidence-root':'evidenceRoot','--output':'output','--contract-root':'contractRoot'}[args[i]];
    fail(key&&!Object.hasOwn(options,key)&&args[i+1],'usage');options[key]=args[i+1];
  }
  const report=runCurrentContract(options);
  console.log(JSON.stringify({state:report.state,historicalRaw:report.historicalRaw.state,currentContract:report.currentContract.state},null,2));
  if(report.currentContract.state!=='passed')process.exitCode=1;
}
