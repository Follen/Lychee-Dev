// Evidence-only: capture failed native owner observations before an assertion
// unwinds the controller. Neither a stale file nor an exit code proves a cause.
import {lstatSync,readFileSync,writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {createHash} from 'node:crypto';
const digest=bytes=>createHash('sha256').update(bytes).digest('hex');
export const names=Object.freeze(['source-native-last-request.json','source-native-last-terminal.json']);
const schemas=Object.freeze(['lycheedev.native-source-request-diagnostic.v1','lycheedev.native-source-terminal-diagnostic.v1']);
const peerJson=raw=>JSON.parse(raw.replace(/("(?:created|workerCreated|analysisCreated|ownerCreated|parent_created)"\s*:\s*)([0-9]+)(?=\s*[,}])/g,'$1"$2"'));
export function inspectDiagnostic(raw,stdout,index){
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
  return {identity:matched?'matched_authenticated_failure_peer':!coherent?'invalid':pid===value.workerPid&&created===undefined?'pid_only':'unmatched',
    expectedPeer:{pid:pid??null,created:created??null},completionEvidence:false,
    memoryAttribution:hasAnalysis?'child_identity_only_parent_unverified':'owner_identity_only',value};
}
function read(home,name){
  const path=join(home,'tmp',name),stat=lstatSync(path);
  if(!stat.isFile()||stat.isSymbolicLink()||stat.size>1<<20)throw new Error('diagnostic_file_budget_or_type');
  const raw=readFileSync(path);if(raw.length>1<<20)throw new Error('diagnostic_file_budget');return raw;
}
export function snapshotDiagnostics(home){
  return names.map(name=>{try{return digest(read(home,name));}catch{return null;}});
}
export function captureDiagnostics({home,out,prefix,stdout,before}){
  if(!/^[a-z0-9][a-z0-9_-]{0,120}$/.test(prefix))throw new Error('invalid_diagnostic_evidence_prefix');
  return names.map((name,index)=>{
    try{
      const raw=read(home,name),sha256=digest(raw),path=`${prefix}.${name}`;
      writeFileSync(join(out,path),raw,{flag:'wx'});
      return {path,bytes:raw.length,sha256,changedSinceCommand:sha256!==before?.[index],...inspectDiagnostic(raw,stdout,index)};
    }catch(error){return {source:name,state:error.code==='ENOENT'?'absent':'capture_failed',reason:error.message};}
  });
}
