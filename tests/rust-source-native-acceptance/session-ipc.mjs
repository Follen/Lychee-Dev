// Real Windows named-pipe acceptance. This module never injects admission
// counters, changes production limits, or runs the analyzed Lua. Run its Node
// controller and peer.ps1 together inside the repository's owned Job runner.
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {spawn,spawnSync} from 'node:child_process';
import {appendFileSync,existsSync,mkdirSync,readFileSync,statSync,writeFileSync} from 'node:fs';
import {dirname,join,resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import {performance} from 'node:perf_hooks';

const HERE=dirname(fileURLToPath(import.meta.url));
export const SESSION_WIRE='lycheedev.native-source-session.v1';
export const SESSION_LIMITS=Object.freeze({quota:600_000,idle:120_000,lifetime:700_000,
  requests:850,requestBytes:2<<20,responseBytes:2<<20,trafficBytes:64<<20,evidenceBytes:192<<20});
const sha256=bytes=>createHash('sha256').update(bytes).digest('hex');
const pretty=value=>JSON.stringify(value,null,2)+'\n';
const sleep=ms=>new Promise(done=>setTimeout(done,ms));
function boundedRead(path,cap){assert(statSync(path).size<=cap,`read budget: ${path}`);const bytes=readFileSync(path);assert(bytes.length<=cap);return bytes;}
function writeNew(path,bytes){writeFileSync(path,bytes,{flag:'wx'});}
function label(value){assert(/^[a-z0-9][a-z0-9_-]{0,95}$/.test(value),`invalid evidence label: ${value}`);return value;}
function budget(start,total){const left=Math.floor(total-(performance.now()-start));assert(left>0,'session group total deadline');return left;}

// FILETIME is wider than JavaScript's exact integer range. Preserve these
// protocol/diagnostic fields as decimal strings, before JSON.parse rounds them.
export function parsePeerJson(raw){return JSON.parse(raw.replace(/("(?:created|workerCreated|analysisCreated|ownerCreated|parent_created)"\s*:\s*)([0-9]+)(?=\s*[,}])/g,'$1"$2"'));}
// Phase files are optional observations from the killable analysis child.
// Neither their presence nor a phase name establishes a request's outcome.
// A previous owner/request must never be presented as this request's progress.
export function classifyPhaseDiagnostic(diagnostic,request,hello){
  const unavailable=reason=>({state:'unavailable',reason,completionEvidence:false});
  if(!diagnostic)return unavailable('absent_or_unreadable');
  if(typeof request!=='object'||request?.action!=='query')return unavailable('not_a_query_observation');
  if(diagnostic.workerPid!==hello.pid||diagnostic.workerCreated!==hello.created)return unavailable('different_owner');
  if(diagnostic.requestDeadlineUnixMs!==request.deadline_unix_ms)return unavailable('different_request_deadline');
  assert.equal(diagnostic.schema,'lycheedev.native-source-request-diagnostic.v1');
  assert(Number.isSafeInteger(diagnostic.analysisPid)&&diagnostic.analysisPid>0&&diagnostic.analysisPid!==hello.pid);
  assert(/^[1-9][0-9]*$/.test(diagnostic.analysisCreated));
  assert(typeof diagnostic.phase==='string'&&diagnostic.phase.length>0);
  return {state:'matching_optional_phase',phase:diagnostic.phase,completionEvidence:false};
}
export function nativePipeName(canonicalHome,logonSid){
  assert(/^S-1-5-5-[0-9]+-[0-9]+$/.test(logonSid),'kernel logon SID required');
  const identity=canonicalHome.replace(/^\\\\\?\\/,'').toLowerCase();
  return `lycheedev-native-source-${sha256(Buffer.from(`${identity}\0${logonSid}`)).slice(0,32)}`;
}
export function tinySource(){return 'function Caller()\n'+Array.from({length:128},(_,n)=>` missing${n}()\n`).join('')+'end\nfunction Empty()\nend\n';}
// Public bootstrap only: the direct IPC fixtures intentionally retain their
// original empty environment and unknown-load-scope contracts.
export const BOOTSTRAP_API_PATH='Interface/AddOns/Blizzard_APIDocumentationGenerated/NativeSessionDocumentation.lua';
export function bootstrapApiSource(){return 'local API = {\n'+
  ' Name = "NativeSession", Type = "System", Namespace = "C_NativeSession",\n'+
  ' Functions = {{Name = "Value", Type = "Function", Returns = {{Name = "value", Type = "number"}}}},\n'+
  '}\nAPIDocumentation:AddDocumentationTable(API)\n';}
