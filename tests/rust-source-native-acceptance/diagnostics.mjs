// Evidence-only: capture failed native owner observations before an assertion
// unwinds the controller. Neither a stale file nor an exit code proves a cause.
import {lstatSync,readFileSync,writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {createHash} from 'node:crypto';
const digest=bytes=>createHash('sha256').update(bytes).digest('hex');
export const names=Object.freeze(['source-native-last-request.json','source-native-last-terminal.json']);
const schemas=Object.freeze(['lycheedev.native-source-request-diagnostic.v1','lycheedev.native-source-terminal-diagnostic.v1']);
const peerJson=raw=>JSON.parse(raw.replace(/("(?:created|workerCreated|analysisCreated|ownerCreated|parent_created)"\s*:\s*)([0-9]+)(?=\s*[,}])/g,'$1"$2"'));
// Only complete lines emitted by the authenticated calling CLI qualify. These
// bytes are already sealed in the command's original stderr, never reconstructed
// from a stale diagnostic file or a reopened PID.
export function inspectTerminal(stderr=Buffer.alloc(0)){
 const candidates=[];
 for(const line of stderr.toString('utf8').split(/(?<=\n)/)){
  if(!line.endsWith('\n')||Buffer.byteLength(line)>64*1024)continue;
  let event;try{event=peerJson(line);}catch{continue;}
  if(event.schema==='lycheedev.native-source-analysis-terminal.v1')candidates.push(event);
 }
 if(candidates.length!==1)return {state:candidates.length?'ambiguous':'unavailable',reason:candidates.length?'multiple_terminal_records':'no_complete_terminal_record',completionEvidence:false};
 const value=candidates[0],t=value.terminal;
 const pid=n=>Number.isInteger(n)&&n>0&&n<=0xffffffff;
 const creation=n=>typeof n==='string'&&/^[1-9][0-9]*$/.test(n);
 const flag=n=>typeof n==='boolean';
 const exit=t?.exit;
 const valid=value.authentication==='same_executable_authenticated_owner_response'
  &&Number.isSafeInteger(value.requestDeadlineUnixMs)&&value.requestDeadlineUnixMs>0
  &&pid(t?.workerPid)&&creation(t?.workerCreated)
  &&((t.analysisPid===null&&t.analysisCreated===null)||(pid(t.analysisPid)&&creation(t.analysisCreated)&&t.analysisPid!==t.workerPid))
  &&['childHelloAuthenticated','terminateRequested','jobTerminateAccepted','rootKillAccepted','rootReaped','pipeWorkerJoined','jobEmpty','analysisRetired'].every(key=>flag(t[key]))
  &&(exit===null||(exit&&flag(exit.observedBeforeTermination)&&(exit.code===null||(Number.isInteger(exit.code)&&exit.code>=0&&exit.code<=0xffffffff))))
  &&(!exit||t.rootReaped)&&(!t.analysisRetired||(t.rootReaped&&t.pipeWorkerJoined&&t.jobEmpty&&Array.isArray(t.handleCloseErrors)&&t.handleCloseErrors.length===0))
  &&value.limitNotification==='unavailable_not_observed';
 if(!valid)return {state:'invalid',reason:'incoherent_terminal_record',completionEvidence:false,value};
 return {state:'observed',identity:'authenticated_owner_held_child',completionEvidence:false,
  exitObservation:exit?(exit.observedBeforeTermination?'before_supervisor_termination':'after_supervisor_termination'):'unavailable',
  limitNotification:'unavailable_not_observed',value};
}
export function inspectDiagnostic(raw,stdout,index,terminal){
  let value,envelope;
  try{value=peerJson(raw.toString('utf8'));}catch{return {identity:'unavailable',reason:'invalid_diagnostic_json'};}
  try{envelope=peerJson(stdout.toString('utf8'));}catch{}
  const error=envelope?.error?.details;
  const pid=error?.workerPid??envelope?.result?.nativeResearch?.nativeSession?.workerPid;
  const created=error?.workerCreated;
  const identity=value.processMemory?.identity;
  const hasAnalysis=value.analysisPid!==undefined&&value.analysisPid!==null;
  const memoryPid=hasAnalysis?value.analysisPid:value.workerPid;
  const memoryCreated=hasAnalysis?value.analysisCreated:value.workerCreated;
  const analysisCoherent=!hasAnalysis||(Number.isInteger(value.analysisPid)&&value.analysisPid>0&&value.analysisPid!==value.workerPid
    &&typeof value.analysisCreated==='string'&&/^[1-9][0-9]*$/.test(value.analysisCreated));
  const coherent=analysisCoherent&&value.schema===schemas[index]&&Number.isInteger(value.workerPid)&&value.workerPid>0
    &&typeof value.workerCreated==='string'&&/^[0-9]+$/.test(value.workerCreated)
    &&(!identity||(identity.pid===memoryPid&&identity.created===memoryCreated));
  const matched=coherent&&pid===value.workerPid&&created===value.workerCreated;
  const held=terminal?.state==='observed'?terminal.value:null;
  const matchedTerminal=coherent&&hasAnalysis&&index===0&&held?.terminal.workerPid===value.workerPid
    &&held.terminal.workerCreated===value.workerCreated&&held.terminal.analysisPid===value.analysisPid
    &&held.terminal.analysisCreated===value.analysisCreated&&held.requestDeadlineUnixMs===value.requestDeadlineUnixMs;
  return {identity:matchedTerminal?'matched_authenticated_analysis_terminal':matched?'matched_authenticated_failure_peer':!coherent?'invalid':pid===value.workerPid&&created===undefined?'pid_only':'unmatched',
    expectedPeer:{pid:pid??null,created:created??null},completionEvidence:false,
    memoryAttribution:matchedTerminal?'authenticated_owner_child_identity':hasAnalysis?'child_identity_only_parent_unverified':'owner_identity_only',value};
}
function read(home,name){
  const path=join(home,'tmp',name),stat=lstatSync(path);
  if(!stat.isFile()||stat.isSymbolicLink()||stat.size>1<<20)throw new Error('diagnostic_file_budget_or_type');
  const raw=readFileSync(path);if(raw.length>1<<20)throw new Error('diagnostic_file_budget');return raw;
}
export function snapshotDiagnostics(home){
  return names.map(name=>{try{return digest(read(home,name));}catch{return null;}});
}
export function captureDiagnostics({home,out,prefix,stdout,stderr,before}){
  if(!/^[a-z0-9][a-z0-9_-]{0,120}$/.test(prefix))throw new Error('invalid_diagnostic_evidence_prefix');
  const terminal=inspectTerminal(stderr);
  return names.map((name,index)=>{
    try{
      const raw=read(home,name),sha256=digest(raw),path=`${prefix}.${name}`;
      writeFileSync(join(out,path),raw,{flag:'wx'});
      return {path,bytes:raw.length,sha256,changedSinceCommand:sha256!==before?.[index],...inspectDiagnostic(raw,stdout,index,terminal)};
    }catch(error){return {source:name,state:error.code==='ENOENT'?'absent':'capture_failed',reason:error.message};}
  });
}
