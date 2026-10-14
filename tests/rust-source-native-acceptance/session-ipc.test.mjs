// Offline contract tests only. A pass here is not Windows IPC, quota, timing,
// compiler or kernel-handle evidence; those are emitted by runSessionGroup.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync} from 'node:fs';
import {acceptanceCliArgs,nativePipeName,parsePeerJson,tinyQuery,tinySnapshot,tinySource,fixtureSelection,SESSION_LIMITS,runSessionGroup} from './session-ipc.mjs';

const sha=value=>createHash('sha256').update(value).digest('hex');
const luac={version:'5.1.5',platform:'windows-amd64',executable_sha256:'a'.repeat(64),executable_bytes:1};

test('FILETIME protocol and diagnostic identities retain every digit',()=>{
  const raw='{"pid":42,"created":134049006123456789,"workerCreated":18446744073709551615,"nested":{"created":134049006123456788},"other":123}';
  const parsed=parsePeerJson(raw);
  assert.equal(parsed.created,'134049006123456789');assert.equal(parsed.workerCreated,'18446744073709551615');
  assert.equal(parsed.nested.created,'134049006123456788');assert.equal(parsed.other,123);
  assert.notEqual(parsed.created,parsed.nested.created);
});
test('scope is canonical-home lowercase, NUL, logon SID SHA256 prefix',()=>{
  const sid='S-1-5-5-123-456',name=nativePipeName('\\\\?\\C:\\Fixture\\HOME',sid);
  assert.equal(name,'lycheedev-native-source-'+sha('c:\\fixture\\home\0'+sid).slice(0,32));
  assert.equal(name,nativePipeName('c:\\fixture\\home',sid));
  assert.notEqual(name,nativePipeName('c:\\fixture\\home','S-1-5-5-123-457'));
  assert.throws(()=>nativePipeName('c:\\fixture','S-1-5-21-123'));
});
test('tiny material contains exactly 128 real outgoing calls and a separate empty function',()=>{
  const source=tinySource(),lines=source.split('\n');
  assert.equal(lines[0],'function Caller()');assert.equal(lines[129],'end');
  assert.equal(lines[130],'function Empty()');assert.equal(lines[131],'end');
  assert.equal((source.match(/ missing\d+\(\)/g)??[]).length,128);
  for(let i=0;i<128;i++)assert.equal(lines[i+1],` missing${i}()`);
});
test('zero, one and two batch selectors match authenticated syntax positions',()=>{
  assert.deepEqual(tinyQuery(0),{symbol:{kind:'declaration',name:'Empty',category:'function',confidence:'',path:'a.lua',line:131,endLine:132},direction:'outgoing'});
  assert.equal(tinyQuery(1).direction,'incoming');assert.equal(tinyQuery(2).direction,'both');
  assert.equal(tinyQuery(1).symbol.name,'Caller');assert.equal(tinyQuery(1).symbol.endLine,130);
  assert.throws(()=>tinyQuery(3));
});
test('wire fixture has the production SnapshotRequest shape and no injected counter',()=>{
  const snapshot=tinySnapshot('exact-256',luac);
  assert.deepEqual(Object.keys(snapshot),['files','load_documents','environment_documents','source','luac','api','environment_manifest','load_plans','input_gaps']);
  assert.deepEqual(snapshot.load_plans,[]);assert.deepEqual(snapshot.luac,luac);
  assert.equal(snapshot.files['a.lua'],tinySource());
  assert(!JSON.stringify(snapshot).includes('batches'));
  assert.notEqual(JSON.stringify(snapshot),JSON.stringify(tinySnapshot('full-255-plus-2',luac)));
});
test('project fixture selection identity uses exact production struct ordering',()=>{
  const commit='1'.repeat(40),pin=fixtureSelection(commit);
  const unsigned={schema:'lycheedev.selection.v1',id:'',source:{repository:'wow-ui-source',product:'retail',requestedRef:'refs/heads/native-acceptance',exactCommit:commit,parserRevision:'source-v1'}};
  assert.equal(pin.id,'PIN-'+sha(JSON.stringify(unsigned)));assert.equal(pin.source.exactCommit,commit);
  assert.throws(()=>fixtureSelection('main'));
});
test('fixed acceptance limits include request, response, total traffic and real long lifetimes',()=>{
  assert(SESSION_LIMITS.requests>=789&&SESSION_LIMITS.requests<=850);
  assert.equal(SESSION_LIMITS.requestBytes,2<<20);assert.equal(SESSION_LIMITS.responseBytes,2<<20);
  assert.equal(SESSION_LIMITS.trafficBytes,64<<20);assert.equal(SESSION_LIMITS.evidenceBytes,192<<20);
  assert(SESSION_LIMITS.lifetime>=620_000&&SESSION_LIMITS.lifetime<=720_000);
});
test('bridge source binds pipe handle PID and held process creation/image to exact Hello digest',()=>{
  const ps=readFileSync(new URL('./peer.ps1',import.meta.url),'utf8');
  for(const text of ['GetNamedPipeServerProcessId(pipe.SafePipeHandle.DangerousGetHandle()',
    'OpenProcess(0x00101000,false,pid)','GetProcessTimes(handle','QueryFullProcessImageNameW(handle',
    'hello.created.ToString()!=identity.created','hello.executable!=digest','ownerHandle=process;process=IntPtr.Zero',
    'FileShare.Read','WaitForSingleObject(ownerHandle','peer.owner_generation_changed'])assert(ps.includes(text),text);
  assert(!ps.includes('TerminateProcess'));assert(!ps.includes('CreateProcess'));
});
test('quota path has real requests for all cumulative boundaries and distinct miss/full paths',()=>{
  const js=readFileSync(new URL('./session-ipc.mjs',import.meta.url),'utf8');
  assert(js.includes('for(let total=2;total<=last;total++)'));
  for(const name of ['exact-256','identity-255-plus-2','full-255-plus-2','identity-overquota-retired','exact-256-full-resend'])assert(js.includes(name));
  assert(js.includes('completed_batches,0'));assert(js.includes('preloadedCounters:false'));
  assert(js.includes('internalReprepareCount:\'not separately observable over the production wire\''));
});
test('non-Windows entry fails before any mock could be reported as acceptance',{skip:process.platform==='win32'},async()=>{
  await assert.rejects(runSessionGroup({group:'quota'}),/Windows IPC groups are not simulated/);
});

test('bootstrap project uses fixed path-local CLI contract without home while source keeps isolated home',()=>{
  const args=['project','init','--path','C:\\isolated\\project','--product','retail'];
  assert.deepEqual(acceptanceCliArgs(args,'C:\\isolated\\home',{useHome:false}),[...args,'--format','json']);
  assert.deepEqual(acceptanceCliArgs(['source','session','status'],'C:\\isolated\\home'),
    ['source','session','status','--home','C:\\isolated\\home','--format','json']);
  assert.throws(()=>acceptanceCliArgs(['source','refs'],'ignored',{useHome:false}),/only project init/);
  const controller=readFileSync(new URL('./run.mjs',import.meta.url),'utf8');
  assert(controller.includes('acceptanceCliArgs(args, home, options)'));
  const bootstrap=readFileSync(new URL('./session-ipc.mjs',import.meta.url),'utf8');
  assert(bootstrap.includes("'--product','retail'],{useHome:false})"));
});
