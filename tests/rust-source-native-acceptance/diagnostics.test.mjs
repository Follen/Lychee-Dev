import test from 'node:test';
import assert from 'node:assert/strict';
import {inspectDiagnostic,inspectTerminal} from './diagnostics.mjs';
const raw=Buffer.from('{"schema":"lycheedev.native-source-terminal-diagnostic.v1","workerPid":1328,"workerCreated":134365363311915552,"phase":"owner_terminal","error":{"code":"command.deadline"},"processMemory":{"identity":{"pid":1328,"created":134365363311915552}}}');
const failed=Buffer.from('{"ok":false,"error":{"code":"external.io","details":{"workerPid":1328,"workerCreated":134365363311915552,"workerExitCode":5}}}');
test('failure terminal preserves exact FILETIME and primary IPC error independently',()=>{
 const result=inspectDiagnostic(raw,failed,1);assert.equal(result.identity,'matched_authenticated_failure_peer');assert.equal(result.value.workerCreated,'134365363311915552');assert.equal(result.value.error.code,'command.deadline');assert.equal(JSON.parse(failed).error.code,'external.io');
});
test('stale owner, wrong schema and incoherent process identity are never attributed',()=>{
 for(const changed of [raw.toString().replaceAll('1328','1329'),raw.toString().replace('terminal-diagnostic.v1','request-diagnostic.v1'),raw.toString().replace('"pid":1328','"pid":42')])assert.notEqual(inspectDiagnostic(Buffer.from(changed),failed,1).identity,'matched_authenticated_failure_peer');
});
test('PID only success, no envelope and malformed diagnostic remain explicit uncertainty',()=>{
 const success=Buffer.from('{"result":{"nativeResearch":{"nativeSession":{"workerPid":1328}}}}');assert.equal(inspectDiagnostic(raw,success,1).identity,'pid_only');assert.equal(inspectDiagnostic(raw,Buffer.from(''),1).identity,'unmatched');assert.equal(inspectDiagnostic(Buffer.from('{'),failed,1).identity,'unavailable');
});
import {mkdtempSync,mkdirSync,writeFileSync,readFileSync,rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {captureDiagnostics,snapshotDiagnostics} from './diagnostics.mjs';
test('real failed-command evidence is saved even when terminal JSON is malformed or absent',()=>{
 const root=mkdtempSync(join(tmpdir(),'source-diagnostics-'));
 try{
  const home=join(root,'home'),out=join(root,'out');mkdirSync(join(home,'tmp'),{recursive:true});mkdirSync(out);
  const before=snapshotDiagnostics(home);assert.deepEqual(before,[null,null]);
  writeFileSync(join(home,'tmp','source-native-last-terminal.json'),raw);
  const captured=captureDiagnostics({home,out,prefix:'007-session-cold-refs',stdout:failed,before});
  assert.equal(captured[0].state,'absent');assert.equal(captured[1].identity,'matched_authenticated_failure_peer');assert.equal(captured[1].changedSinceCommand,true);
  assert.deepEqual(readFileSync(join(out,captured[1].path)),raw);
  const later=captureDiagnostics({home,out,prefix:'008-final-close',stdout:Buffer.from('{}'),before:snapshotDiagnostics(home)});
  assert.equal(later[1].changedSinceCommand,false);assert.equal(later[1].identity,'unmatched');
  writeFileSync(join(home,'tmp','source-native-last-request.json'),'{');
  const malformed=captureDiagnostics({home,out,prefix:'009-failed',stdout:failed,before});assert.equal(malformed[0].identity,'unavailable');assert.equal(readFileSync(join(out,malformed[0].path),'utf8'),'{');
  writeFileSync(join(home,'tmp','source-native-last-request.json'),Buffer.alloc((1<<20)+1));
  assert.equal(captureDiagnostics({home,out,prefix:'010-too-large',stdout:failed,before})[0].state,'capture_failed');
 }finally{rmSync(root,{recursive:true});}
});

test('analysis phase memory is child-scoped and cannot claim owner aggregate attribution',()=>{
 const child=Buffer.from('{"schema":"lycheedev.native-source-request-diagnostic.v1","workerPid":1328,"workerCreated":134365363311915552,"analysisPid":1329,"analysisCreated":134365363311915553,"requestDeadlineUnixMs":1234,"phase":"emmy_indexed","processMemory":{"identity":{"pid":1329,"created":134365363311915553}}}');
 const result=inspectDiagnostic(child,failed,0);
 assert.equal(result.identity,'matched_authenticated_failure_peer');
 assert.equal(result.value.analysisCreated,'134365363311915553');
 assert.equal(result.memoryAttribution,'child_identity_only_parent_unverified');
 assert.equal(result.completionEvidence,false);
 const wrong=Buffer.from(child.toString().replace('"pid":1329','"pid":1328'));
 assert.equal(inspectDiagnostic(wrong,failed,0).identity,'invalid');
});

const terminalEvent=()=>({
 schema:'lycheedev.native-source-analysis-terminal.v1',authentication:'same_executable_authenticated_owner_response',requestDeadlineUnixMs:1234,
 terminal:{workerPid:1328,workerCreated:'134365363311915552',analysisPid:1329,analysisCreated:'134365363311915553',
  childHelloAuthenticated:true,exit:{code:0xc0000017,observedBeforeTermination:true},exitWaitError:null,
  jobMemory:{peakJobMemoryUsed:2144694272,peakProcessMemoryUsed:2144694272,jobMemoryLimit:2147483648,activeProcesses:0,limitFlags:0x2200},jobMemoryError:null,
  terminateRequested:true,jobTerminateAccepted:true,rootKillAccepted:false,rootReaped:true,pipeWorkerJoined:true,jobEmpty:true,analysisRetired:true,handleCloseErrors:[]},
 exitScope:'exact guarded child handle; cleanup exit does not establish original failure cause',
 jobScope:'exact held analysis Job; historical commit peaks, not current aggregate or RSS',limitNotification:'unavailable_not_observed',
});
const terminalBytes=event=>Buffer.from(`${JSON.stringify(event)}\n`);
test('authenticated held-child terminal preserves exact identities and unsigned exit without cause inference',()=>{
 const event=terminalEvent(),result=inspectTerminal(terminalBytes(event));
 assert.equal(result.state,'observed');assert.equal(result.identity,'authenticated_owner_held_child');
 assert.equal(result.value.terminal.workerCreated,'134365363311915552');
 assert.equal(result.value.terminal.analysisCreated,'134365363311915553');
 assert.equal(result.value.terminal.exit.code,3221225495);
 assert.equal(result.exitObservation,'before_supervisor_termination');
 assert.equal(result.limitNotification,'unavailable_not_observed');assert.equal(result.completionEvidence,false);
 assert.equal(result.cause,undefined);assert.equal(result.oom,undefined);
 assert.equal(inspectTerminal(Buffer.alloc(0)).state,'unavailable');
});
test('cleanup exit and missing exit remain distinct from original terminal observation',()=>{
 const event=terminalEvent();event.terminal.exit={code:7,observedBeforeTermination:false};
 assert.equal(inspectTerminal(terminalBytes(event)).exitObservation,'after_supervisor_termination');
 event.terminal.exit=null;event.terminal.jobMemory=null;
 assert.equal(inspectTerminal(terminalBytes(event)).exitObservation,'unavailable');
});
test('terminal reader rejects partial duplicate unbound and incoherent records',()=>{
 const bytes=terminalBytes(terminalEvent());
 assert.equal(inspectTerminal(bytes.subarray(0,bytes.length-1)).state,'unavailable');
 assert.equal(inspectTerminal(Buffer.concat([bytes,bytes])).state,'ambiguous');
 for(const change of [
  e=>{e.authentication='file_claim';},e=>{e.terminal.workerCreated='0';},
  e=>{e.terminal.analysisPid=e.terminal.workerPid;},e=>{e.terminal.rootReaped=false;},
  e=>{e.terminal.jobEmpty=false;},e=>{e.terminal.exit.code=-1;},
  e=>{e.terminal.handleCloseErrors=['unconfirmed'];},e=>{e.limitNotification='memory_limit_hit';},
 ]){const event=terminalEvent();change(event);assert.equal(inspectTerminal(terminalBytes(event)).state,'invalid');}
});
test('saved phase attribution requires exact owner child and request match from actual terminal stderr',()=>{
 const phase={schema:'lycheedev.native-source-request-diagnostic.v1',workerPid:1328,workerCreated:'134365363311915552',analysisPid:1329,analysisCreated:'134365363311915553',requestDeadlineUnixMs:1234,phase:'query_execute_operations',processMemory:{identity:{pid:1329,created:'134365363311915553'}}};
 const terminal=inspectTerminal(terminalBytes(terminalEvent())),stdout=Buffer.from('{"ok":false,"error":{"code":"external.io","message":"failed to fill whole buffer"}}');
 const inspect=value=>inspectDiagnostic(Buffer.from(JSON.stringify(value)),stdout,0,terminal);
 const matched=inspect(phase);assert.equal(matched.identity,'matched_authenticated_analysis_terminal');assert.equal(matched.memoryAttribution,'authenticated_owner_child_identity');assert.equal(matched.completionEvidence,false);
 for(const change of [p=>{p.workerCreated='134365363311915554';},p=>{p.analysisCreated='134365363311915554';p.processMemory.identity.created=p.analysisCreated;},p=>{p.requestDeadlineUnixMs++;},p=>{p.analysisPid++;p.processMemory.identity.pid=p.analysisPid;}]){
  const altered=structuredClone(phase);change(altered);assert.notEqual(inspect(altered).identity,'matched_authenticated_analysis_terminal');
 }
 assert.equal(inspectDiagnostic(Buffer.from(JSON.stringify(phase)),stdout,0).identity,'unmatched');
 assert.equal(JSON.parse(stdout).error.code,'external.io');
});

import {inspectPhaseJournal} from './diagnostics.mjs';
const phaseEvent=(sequence=0,phase='query_references_project')=>({
 schema:'lycheedev.native-source-request-phase.v1',workerPid:1328,workerCreated:'134365363311915552',analysisPid:1329,analysisCreated:'134365363311915553',requestDeadlineUnixMs:1234,
 phase,elapsedMs:sequence,sequence,callbackOrdinal:sequence+1,previousCallbackOrdinal:sequence?sequence:null,fileVisit:0,
 omittedBeforeThisSample:0,detailSamplingExhausted:false,ordinarySamplingExhausted:false,
 processMemory:{identity:{pid:1329,created:'134365363311915553'},private_commit:1000+sequence,working_set:2000,peak_private_commit:1000+sequence,peak_working_set:2000},privateCommitDelta:sequence?1:null,workingSetDelta:sequence?0:null,
});
const journalBytes=events=>Buffer.from(events.map(event=>JSON.stringify(event)+'\n').join(''));
test('phase journal authenticates complete sample prefix and preserves incomplete trailing bytes',()=>{
 const terminal=inspectTerminal(terminalBytes(terminalEvent()));
 const complete=journalBytes([phaseEvent(0),phaseEvent(1,'query_type_observations')]);
 const prefix=Buffer.concat([complete,Buffer.from('{"schema":"interrupted')]);
 const result=inspectPhaseJournal(prefix,failed,terminal);
 assert.equal(result.state,'prefix_with_incomplete_tail');assert.equal(result.records,2);assert.equal(result.identity,'matched_authenticated_analysis_terminal');
 assert.equal(result.tailBytes,prefix.length-complete.length);assert.equal(result.completionEvidence,false);assert.equal(result.cause,undefined);
 assert.equal(result.samples[1].value.phase,'query_type_observations');assert.equal(result.samples[1].value.analysisCreated,'134365363311915553');
 assert.equal(inspectPhaseJournal(complete,failed,terminal).state,'observed_prefix');
 assert.equal(inspectPhaseJournal(complete,failed).identity,'unverified_or_empty');
});
test('phase journal exposes sampling gaps and capacity while rejecting duplicate or incoherent events',()=>{
 const second=phaseEvent(1,'query_type_api');second.callbackOrdinal=200;second.fileVisit=49;second.omittedBeforeThisSample=198;second.detailSamplingExhausted=true;second.ordinarySamplingExhausted=true;
 const accepted=inspectPhaseJournal(journalBytes([phaseEvent(),second]),failed,inspectTerminal(terminalBytes(terminalEvent())));
 assert.equal(accepted.records,2);assert.equal(accepted.samplingGaps,198);assert.equal(accepted.detailSamplingExhausted,true);assert.equal(accepted.ordinarySamplingExhausted,true);
 for(const change of [e=>{e.sequence=0;},e=>{e.previousCallbackOrdinal=0;},e=>{e.callbackOrdinal=0;},e=>{e.processMemory.identity.pid=42;},e=>{e.elapsedMs=-1;},e=>{e.phase='x'.repeat(65);}]){
  const altered=phaseEvent(1);change(altered);assert.equal(inspectPhaseJournal(journalBytes([phaseEvent(),altered]),failed).state,'invalid');
 }
 const other=phaseEvent(1);other.requestDeadlineUnixMs++;
 assert.equal(inspectPhaseJournal(journalBytes([phaseEvent(),other]),failed,inspectTerminal(terminalBytes(terminalEvent()))).identity,'unverified_or_empty');
 assert.equal(inspectPhaseJournal(Buffer.alloc(128*1024+1),failed).reason,'phase_byte_limit');
 assert.equal(inspectPhaseJournal(journalBytes(Array.from({length:129},(_,i)=>phaseEvent(i))),failed).state,'invalid');
});
test('phase journal capture preserves exact bytes and forbids supplied path traversal',()=>{
 const root=mkdtempSync(join(tmpdir(),'source-phase-journal-'));
 try{
  const home=join(root,'home'),out=join(root,'out');mkdirSync(join(home,'tmp'),{recursive:true});mkdirSync(out);
  const name='source-native-phases-10.jsonl';
  const phase=phaseEvent();phase.schema='lycheedev.native-source-request-diagnostic.v1';phase.phaseJournal={file:name,state:'opened'};
  const bytes=Buffer.concat([journalBytes([phaseEvent()]),Buffer.from('{')]);
  writeFileSync(join(home,'tmp',name),bytes);writeFileSync(join(home,'tmp','source-native-last-request.json'),JSON.stringify(phase));
  const results=captureDiagnostics({home,out,prefix:'001-with-journal',stdout:failed,stderr:terminalBytes(terminalEvent())});
  assert.equal(results.length,3);assert.equal(results[2].identity,'matched_authenticated_analysis_terminal');assert.equal(results[2].tailBytes,1);
  assert.deepEqual(readFileSync(join(out,results[2].path)),bytes);assert.equal(results[2].changedSinceCommand,null);
  phase.phaseJournal.file='../private';writeFileSync(join(home,'tmp','source-native-last-request.json'),JSON.stringify(phase));
  const blocked=captureDiagnostics({home,out,prefix:'002-invalid-name',stdout:failed});assert.equal(blocked[2].reason,'invalid_phase_journal_name');
 }finally{rmSync(root,{recursive:true});}
});

test('an unowned collision journal is never adopted even when its filename matches',()=>{
 const root=mkdtempSync(join(tmpdir(),'source-phase-collision-'));
 try{
  const home=join(root,'home'),out=join(root,'out');mkdirSync(join(home,'tmp'),{recursive:true});mkdirSync(out);
  const name='source-native-phases-10.jsonl';
  const phase=phaseEvent();phase.schema='lycheedev.native-source-request-diagnostic.v1';phase.phaseJournal={file:name,state:'unavailable'};
  writeFileSync(join(home,'tmp',name),journalBytes([phaseEvent()]));writeFileSync(join(home,'tmp','source-native-last-request.json'),JSON.stringify(phase));
  const results=captureDiagnostics({home,out,prefix:'003-unowned-journal',stdout:failed,stderr:terminalBytes(terminalEvent())});
  assert.equal(results[2].state,'unavailable');assert.equal(results[2].reason,'journal_not_opened_for_request');assert.equal(results[2].path,undefined);
 }finally{rmSync(root,{recursive:true});}
});


test('non-object JSON records preserve a validated prefix without throwing',()=>{
 const terminal=inspectTerminal(terminalBytes(terminalEvent()));
 for(const invalid of [null,[],42,true,'text']){
  const result=inspectPhaseJournal(journalBytes([phaseEvent(),invalid]),failed,terminal);
  assert.equal(result.state,'invalid');assert.equal(result.records,1);assert.equal(result.completionEvidence,false);
  assert.equal(inspectDiagnostic(Buffer.from(JSON.stringify(invalid)),failed,0).identity,'unavailable');
 }
 assert.equal(inspectTerminal(Buffer.from('null\n[]\n42\n')).state,'unavailable');
});

test('exhausted slots and unauthenticated slot pointers are never sealed',()=>{
 const root=mkdtempSync(join(tmpdir(),'source-phase-unverified-'));
 try{
  const home=join(root,'home'),out=join(root,'out');mkdirSync(join(home,'tmp'),{recursive:true});mkdirSync(out);
  const name='source-native-phases-10.jsonl',phase=phaseEvent();phase.schema='lycheedev.native-source-request-diagnostic.v1';
  writeFileSync(join(home,'tmp',name),journalBytes([phaseEvent()]));
  for(const [i,journal] of [{file:null,state:'slots_exhausted'},{file:name,state:'opened'}].entries()){
   phase.phaseJournal=journal;writeFileSync(join(home,'tmp','source-native-last-request.json'),JSON.stringify(phase));
   const results=captureDiagnostics({home,out,prefix:`004-unavailable-${i}`,stdout:failed});
   assert.equal(results[2].state,'unavailable');assert.equal(results[2].path,undefined);
   assert.equal(results[2].reason,i?'journal_identity_unverified':'journal_not_opened_for_request');
  }
 }finally{rmSync(root,{recursive:true});}
});