export function bootstrapEnvironmentInputSHA256(){
  const raw=bootstrapApiSource();
  return sha256(`${Buffer.byteLength(BOOTSTRAP_API_PATH)}:${BOOTSTRAP_API_PATH}:${Buffer.byteLength(raw)}:${raw}`);
}
export function assertBootstrapEnvironment(result,selection){
  assert.equal(result?.repository,selection.source.repository,'bootstrap environment repository');
  assert.equal(result.product,selection.source.product,'bootstrap environment product');
  assert.equal(result.commit,selection.source.exactCommit,'bootstrap environment source commit');
  assert.equal(result.snapshot,selection.id,'bootstrap environment snapshot');
  assert.equal(result.symbol?.kind,'declaration','bootstrap declaration kind');
  assert.equal(result.symbol.name,'Caller','bootstrap declaration name');
  assert.equal(result.symbol.path,'a.lua','bootstrap declaration path');
  assert.equal(result.symbol.line,1,'bootstrap declaration line');
  assert.equal(result.symbol.endLine,130,'bootstrap declaration end');
  assert.equal(result.coverage?.structural,'complete','bootstrap structural coverage');
  assert.equal(result.coverage.environment,'ready','bootstrap environment coverage');
  assert.equal(result.coverage.scannedFacts,1,'bootstrap declaration must be unambiguous');
  assert.equal(result.truncated,false,'bootstrap preflight truncation');
  const manifest=result.environmentManifest;
  assert.equal(manifest?.schema,'lycheedev.source-environment.v1','bootstrap environment schema');
  assert.equal(manifest.identity?.repository,selection.source.repository,'bootstrap manifest repository');
  assert.equal(manifest.identity.commit,selection.source.exactCommit,'bootstrap manifest commit');
  assert.equal(manifest.identity.client,selection.source.product,'bootstrap manifest client');
  assert.equal(typeof manifest.identity.generatorVersion,'string','bootstrap environment generator');
  assert(manifest.identity.generatorVersion.length>0,'bootstrap environment generator');
  assert.equal(manifest.documents,1,'bootstrap generated API document count');
  assert.equal(manifest.inputSHA256,bootstrapEnvironmentInputSHA256(),'bootstrap authenticated API input digest');
  for(const field of ['definitionsSHA256','factsSHA256'])assert.match(manifest[field],/^[a-f0-9]{64}$/,'bootstrap generated API digest');
  return manifest;
}
export function tinySnapshot(variant,luac){
  assert(/^[a-z0-9_-]+$/.test(variant));
  return {files:{'a.lua':tinySource()},load_documents:{},environment_documents:{},
    source:{repository:'native-session-quota-fixture',product:'retail',variant},luac,
    api:[],environment_manifest:null,load_plans:[],input_gaps:[]};
}
export function tinyQuery(batches){
  assert([0,1,2].includes(batches));
  return {symbol:{kind:'declaration',name:batches===0?'Empty':'Caller',category:'function',confidence:'',
    path:'a.lua',line:batches===0?131:1,endLine:batches===0?132:130},
  direction:batches===0?'outgoing':batches===1?'incoming':'both'};
}
export function fixtureSelection(commit){
  assert(/^[a-f0-9]{40}$/.test(commit));
  // PinnedSet/SourcePin struct field order, with omitted empty optional fields.
  const unsigned={schema:'lycheedev.selection.v1',id:'',source:{repository:'wow-ui-source',product:'retail',
    requestedRef:'refs/heads/native-acceptance',exactCommit:commit,parserRevision:'source-v1'}};
  const identity=JSON.stringify(unsigned).replaceAll('&','\\u0026').replaceAll('<','\\u003c').replaceAll('>','\\u003e')
    .replaceAll('\u2028','\\u2028').replaceAll('\u2029','\\u2029');
  return {...unsigned,id:`PIN-${sha256(identity)}`};
}
function releaseLuac(release){
  const root=join(release,'payload','tool','luac'),manifest=JSON.parse(boundedRead(join(root,'runtime.json'),16<<10));
  assert.equal(manifest.schema,'lycheedev.luac-runtime.v1');assert.equal(manifest.version,'5.1.5');
  assert.equal(manifest.platform,'windows-amd64');assert.equal(manifest.executable,'luac.exe');
  const raw=boundedRead(join(root,'luac.exe'),16<<20);
  assert.equal(raw.length,manifest.bytes);assert.equal(sha256(raw),manifest.sha256);
  return {version:manifest.version,platform:manifest.platform,executable_sha256:manifest.sha256,executable_bytes:manifest.bytes};
}

/** Public CLI bootstrap over a tiny local Git fixture. No startup-token or
 * SQLite writes: --project validates the fixture lock and admits its pin via
 * the ordinary public source index path. Static-only refs extracts/authenticates
 * the generated API literals before native refs opens the owner. invoke(caseId,args,options) is the
 * caller's bounded, evidence-recording CLI launcher (it supplies --home/json).
 */
// project init is path-local in the fixed Go public contract and has no --home.
// All workspace-backed operations retain the explicit isolated acceptance home.
export function acceptanceCliArgs(args, home, {useHome = true} = {}) {
  if (!useHome) assert(args[0] === 'project' && args[1] === 'init', 'only project init omits workspace home');
  return [...args, ...(useHome ? ['--home', home] : []), '--format', 'json'];
}

