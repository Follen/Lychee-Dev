// Offline contract tests only. A pass here is not Windows IPC, quota, timing,
// compiler or kernel-handle evidence; those are emitted by runSessionGroup.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {spawnSync} from 'node:child_process';
import {mkdirSync,mkdtempSync,readFileSync,rmSync} from 'node:fs';
import {join} from 'node:path';
import {tmpdir} from 'node:os';
import {bootstrapApiSource,bootstrapEnvironmentInputSHA256,assertBootstrapEnvironment,BOOTSTRAP_API_PATH,bootstrapTinyOwner,acceptanceCliArgs,nativePipeName,parsePeerJson,tinyQuery,tinySnapshot,tinySource,fixtureSelection,SESSION_LIMITS,runSessionGroup} from './session-ipc.mjs';

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
test('PowerShell serializer peer names its framework XML assembly dependency',()=>{
  const ps=readFileSync(new URL('./peer.ps1',import.meta.url),'utf8');
  const references=/Add-Type -ReferencedAssemblies ([^\r\n]+) -TypeDefinition/.exec(ps)?.[1].split(',');
  assert(references, 'peer assembly list missing');
  for(const assembly of ['System.dll','System.Core.dll','System.Runtime.Serialization.dll','System.Xml.dll']) assert(references.includes(assembly),assembly);
  assert.equal(new Set(references).size,references.length);
  assert(ps.includes('DataContractJsonSerializer'));
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

test('all bootstrap public commands and flags exist in the fixed production contract',()=>{
  const contract=JSON.parse(readFileSync(new URL('../../crates/lycheedev-cli/src/commands.json',import.meta.url),'utf8'));
  const commands={init:['--home','--format'],'project init':['--product','--path','--format'],
    'source index':['--project','--home','--format'],
    'source refs':['--snapshot','--symbol','--direction','--limit','--static-only','--session-reuse','--release','--home','--format']};
  for(const [path,flags] of Object.entries(commands)){
    const command=contract.find(row=>row.path===path);assert(command,path);
    const allowed=new Set(command.flags.map(flag=>flag.split(' ')[0]));
    for(const flag of flags)assert(allowed.has(flag),`${path} ${flag}`);
  }
  assert(!contract.find(row=>row.path==='project init').flags.some(flag=>flag.startsWith('--home ')));
  const refs=contract.find(row=>row.path==='source refs');
  assert(refs.flags.includes('--direction <incoming|outgoing|both>'));
  assert(refs.flags.includes('--limit <1-200>'));
});

function environmentResult(selection){
  return {repository:'wow-ui-source',product:'retail',commit:selection.source.exactCommit,snapshot:selection.id,
    symbol:{kind:'declaration',name:'Caller',path:'a.lua',line:1,endLine:130},
    truncated:false,complete:false,coverage:{structural:'complete',semantic:'unavailable',environment:'ready',scannedFacts:1},
    environmentManifest:{schema:'lycheedev.source-environment.v1',identity:{repository:'wow-ui-source',
      commit:selection.source.exactCommit,client:'retail',generatorVersion:'test-observation-only'},
      inputSHA256:bootstrapEnvironmentInputSHA256(),definitionsSHA256:'d'.repeat(64),factsSHA256:'f'.repeat(64),documents:1}};
}
function nativeResult(selection){const result=environmentResult(selection);
  result.coverage.semantic='partial';
  result.nativeResearch={schema:'lycheedev.source-native-research.v1',environmentManifest:structuredClone(result.environmentManifest),
    analysis:{engine:'native_common_facts',sourceExecution:false,sourceFiles:2},nativeSession:{workerPid:42,snapshotTransfer:'full'}};
  return result;
}
test('bootstrap API is a distinct literal source document and matches the production input hash framing',()=>{
  const raw=bootstrapApiSource();
  assert.equal(BOOTSTRAP_API_PATH,'Interface/AddOns/Blizzard_APIDocumentationGenerated/NativeSessionDocumentation.lua');
  assert.equal(raw,'local API = {\n'+
    ' Name = "NativeSession", Type = "System", Namespace = "C_NativeSession",\n'+
    ' Functions = {{Name = "Value", Type = "Function", Returns = {{Name = "value", Type = "number"}}}},\n'+
    '}\nAPIDocumentation:AddDocumentationTable(API)\n');
  const input=createHash('sha256').update(`${Buffer.byteLength(BOOTSTRAP_API_PATH)}:${BOOTSTRAP_API_PATH}:${Buffer.byteLength(raw)}:`).update(raw).digest('hex');
  assert.equal(bootstrapEnvironmentInputSHA256(),input);
  const direct=tinySnapshot('exact-256',luac);
  assert.deepEqual(direct.files,{'a.lua':tinySource()});assert.deepEqual(direct.environment_documents,{});
  assert.deepEqual(direct.api,[]);assert.equal(direct.environment_manifest,null);assert.deepEqual(direct.load_documents,{});
});
test('environment result validation rejects absent, partial, foreign and unauthenticated inputs',()=>{
  const selection=fixtureSelection('1'.repeat(40));
  assert.deepEqual(assertBootstrapEnvironment(environmentResult(selection),selection),environmentResult(selection).environmentManifest);
  const mutations=[
    value=>{delete value.environmentManifest;},
    value=>{value.coverage.environment='partial';},
    value=>{value.coverage.structural='partial';},
    value=>{value.coverage.scannedFacts=2;},
    value=>{value.snapshot=fixtureSelection('2'.repeat(40)).id;},
    value=>{value.environmentManifest.identity.commit='2'.repeat(40);},
    value=>{value.environmentManifest.identity.repository='other';},
    value=>{value.environmentManifest.identity.client='classic';},
    value=>{value.environmentManifest.identity.generatorVersion='';},
    value=>{value.environmentManifest.documents=0;},
    value=>{value.environmentManifest.inputSHA256='0'.repeat(64);},
    value=>{value.environmentManifest.definitionsSHA256='not-a-digest';},
    value=>{value.environmentManifest.factsSHA256='not-a-digest';},
    value=>{value.symbol.path='other.lua';},
    value=>{value.symbol.name='Empty';},
    value=>{value.truncated=true;},
  ];
  for(const mutate of mutations){const result=environmentResult(selection);mutate(result);assert.throws(()=>assertBootstrapEnvironment(result,selection));}
});

// These tests execute real local Git operations and the JavaScript controller.
// CLI observations below are injected fixtures, not Rust/parser/Windows proof.
for (const scenario of ['valid','missing-snapshot','different-snapshot','missing-environment','partial-environment','wrong-environment-input',
  'preflight-opened-owner','native-mismatched-environment','native-missing-document','native-missing-owner']) {
  test(`bootstrap commits API input and fails closed through public preflight: ${scenario}`,async()=>{
    const root=mkdtempSync(join(tmpdir(),'lycheedev-bootstrap-contract-'));
    const home=join(root,'home'),release=join(root,'release'),evidenceDir=join(root,'evidence');
    const repository=join(home,'tmp','native-session-bootstrap'),calls=[];
    let selection;
    const invalidSnapshot=scenario.endsWith('-snapshot'),invalidEnvironment=['missing-environment','partial-environment','wrong-environment-input','preflight-opened-owner'].includes(scenario);
    const invoke=(id,args,options)=>{
      calls.push({id,args:acceptanceCliArgs(args,home,options),options});
      if(id==='session-bootstrap-init')mkdirSync(join(home,'mirrors'),{recursive:true});
      if(id==='session-bootstrap-index'){
        selection=JSON.parse(readFileSync(join(repository,'lycheedev.lock.json'),'utf8')).selection;
        assert.deepEqual(args,['source','index','--project',repository]);
        const snapshot=scenario==='missing-snapshot'?undefined:scenario==='different-snapshot'?'PIN-'+'f'.repeat(64):selection.id;
        return {result:{complete:true},envelope:{context:{snapshot}}};
      }
      if(id==='session-bootstrap-environment'){
        assert(!invalidSnapshot,'environment must not run after invalid index response');
        assert.deepEqual(args,['source','refs','--snapshot',selection.id,'--symbol','Caller',
          '--direction','incoming','--limit','25','--static-only']);
        const result=environmentResult(selection);
        if(scenario==='missing-environment')delete result.environmentManifest;
        if(scenario==='partial-environment')result.coverage.environment='partial';
        if(scenario==='wrong-environment-input')result.environmentManifest.inputSHA256='0'.repeat(64);
        if(scenario==='preflight-opened-owner')result.nativeResearch={nativeSession:{workerPid:42}};
        return {result};
      }
      if(id==='session-bootstrap-open'){
        assert(!invalidSnapshot&&!invalidEnvironment,'native owner must not open after failed preflight');
        assert.deepEqual(args,['source','refs','--snapshot',selection.id,'--symbol','Caller',
          '--direction','incoming','--limit','25','--session-reuse','--release',release]);
        const result=nativeResult(selection);
        if(scenario==='native-mismatched-environment')result.nativeResearch.environmentManifest.definitionsSHA256='0'.repeat(64);
        if(scenario==='native-missing-document')result.nativeResearch.analysis.sourceFiles=1;
        if(scenario==='native-missing-owner')delete result.nativeResearch.nativeSession;
        return {result};
      }
      return {result:{}};
    };
    try{
      const run=bootstrapTinyOwner({binary:'not-executed',home,release,evidenceDir,invoke});
      if(scenario==='valid'){
        const opened=await run;assert.equal(opened.ownerPid,42);assert.deepEqual(opened.selection,selection);
        const manifest=JSON.parse(readFileSync(join(evidenceDir,'bootstrap-environment.json'),'utf8'));
        assert.deepEqual(manifest.environmentManifest,environmentResult(selection).environmentManifest);
        assert.equal(manifest.sourceExecuted,false);
      }else await assert.rejects(run,/bootstrap|preflight|native worker environment changed|authenticated Lua|public source index|native session/);
      const ids=['session-bootstrap-init','session-bootstrap-project','session-bootstrap-index'];
      if(!invalidSnapshot)ids.push('session-bootstrap-environment');
      if(!invalidSnapshot&&!invalidEnvironment)ids.push('session-bootstrap-open');
      assert.deepEqual(calls.map(call=>call.id),ids);
      const fixture=JSON.parse(readFileSync(join(evidenceDir,'bootstrap-fixture.json'),'utf8'));
      assert.equal(fixture.sourceExecuted,false);assert.equal(fixture.files.length,3);
      const git=(...args)=>{const value=spawnSync('git',['--git-dir',fixture.mirror,...args],{encoding:'utf8',timeout:5000,maxBuffer:1<<20});assert.equal(value.status,0,value.stderr);return value.stdout;};
      assert.deepEqual(git('ls-tree','-r','--name-only',fixture.commit).trim().split('\n').sort(),['a.lua','Main.toc',BOOTSTRAP_API_PATH].sort());
      for(const row of fixture.files){const raw=git('show',`${fixture.commit}:${row.path}`);assert.equal(sha(raw),row.sha256);assert.equal(Buffer.byteLength(raw),row.bytes);}
      assert.equal(git('show',`${fixture.commit}:${BOOTSTRAP_API_PATH}`),bootstrapApiSource());
      assert.equal(readFileSync(join(evidenceDir,'bootstrap-api.lua'),'utf8'),bootstrapApiSource());
      for(const call of calls){
        assert.equal(call.options.phase,'local');assert(call.options.timeout>0&&call.options.timeout<=45_000);
        assert.deepEqual(call.args.slice(-2),['--format','json']);
        if(call.id==='session-bootstrap-project')assert(!call.args.includes('--home'));
        else assert.deepEqual(call.args.slice(-4),['--home',home,'--format','json']);
      }
    }finally{rmSync(root,{recursive:true,force:true});}
  });
}
