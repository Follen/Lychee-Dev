import test from 'node:test';
import assert from 'node:assert/strict';
import {inspectDiagnostic} from './diagnostics.mjs';
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