export async function bootstrapTinyOwner({binary,home,release,evidenceDir,invoke,timeoutMs=60_000}){
  assert.equal(typeof invoke,'function');const started=performance.now();mkdirSync(evidenceDir,{recursive:true});
  const command=(id,args,options={})=>invoke(id,args,{...options,timeout:Math.min(45_000,budget(started,timeoutMs)),phase:'local'});
  command('session-bootstrap-init',['init']);
  const repository=join(home,'tmp','native-session-bootstrap');assert(!existsSync(repository),'fresh tiny fixture required');
  mkdirSync(repository,{recursive:true});writeNew(join(repository,'a.lua'),tinySource());
  writeNew(join(repository,'Main.toc'),'a.lua\n');
  mkdirSync(dirname(join(repository,BOOTSTRAP_API_PATH)),{recursive:true});
  writeNew(join(repository,BOOTSTRAP_API_PATH),bootstrapApiSource());
  const environment={...process.env};for(const key of Object.keys(environment))if(/^GIT_/i.test(key)||key==='GCM_INTERACTIVE')delete environment[key];
  Object.assign(environment,{GIT_CONFIG_NOSYSTEM:'1',GIT_CONFIG_GLOBAL:process.platform==='win32'?'NUL':'/dev/null',
    GIT_TERMINAL_PROMPT:'0',GIT_NO_REPLACE_OBJECTS:'1',GIT_AUTHOR_NAME:'Native acceptance fixture',
    GIT_AUTHOR_EMAIL:'fixture@example.invalid',GIT_COMMITTER_NAME:'Native acceptance fixture',GIT_COMMITTER_EMAIL:'fixture@example.invalid',
    GIT_AUTHOR_DATE:'2000-01-01T00:00:00Z',GIT_COMMITTER_DATE:'2000-01-01T00:00:00Z'});
  let sequence=0;
  function git(args){
    const nullPath=process.platform==='win32'?'NUL':'/dev/null';
    const argv=['-c',`core.hooksPath=${nullPath}`,'-c','gc.auto=0','-c','maintenance.auto=false',...args];
    const result=spawnSync('git',argv,{cwd:repository,env:environment,windowsHide:true,
      timeout:Math.min(15_000,budget(started,timeoutMs)),maxBuffer:1<<20,encoding:'utf8'});
    writeNew(join(evidenceDir,`bootstrap-git-${++sequence}.json`),pretty({command:'git',args:argv,cwd:repository,
      status:result.status,signal:result.signal,error:result.error?.message,stdout:result.stdout,stderr:result.stderr}));
    if(result.error||result.signal){const error=new Error(`owned fixture git failed: ${result.error?.message??result.signal}`);
      error.fatalProcessFailure=true;throw error;}
    assert.equal(result.status,0,`git ${args}: ${result.stderr}`);return result.stdout.trim();
  }
  git(['init','--initial-branch=native-acceptance']);git(['add','a.lua','Main.toc',BOOTSTRAP_API_PATH]);git(['commit','-m','fixed native session acceptance source']);
  const commit=git(['rev-parse','HEAD']),mirror=join(home,'mirrors','wow-ui-source.git');
  assert(!existsSync(mirror),'tiny bootstrap cannot replace an existing source mirror');
  git(['clone','--bare','--no-hardlinks','--','.',mirror]);
  command('session-bootstrap-project',['project','init','--path',repository,'--product','retail'],{useHome:false});
  const selection=fixtureSelection(commit),lock={schema:'lycheedev.project-lock.v1',selection};
  writeNew(join(repository,'lycheedev.lock.json'),pretty(lock));
  writeNew(join(evidenceDir,'bootstrap-fixture.json'),pretty({binary,home,repository,mirror,commit,selection,
    sourceExecuted:false,files:[{path:'a.lua',sha256:sha256(tinySource()),bytes:Buffer.byteLength(tinySource())},
      {path:'Main.toc',sha256:sha256('a.lua\n'),bytes:6},
      {path:BOOTSTRAP_API_PATH,sha256:sha256(bootstrapApiSource()),bytes:Buffer.byteLength(bootstrapApiSource())}],lock}));
  writeNew(join(evidenceDir,'bootstrap-a.lua'),tinySource());
  writeNew(join(evidenceDir,'bootstrap-api.lua'),bootstrapApiSource());
  // Index resolves and registers the project lock through the public CLI. Refs
  // requires an explicit snapshot before its relation validation can proceed.
  const indexed=command('session-bootstrap-index',['source','index','--project',repository]);
  const snapshot=indexed.envelope?.context?.snapshot;
  assert.equal(snapshot,selection.id,'public source index did not return the fixture snapshot');
  // This is the same production environment::from_pin route used by native
  // research. It reads authenticated Git blobs, parses static literals and
  // emits declarations without executing documentation Lua. The harness never
  // injects a generated manifest, declaration record or cache/store entry.
  const environmentResult=command('session-bootstrap-environment',['source','refs','--snapshot',snapshot,'--symbol','Caller',
    '--direction','incoming','--limit','25','--static-only']);
  const preflight=environmentResult.result??environmentResult.envelope?.result;
  const environmentManifest=assertBootstrapEnvironment(preflight,selection);
  assert.equal(preflight.coverage.semantic,'unavailable','static preflight must not start semantic work');
  assert.equal(preflight.complete,false,'static preflight is not native acceptance');
  assert.equal(preflight.nativeResearch,undefined,'static preflight must not open an owner');
  writeNew(join(evidenceDir,'bootstrap-environment.json'),pretty({selection:selection.id,sourceExecuted:false,
    input:{path:BOOTSTRAP_API_PATH,sha256:sha256(bootstrapApiSource()),bytes:Buffer.byteLength(bootstrapApiSource())},environmentManifest}));
  const opened=command('session-bootstrap-open',['source','refs','--snapshot',snapshot,'--symbol','Caller',
    '--direction','incoming','--limit','25','--session-reuse','--release',release]);
  const result=opened.result??opened.envelope?.result;
  assert.deepEqual(assertBootstrapEnvironment(result,selection),environmentManifest,'native public environment changed');
  const native=result.nativeResearch;
  assert.equal(native?.schema,'lycheedev.source-native-research.v1','native bootstrap schema');
  assert.deepEqual(native.environmentManifest,environmentManifest,'native worker environment changed');
  assert.equal(native.analysis?.engine,'native_common_facts','native bootstrap engine');
  assert.equal(native.analysis.sourceExecution,false,'bootstrap source must not execute');
  assert.equal(native.analysis.sourceFiles,2,'both authenticated Lua documents reach native worker');
  assert(native?.nativeSession?.workerPid>0,'public CLI did not open a native session');
  assert.equal(native.nativeSession.snapshotTransfer,'full');
  return {ownerPid:native.nativeSession.workerPid,selection,repository,commit,elapsedMs:performance.now()-started,opened};
}

