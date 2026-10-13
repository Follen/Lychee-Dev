// Verify native projections against independently fixed source expectations.
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {existsSync,readFileSync,writeFileSync} from 'node:fs';
import {dirname,join,resolve} from 'node:path';
import {hashFile} from '../../tools/rust-build.mjs';
const [reportName,outName]=process.argv.slice(2);assert(reportName&&outName);
const reportPath=resolve(reportName),out=resolve(outName);assert(!existsSync(out));
const report=JSON.parse(readFileSync(reportPath));assert.equal(report.schema,'lycheedev.emmy-native-context-owned.v1');
const contract=JSON.parse(readFileSync(join(dirname(reportPath),'fixed-contract.json')));
assert.equal(contract.schema,'lycheedev.emmy-native-context-contract.v1');
const checks=[];
function check(name,expected,actual){let passed=true,error;try{assert.deepEqual(actual,expected);}catch(e){passed=false;error=e.message;}checks.push({name,expected,actual,passed,error});}
const sha=text=>createHash('sha256').update(text).digest('hex');
const same=(a,b)=>a.file===b.file&&a.start===b.start&&a.end===b.end;
const inputs=new Map(report.artifact.fixtures.map(input=>[input.file,readFileSync(join(dirname(report.artifact.sealedBuild.binary),'inputs/tests/rust-source-engine/wow',input.diskFile))]));
check('actual owned execution and syntax',true,report.transportComplete&&report.handlesReaped&&report.syntaxComplete);
check('fixed complete assertion contract',true,report.assertionContractComplete);
check('sealed binary',report.artifact.binarySHA256,hashFile(report.artifact.binary));check('sealed matching PDB',report.artifact.pdbSHA256,hashFile(report.artifact.pdb));
for(const input of contract.inputs){check(`input ${input.file}`,input.sha256,sha(inputs.get(input.file)));check(`input bytes ${input.file}`,input.bytes,inputs.get(input.file).length);}
const scenarios=report.frames.filter(frame=>frame.event==='scenario'),queries=report.frames.filter(frame=>frame.event==='native_query');
check('three ordered complete scenarios',contract.scenarios,scenarios.map(row=>row.name));check('all 54 native queries',54,queries.length);
for(const frame of scenarios){
 const graph=frame.graph,parameters=JSON.parse(graph.snapshot_inputs_json);
 check(`${frame.name}: snapshot input object`,parameters,graph.snapshot_inputs);
 check(`${frame.name}: exact input digest`,sha(graph.snapshot_inputs_json),frame.snapshotKey);
 check(`${frame.name}: graph snapshot key`,frame.snapshotKey,graph.snapshot_key);
 check(`${frame.name}: exact engine patch`,report.artifact.sealedBuild.contextIdentity.patchSHA256,parameters.engine.patchSHA256);
 check(`${frame.name}: capability profile`,contract.capabilityProfile,parameters.adapter.capabilityProfile);
 check(`${frame.name}: rule version`,contract.rulesVersion,parameters.adapter.rulesVersion);
 check(`${frame.name}: input set`,contract.inputs.map(row=>({file:row.file,bytes:row.bytes,sha256:row.sha256})),parameters.files);
 check(`${frame.name}: no investigated source/config execution`,false,parameters.configExecution);
 for(const name of ['values','functions','uses','calls','writes','copies'])check(`${frame.name}: contiguous ${name} identities`,graph[name].map((_,i)=>i),graph[name].map(row=>row.id));
}
check('three isolated context identities',3,new Set(scenarios.map(row=>row.snapshotKey)).size);
for(const expected of contract.queryExpectations){
 const tag=`${expected.scenario}:${expected.query}:${expected.interleaveStep}`;
 const rows=queries.filter(row=>row.scenario===expected.scenario&&row.query===expected.query&&row.interleaveStep===expected.interleaveStep);
 check(`${tag}: exact query occurrence`,1,rows.length);if(rows.length!==1)continue;
 const row=rows[0],scenario=scenarios.find(row=>row.name===expected.scenario),graph=scenario.graph;
 check(`${tag}: original source site`,expected.sourceSite,row.sourceSite);
 check(`${tag}: query context`,scenario.snapshotKey,row.contextIdentity);check(`${tag}: query snapshot`,scenario.snapshotKey,row.snapshotKey);
 check(`${tag}: native family`,expected.nativeFamily,row.nativeFamily);
 check(`${tag}: type coverage`,expected.typeCoverage,row.typeCoverage.state);
 check(`${tag}: candidate count`,expected.candidateCount,row.nativeCandidates.length);
 const call=graph.calls[row.callFactId],callee=graph.uses[call?.callee],result=graph.uses[call?.resultFactId];
 check(`${tag}: exact graph call site`,row.sourceSite,call?.site);
 check(`${tag}: graph-owned result`,call?.resultFactId,row.resultFactId);
 check(`${tag}: callee proof`,callee?.proof_id,row.calleeProofId);check(`${tag}: result proof`,result?.proof_id,row.resultProofId);
 check(`${tag}: type dependency proofs`,[callee?.proof_id,result?.proof_id],row.typeCoverage.proofIds);
 if(expected.selectedFunction===null){
  check(`${tag}: unavailable binding`, 'unavailable',row.calleeDecision.state);
  check(`${tag}: native type remains unknown`,'Unknown',row.nativeLuaType);
  check(`${tag}: unavailable reason exists`,true,typeof row.typeCoverage.reason==='string'&&row.typeCoverage.reason.length>0);
 }else{
  check(`${tag}: selected bound callee`,'bound',row.calleeDecision.state);
  const value=graph.values[callee?.decision?.detail],functionId=value?.kind?.detail,selected=graph.functions[functionId];
  check(`${tag}: graph callee is a function`,'function',value?.kind?.kind);
  check(`${tag}: selected original function`,expected.selectedFunction,selected?.name?.split(/[.:]/).at(-1));
  check(`${tag}: exact source closure`,selected?.closure,row.calleeDecision.selectedValue.closure);
  check(`${tag}: selected value identity`,`${scenario.snapshotKey}:value:${value?.id}`,row.calleeDecision.valueIdentity);
  check(`${tag}: selected callee proof`,callee?.proof_id,row.calleeDecision.proofId);
  check(`${tag}: known result is not Any/Unknown`,false,['Any','Unknown'].includes(row.nativeLuaType));
  check(`${tag}: clear result has no unavailable reason`,null,row.typeCoverage.reason);
  const candidate=row.nativeCandidates[0];
  check(`${tag}: candidate source mapping`,selected?.closure,candidate?.originFunction);
  check(`${tag}: native method alignment`,expected.selectedFunction==='Echo',candidate?.isMethod);
  check(`${tag}: native generic declaration`,expected.selectedFunction==='Replacement'?[]:['T'],candidate?.genericParameters);
  check(`${tag}: primitive result representation`,true,expected.nativeFamily==='number'?/^(?:Number|Integer|IntegerConst\(|FloatConst\()/.test(row.nativeLuaType):/^(?:String|StringConst\()/.test(row.nativeLuaType));
 }
}
for(const scenario of contract.scenarios){
 const diagnostic=report.frames.find(frame=>frame.event==='native_diagnostics'&&frame.scenario===scenario);
 check(`${scenario}: diagnostic byte coordinate`,'original_utf8_bytes',diagnostic?.coordinate);
 const expected=contract.diagnosticExpectations.filter(row=>row.scenario===scenario&&row.expectedCount===1).map(row=>({code:row.code,sourceSite:row.sourceSite}));
 const key=row=>`${row.code}:${row.sourceSite.file}:${row.sourceSite.start}:${row.sourceSite.end}`;
 check(`${scenario}: exact complete diagnostic set`,expected.sort((a,b)=>key(a).localeCompare(key(b))),diagnostic?.diagnostics.map(row=>({code:row.code,sourceSite:row.sourceSite})).sort((a,b)=>key(a).localeCompare(key(b))));
 const foreign=report.frames.find(frame=>frame.event==='native_foreign_query'&&frame.scenario===scenario);
 check(`${scenario}: foreign model inherited context`,scenarios.find(row=>row.name===scenario).snapshotKey,foreign?.contextIdentity);
 check(`${scenario}: selected formal parameter`,'parameter',foreign?.decision.selectedValue.kind);
 check(`${scenario}: formal source spelling`,'value',inputs.get(foreign.sourceSite.file).subarray(foreign.sourceSite.start,foreign.sourceSite.end).toString());
 const refs=report.frames.find(frame=>frame.event==='native_references'&&frame.scenario===scenario);
 check(`${scenario}: no raw reference fallback`,false,refs?.rawIndexFallback);
 check(`${scenario}: references use value overlay`,'bound',refs?.references.state);
 const graph=scenarios.find(row=>row.name===scenario).graph;
 for(const reference of refs?.references.items??[]){
  const facts=graph.uses.filter(fact=>same(fact.site,reference.site)&&fact.proof_id===reference.proofId);
  check(`${scenario}: reference ${reference.proofId} uniquely graph-owned`,1,facts.length);
  if(facts.length===1){const fact=facts[0];check(`${scenario}: reference value ${reference.proofId}`,`${graph.snapshot_key}:value:${fact.decision.detail}`,refs.valueIdentity);check(`${scenario}: reference context ${reference.proofId}`,fact.context,reference.executionContext);}
 }
}
const failed=checks.filter(row=>!row.passed),result={schema:'lycheedev.emmy-native-independent.v1',passed:failed.length===0,checks,failed:failed.length,sourceReportSHA256:hashFile(reportPath),contractSHA256:hashFile(join(dirname(reportPath),'fixed-contract.json')),
 scope:'fixed 54 native query projections, source/callee/result proof links, diagnostic sets, foreign parameter identity and value-overlay references; not full Source or engine selection'};
writeFileSync(out,JSON.stringify(result,null,2)+'\n');console.log(JSON.stringify({report:out,passed:result.passed,checks:checks.length,failed:failed.length}));if(!result.passed)process.exitCode=1;