/** Persistent, authenticated bridge. No public-CLI requests are made here, so
 * idle observations cannot accidentally refresh the server's idle deadline.
 * Methods are serial and every request gets immutable raw wire evidence.
 */
export async function createSessionPeer({binary,home,evidenceDir,ownerPid,timeoutMs=SESSION_LIMITS.quota}){
  assert.equal(process.platform,'win32','actual named-pipe acceptance requires Windows');
  assert(Number.isSafeInteger(ownerPid)&&ownerPid>0,'publicly opened owner PID required');
  assert(timeoutMs>0&&timeoutMs<=720_000);mkdirSync(evidenceDir,{recursive:true});
  const started=performance.now(),binarySHA256=sha256(boundedRead(binary,512<<20));
  const spec=join(evidenceDir,'peer-spec.json');writeNew(spec,pretty({binary:resolve(binary),home:resolve(home),
    evidenceDir:resolve(evidenceDir),binarySHA256,timeoutMs,expectedOwnerPid:ownerPid}));
  const executable=join(process.env.SystemRoot??'C:\\Windows','System32','WindowsPowerShell','v1.0','powershell.exe');
  const args=['-NoLogo','-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',join(HERE,'peer.ps1'),'-Spec',spec];
  const child=spawn(executable,args,{windowsHide:true,stdio:['pipe','pipe','pipe']});
  // One MiB is reserved separately for the complete helper stdout/stderr.
  const dataCap=SESSION_LIMITS.evidenceBytes-(1<<20);
  const queue=[];let waiter,failed,closed=false,pending=false,stdout='',outputBytes=0,sequence=0,traffic=0,evidenceBytes=statSync(spec).size,peerHandlesReaped=false;
  function saveData(path,bytes){const count=Buffer.byteLength(bytes);assert(evidenceBytes+count<=dataCap,'peer evidence total byte budget');
    writeNew(path,bytes);evidenceBytes+=count;}
  const exit=new Promise(done=>child.once('close',(code,signal)=>{closed=true;done({code,signal});
    if(waiter){waiter.reject(new Error(`peer exited: ${code}/${signal}`));waiter=null;}}));
  function fail(error){failed??=error;if(waiter){waiter.reject(failed);waiter=null;}if(!closed)child.kill();}
  async function reap(){
    let timer;
    try{
      const proof=await Promise.race([exit,new Promise((_,reject)=>{timer=setTimeout(()=>{
        const error=new Error('peer exit not established; outer Job retirement required');
        error.fatalProcessFailure=true;error.peerHandlesReaped=false;reject(error);
      },5000);})]);
      peerHandlesReaped=true;return {...proof,peerHandlesReaped:true,handleClosure:'kernel process exit'};
    }finally{clearTimeout(timer);}
  }
  child.once('error',fail);child.stdin.on('error',fail);
  child.stderr.on('data',chunk=>{outputBytes+=chunk.length;if(outputBytes>1<<20)return fail(new Error('peer output byte budget'));
    appendFileSync(join(evidenceDir,'peer.stderr.txt'),chunk);});
  child.stdout.on('data',chunk=>{
    outputBytes+=chunk.length;if(outputBytes>1<<20)return fail(new Error('peer output byte budget'));
    appendFileSync(join(evidenceDir,'peer.stdout.jsonl'),chunk);stdout+=chunk.toString('utf8');
    if(stdout.length>32768)return fail(new Error('peer line byte budget'));
    for(let newline;(newline=stdout.indexOf('\n'))>=0;){const line=stdout.slice(0,newline).trim();stdout=stdout.slice(newline+1);
      if(!line)continue;let value;try{value=JSON.parse(line);}catch(error){return fail(error);}
      if(value.kind==='failure')return fail(new Error(value.message));
      if(waiter){const current=waiter;waiter=null;current.resolve(value);}else queue.push(value);
      if(queue.length>2)return fail(new Error('unexpected peer output'));
    }
  });
  async function receive(timeout){
    if(failed)throw failed;if(queue.length)return queue.shift();assert(!waiter,'one pending bridge read');
    assert(!closed,'peer already exited');let timer;
    try{return await new Promise((resolve,reject)=>{waiter={resolve,reject};timer=setTimeout(()=>fail(new Error('peer response deadline')),
      Math.min(timeout,budget(started,timeoutMs)));});}finally{clearTimeout(timer);waiter=null;}
  }
  const watchdog=setTimeout(()=>fail(new Error('peer overall deadline')),timeoutMs);
  let ready,pipeName;
  try{
    ready=await receive(Math.min(20_000,timeoutMs));assert.equal(ready.kind,'ready');
    pipeName=nativePipeName(ready.canonicalHome,ready.logonSid);
    saveData(join(evidenceDir,'peer-identity.json'),pretty({binarySHA256,ownerPid,pipeName,ready,helperSHA256:sha256(readFileSync(join(HERE,'peer.ps1')))}));
  }catch(error){clearTimeout(watchdog);fail(error);const proof=await reap();
    saveData(join(evidenceDir,'peer-start-failure.json'),pretty({...proof,error:String(error)}));error.peerHandlesReaped=true;throw error;}
  async function command(action,fields={},caseId=action){
    assert(!pending,'peer operations must be serial');assert(!closed);pending=true;
    const id=++sequence;assert(id<=900);const output=join(evidenceDir,`${String(id).padStart(4,'0')}-${label(caseId)}.peer.json`);
    try{
      const outputBudgetBytes=Math.min(8<<20,dataCap-evidenceBytes-16384);assert(outputBudgetBytes>0,'peer evidence total byte budget');
      const line=JSON.stringify({sequence:id,action,output,outputBudgetBytes,...fields});assert(Buffer.byteLength(line)<32768);
      child.stdin.write(line+'\n');const reply=await receive(Math.min(budget(started,timeoutMs),(fields.timeoutMs??5000)+3000));
      assert.equal(reply.sequence,id);
      if(action==='finish'){assert.equal(reply.kind,'finished');assert.equal(reply.handlesClosed,true);return reply;}
      assert.equal(reply.kind,'result');assert.equal(resolve(reply.output),resolve(output));
      const bytes=boundedRead(output,outputBudgetBytes);evidenceBytes+=bytes.length;assert(evidenceBytes<=dataCap);
      return parsePeerJson(bytes.toString('utf8'));
    }finally{pending=false;}
  }
  let requestCount=0;
  async function exchange(caseId,request,{reject=false,timeout=10_000}={}){
    assert(++requestCount<=SESSION_LIMITS.requests);const prefix=`request-${String(requestCount).padStart(4,'0')}-${label(caseId)}`;
    const raw=typeof request==='string'?request:JSON.stringify(request),bytes=Buffer.from(raw);
    assert(bytes.length>0&&bytes.length<=SESSION_LIMITS.requestBytes);const requestPath=join(evidenceDir,prefix+'.request.json');saveData(requestPath,bytes);
    const observed=await command('exchange',{pipeName,requestPath,reject,timeoutMs:Math.min(timeout,budget(started,timeoutMs))},caseId);
    const row=observed.value;assert.equal(row.owner.pid,ownerPid);assert.equal(row.owner.sha256,binarySHA256);
    assert.equal(row.requestBytes,bytes.length);assert.equal(row.rejected,reject);
    saveData(join(evidenceDir,prefix+'.hello.json'),row.hello);
    const hello=parsePeerJson(row.hello);assert.equal(hello.pid,ownerPid);assert.equal(hello.created,row.owner.created);
    let response=null;
    if(!reject){assert.equal(Buffer.byteLength(row.response),row.responseBytes);assert(row.responseBytes<=SESSION_LIMITS.responseBytes);
      saveData(join(evidenceDir,prefix+'.response.json'),row.response);response=JSON.parse(row.response);}
    const diagnosticPath=join(home,'tmp','source-native-last-request.json');
    let diagnostic=null,diagnosticReadError=null;
    try{if(existsSync(diagnosticPath)){
      const bytes=boundedRead(diagnosticPath,1<<20);saveData(join(evidenceDir,prefix+'.diagnostic.json'),bytes);
      diagnostic=parsePeerJson(bytes.toString('utf8'));
    }}catch(error){diagnosticReadError=String(error);}
    const diagnosticObservation=classifyPhaseDiagnostic(diagnostic,request,hello);
    // The authenticated response remains the sole completion/error fact. No
    // owner-side synchronous "replying" sidecar is required before the next IPC.
    saveData(join(evidenceDir,prefix+'.diagnostic-observation.json'),pretty({...diagnosticObservation,readError:diagnosticReadError}));
    traffic+=row.requestBytes+row.responseBytes+Buffer.byteLength(row.hello)+8;assert(traffic<=SESSION_LIMITS.trafficBytes);
    return {response,owner:row.owner,hello,diagnostic,diagnosticObservation,startedElapsedMs:row.startedElapsedMs,finishedElapsedMs:row.finishedElapsedMs,
      evidencePrefix:prefix,requestBytes:row.requestBytes,responseBytes:row.responseBytes};
  }
  function envelope(action,work=null,cached=undefined){return {wire:SESSION_WIRE,deadline_unix_ms:Date.now()+10_000,action,work,...(cached?{cached}:{})};}
  async function status(caseId='status'){
    const row=await exchange(caseId,envelope('status'));assert.equal(row.response.error,null);const value=row.response.value;
    assert.equal(value.backend,'native');assert.equal(value.ownerPid,ownerPid);assert(['idle','ready'].includes(value.state));
    assert.equal(value.workerPid,value.state==='idle'?0:ownerPid);
    assert(value.state==='idle'?value.cachedSnapshot===null:/^[a-f0-9]{64}$/.test(value.cachedSnapshot));
    return {...row,status:value};
  }
  async function waitForExit(caseId,timeout){return (await command('wait',{timeoutMs:Math.min(timeout,budget(started,timeoutMs))},caseId));}
  async function close(caseId='close'){
    const row=await exchange(caseId,envelope('close'));assert.equal(row.response.error,null);assert.equal(row.response.value.state,'closed');
    const proof=await waitForExit(caseId+'-exit',5000);assert.equal(proof.value.exited,true);assert.equal(proof.value.exitCode,0);return {row,proof};
  }
  async function finish(){
    let failure;
    try{if(!closed&&!failed)await command('finish');else if(!closed)child.kill();}
    catch(error){failure=error;if(!closed)child.kill();}
    finally{child.stdin.end();clearTimeout(watchdog);}
    const proof=await reap();
    saveData(join(evidenceDir,'peer-exit.json'),pretty({...proof,requestCount,trafficBytes:traffic,evidenceBytes,logBytes:outputBytes,elapsedMs:performance.now()-started}));
    if(failure||proof.code!==0){const error=failure??failed??new Error('peer process failed');error.peerHandlesReaped=true;throw error;}
    return proof;
  }
  return {exchange,envelope,status,close,waitForExit,observe:async(caseId='observe')=>command('observe',{},caseId),finish,
    get stats(){return {requestCount,trafficBytes:traffic,evidenceBytes:evidenceBytes+outputBytes,elapsedMs:performance.now()-started,peerHandlesReaped};}};
}

function assertQuery(row,{batches,total,hit,transfer,retired=false}){
  assert.equal(row.response.error,null,JSON.stringify(row.response.error));const value=row.response.value,session=value.nativeSession;
  assert.equal(value.schema,'lycheedev.source-native-research.v1');assert.equal(session.workerPid,row.owner.pid);
  assert.equal(session.batches,total);assert.equal(session.admittedSemanticBatches,batches);assert.equal(session.analysisRetired,retired);
  assert.equal(session.cacheHit,hit);assert.equal(value.analysis.cacheHit,hit);assert.equal(session.snapshotTransfer,transfer);
  assert.equal(value.semanticExecution.receipt.planned_batches,batches);
  // Deliberately no load scope: this fixture verifies attempted/admitted work
  // still consumes quota when completed semantic work is zero.
  assert.equal(value.semanticExecution.receipt.completed_batches,0);
  return value;
}
async function quotaCases(peer,luac,emit){
  const proofs=[],snapshots={};
  const full=(name,snapshot,batches)=>peer.exchange(name,peer.envelope('query',{snapshot,query:tinyQuery(batches)}));
  const cached=(name,key,batches)=>peer.exchange(name,peer.envelope('query',null,{key,query:tinyQuery(batches)}));
  const check=(row,options)=>assertQuery(row,options);
  let snapshot=tinySnapshot('zero-one-two',luac);
  check(await full('zero-full',snapshot,0),{batches:0,total:0,hit:false,transfer:'full'});
  let status=await peer.status('zero-status'),key=status.status.cachedSnapshot;assert.equal(status.status.batches,0);
  for(let i=0;i<3;i++)check(await cached(`zero-reuse-${i}`,key,0),{batches:0,total:0,hit:true,transfer:'identity_only'});
  check(await cached('one-batch',key,1),{batches:1,total:1,hit:true,transfer:'identity_only'});
  check(await cached('two-batches',key,2),{batches:2,total:3,hit:true,transfer:'identity_only'});
  status=await peer.status('mixed-status');assert.equal(status.status.batches,3);proofs.push({case:'real-zero-one-two',passed:true});
  for(const scenario of ['exact-256','identity-255-plus-2','full-255-plus-2']){
    snapshot=tinySnapshot(scenario,luac);snapshots[scenario]=snapshot;
    check(await full(`${scenario}-first`,snapshot,1),{batches:1,total:1,hit:false,transfer:'full'});
    status=await peer.status(`${scenario}-key`);key=status.status.cachedSnapshot;assert.equal(status.status.batches,1);
    const last=scenario==='exact-256'?256:255;
    for(let total=2;total<=last;total++){
      check(await cached(`${scenario}-${total}`,key,1),{batches:1,total,hit:true,transfer:'identity_only',retired:total===256});
    }
    status=await peer.status(`${scenario}-boundary`);
    if(scenario==='exact-256'){
      assert.equal(status.status.state,'idle');assert.equal(status.status.batches,0);assert.equal(status.status.cachedSnapshot,null);
      const miss=await cached('exact-256-retired-miss',key,1);assert.equal(miss.response.value,null);assert.equal(miss.response.error.code,'source.native_snapshot_missing');
      check(await full('exact-256-full-resend',snapshot,1),{batches:1,total:1,hit:false,transfer:'full'});
    }else{
      assert.equal(status.status.state,'ready');assert.equal(status.status.batches,255);assert.equal(status.status.cachedSnapshot,key);
      if(scenario==='identity-255-plus-2'){
        const miss=await cached('identity-overquota-miss',key,2);assert.equal(miss.response.value,null);assert.equal(miss.response.error.code,'source.native_snapshot_missing');
        const retired=await peer.status('identity-overquota-retired');assert.equal(retired.status.state,'idle');assert.equal(retired.status.batches,0);
      }
      check(await full(`${scenario}-full`,snapshot,2),{batches:2,total:2,hit:false,transfer:'full'});
      check(await cached(`${scenario}-reused`,key,2),{batches:2,total:4,hit:true,transfer:'identity_only'});
    }
    proofs.push({case:scenario,passed:true,realOneBatchRequests:last,preloadedCounter:false});emit?.({group:'quota',case:scenario,state:'passed'});
  }
  return {proofs,scope:'real named-pipe admission and cache retirement; intentionally zero completed batches, no load scopes',
    preloadedCounters:false,fullOverflowRequests:1,internalReprepareCount:'not separately observable over the production wire'};
}

async function idleCase(peer){
  const admitted=await peer.status('idle-last-admitted'),started=performance.now(),proofs=[];
  for(const [offset,kind] of [[20_000,'zero-deadline'],[40_000,'unknown-field'],[55_000,'malformed-json']]){
    await sleep(Math.max(0,offset-(performance.now()-started)));
    const request=kind==='malformed-json'?'{"wire":':{...peer.envelope('status'),...(kind==='zero-deadline'?{deadline_unix_ms:0}:{unknown_field:true})};
    const row=await peer.exchange(`idle-invalid-${kind}`,request,{reject:true,timeout:3000});
    assert.equal(row.response,null);proofs.push({kind,atMs:performance.now()-started,evidencePrefix:row.evidencePrefix});
  }
  const exited=await peer.waitForExit('idle-owner-exit',15_000),idleMs=exited.elapsedMs-admitted.finishedElapsedMs;
  assert.equal(exited.value.exited,true,'invalid frames extended the 60s idle deadline');assert.equal(exited.value.exitCode,0);
  assert(idleMs>=58_000&&idleMs<=68_000,`idle retirement outside 60s observation window: ${idleMs}`);
  return {passed:true,idleMs,invalidFrames:proofs,exit:exited.value,admittedRequestsAfterAnchor:0};
}
async function lifetimeCase(peer,emit){
  const first=await peer.status('life-first'),initialAge=Date.now()-first.owner.creationUnixMs;
  assert(initialAge>=0&&initialAge<60_000,'lifetime owner must be freshly started');
  const start=performance.now(),heartbeats=[{ageMs:initialAge,evidencePrefix:first.evidencePrefix}];
  let last=first;
  for(let tick=1;tick<=31;tick++){
    const target=Math.min(initialAge+tick*20_000,590_000);
    await sleep(Math.max(0,target-initialAge-(performance.now()-start)));
    last=await peer.status(`life-heartbeat-${tick}`);
    const ageMs=initialAge+performance.now()-start;heartbeats.push({ageMs,evidencePrefix:last.evidencePrefix});
    emit?.({group:'lifetime',state:'running',ageMs,heartbeats:heartbeats.length});
    if(target===590_000)break;
  }
  const exited=await peer.waitForExit('life-fixed-owner-exit',35_000),sinceLastStatusMs=exited.elapsedMs-last.finishedElapsedMs;
  assert.equal(exited.value.exited,true,'status traffic extended the fixed 600s owner lifetime');assert.equal(exited.value.exitCode,0);
  assert(exited.value.lifeMs>=595_000&&exited.value.lifeMs<=620_000,`owner lifetime outside expected bound: ${exited.value.lifeMs}`);
  assert(sinceLastStatusMs>=0&&sinceLastStatusMs<59_000,'last status must rule out ordinary idle retirement');
  return {passed:true,heartbeats,sinceLastStatusMs,ownerLifeMs:exited.value.lifeMs,exit:exited.value};
}

/** group is quota, idle, or lifetime (session- prefix accepted). Returns a
 * report only after authenticated owner exit and bridge handle closure.
 * Pass startOwner() to reuse another public-CLI bootstrap. It must return
 * {ownerPid}; no owner is launched by the IPC bridge itself.
 */
export async function runSessionGroup({group,binary,home,release,evidenceDir,invoke,report,startOwner,ownerPid,timeoutMs,deadline}){
  group=group.replace(/^session-/,'');assert(Object.hasOwn(SESSION_LIMITS,group)&&['quota','idle','lifetime'].includes(group));
  assert.equal(process.platform,'win32','Windows IPC groups are not simulated on other platforms');
  const started=performance.now(),total=Math.floor(Math.min(timeoutMs??SESSION_LIMITS[group],
    deadline===undefined?Infinity:deadline-started-5000));assert(total>0&&total<=720_000);
  mkdirSync(evidenceDir,{recursive:true});let peer,primary,result,cleanup,bootstrap;
  const emit=typeof report==='function'?report:undefined;
  try{
    bootstrap=ownerPid?{ownerPid}:await (startOwner?startOwner():bootstrapTinyOwner({binary,home,release,evidenceDir,invoke,
      timeoutMs:Math.min(60_000,budget(started,total))}));
    const pid=bootstrap.ownerPid??bootstrap.result?.nativeResearch?.nativeSession?.workerPid;assert(pid>0,'bootstrap owner missing');
    if(group==='idle')assert(budget(started,total)>=75_000,'insufficient original budget for the real 60s idle observation');
    if(group==='lifetime')assert(budget(started,total)>=640_000,'insufficient original budget for the real 600s lifetime observation');
    peer=await createSessionPeer({binary,home,evidenceDir:join(evidenceDir,'ipc'),ownerPid:pid,timeoutMs:budget(started,total)});
    result=group==='quota'?await quotaCases(peer,releaseLuac(release),emit):group==='idle'?await idleCase(peer):await lifetimeCase(peer,emit);
    if(group==='quota')cleanup=await peer.close('quota-close');
  }catch(error){primary=error;}
  finally{
    if(peer)try{await peer.finish();}catch(error){
      if(error.fatalProcessFailure){if(primary)primary.fatalProcessFailure=true;else primary=error;}
      else primary??=error;
    }
    if(primary&&!primary.fatalProcessFailure){
      // Public close preserves the same authentication rules on a failed run.
      // Its result cannot turn the failed acceptance into a pass.
      try{cleanup=invoke('session-failed-close',['source','session','close'],{timeout:Math.min(10_000,Math.max(1000,total-(performance.now()-started))),phase:'cleanup'});}
      catch(error){cleanup={error:String(error)};}
    }
  }
  const answer={schema:'lycheedev.native-session-ipc-acceptance.v1',group,state:primary?'failed':'passed',
    scope:'real Windows named pipe, public CLI owner, owned outer Job',elapsedMs:performance.now()-started,
    bootstrap:bootstrap?{ownerPid:bootstrap.ownerPid,commit:bootstrap.commit,selection:bootstrap.selection}:null,
    limits:{...SESSION_LIMITS,totalMs:total},stats:peer?.stats,result,cleanup,
    peerHandlesReaped:peer?.stats.peerHandlesReaped??primary?.peerHandlesReaped??null,
    fatalProcessFailure:primary?.fatalProcessFailure===true,error:primary?.stack};
  writeNew(join(evidenceDir,'session-result.json'),pretty(answer));
  if(primary)throw primary;return answer;
}
