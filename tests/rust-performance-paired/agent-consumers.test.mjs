import {agentEnvironment,preparationTimeout} from './agent-environment.mjs';
import {freezeDecoder,verifyDecoder,loadDecoder} from './agent-decoder.mjs';
import {fileIdentity,resolvedTool,verifyTool,cargoAdmission} from './agent-tool-identity.mjs';
// Declarations only until the full-migration test gate is opened.
import test from 'node:test';
import assert from 'node:assert/strict';
import {existsSync,mkdtempSync,mkdirSync,readFileSync,writeFileSync,rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join,toNamespacedPath} from 'node:path';
import {parse,safeNumericToken} from './agent-json.mjs';
import {digest,fixturePreparationRecipe} from './agent-fixture-inputs.mjs';
import {fixtureArgs,selectedFixturePreparers} from './agent-prepare-fixtures.mjs';
import {auditTranscript,queryFor,rowsFor,columnsFor,generationProof} from './agent-audit.mjs';
import {sampleHome} from './agent-report.mjs';
import {businessElapsed,comparison,groups,requiredFixtureKinds,selection} from './agent-policy.mjs';
import {authenticate,openArchive,archiveSample} from './agent-custody.mjs';
test('current numeric parser requires source context and rejects silent widening or narrowing',()=>{assert.throws(()=>safeNumericToken(1,undefined));assert.throws(()=>parse('{"value":9007199254740993}'));assert.throws(()=>parse('{"value":1.5000000000000001}'));assert.deepEqual(parse('{"value":1.5,"integer":4294967295}'),{value:1.5,integer:4294967295});});
function packet(format){const rows=rowsFor('catalog'),columns=columnsFor('catalog'),page={columns,rows,offset:0,totalRows:rows.length,complete:true},query=queryFor('catalog'),result={result:{handle:'HANDLE-test',rows:3,retainedBytes:1024,expiresAfterMs:300000},complete:true},capture={id:'CAP-test'},quote=v=>'"'+String(v).replaceAll('"','""')+'"',text=[columns,...rows].map(r=>r.map(quote).join(',')+'\r\n').join(''),exports=format==='csv'?[{format:'csv',text,page}]:[],tasks=[{task:'query',query},format==='csv'?{task:'export',handle:'HANDLE-test',offset:0,limit:1000,format:'csv'}:{task:'page',handle:'HANDLE-test',offset:0,limit:1000},{task:'capture',handle:'HANDLE-test'},{task:'release',handle:'HANDLE-test'},{task:'close'}],results=[result,exports[0]??page,capture,{released:'HANDLE-test'},{closed:true,handlesReleased:true}];return{schema:'lycheedev.paired-agent-result.recovered.v1',query,result,capture,pages:[page],exports,child:{code:0,signal:null},transcript:tasks.map((task,i)=>{const request={protocol:'lycheedev.data-agent.v1',id:i+1,timeout_ms:30000,...task},frame={protocol:request.protocol,id:request.id,ok:true,result:results[i]};return{request,frame,raw:Buffer.from(JSON.stringify(frame)).toString('base64')};})};}
test('CSV admission requires actual export tasks not equivalent pages',()=>{assert.throws(()=>auditTranscript(packet('rows'),'catalog','csv'));assert.doesNotThrow(()=>auditTranscript(packet('csv'),'catalog','csv'));});
test('CSV replay rejects altered CRLF, quoting, cell, and raw frame',()=>{for(const change of [p=>p.exports[0].text=p.exports[0].text.replaceAll('\r\n','\n'),p=>p.exports[0].text=p.exports[0].text.replace('"ID"','ID'),p=>p.pages[0].rows[0][1]='wrong',p=>p.transcript[0].raw=Buffer.from('{}').toString('base64')]){const p=packet('csv');change(p);assert.throws(()=>auditTranscript(p,'catalog','csv'));}});
test('correctness-only Data request never evaluates performance acceptance',()=>{const rows=Array.from({length:20},()=>({go:{cold:{controllableMillis:10},warm:{controllableMillis:10}},rust:{cold:{controllableMillis:1},warm:{controllableMillis:1}}}));assert(groups(rows,{acceptance:true,mode:'data'},{id:'correctness',performance:false}).every(g=>!g.performanceAcceptanceEvaluated&&g.state==='equivalent_correctness_only'));});
test('current Agent comparisons identify Go324 without changing CSV acceptance',()=>{
 assert.equal(comparison({mode:'data'},{performance:true}).baseline,'authenticated Go324 production workload');
 const csv=comparison({mode:'csv'},{});assert.equal(csv.baseline,'authenticated Go324 SQL JSON and complete query CAP');assert.equal(csv.performanceAcceptanceEligible,false);
});
test('business timing retains the controller total and includes the complete Agent spawn-to-close interval',()=>{
 const proof={schema:'lycheedev.owned-process.v1',rootReaped:true,treeReaped:true,handlesClosed:true,toolIdentityLocksClosed:true,outputReadersComplete:true,activeProcesses:0,forced:false,timing:{schema:'lycheedev.owned-timing.v1',frequency:1000000,startTicks:'1000000',rootExitTicks:'1500000',treeReapedTicks:'1600000'}};
 const direct=businessElapsed(proof);
 assert.equal(direct.controllableMillis,600);assert.equal(direct.controllerTiming.controllableMillis,600);
 const packet={child:{code:0,signal:null},businessTiming:{schema:'lycheedev.agent-business-timing.v1',startNs:'1000000000',endNs:'1200000000'}};
 const agent=businessElapsed(proof,packet);
 assert.equal(agent.controllableMillis,200);assert.deepEqual(agent.controllerTiming,direct.controllerTiming);
 assert.deepEqual(agent.businessTiming,packet.businessTiming);
 for(const mutate of [p=>delete p.businessTiming,p=>p.businessTiming.endNs=p.businessTiming.startNs,p=>p.businessTiming.endNs='1700000000',p=>p.child.code=1]){
  const changed=structuredClone(packet);mutate(changed);assert.throws(()=>businessElapsed(proof,changed));
 }
 assert.throws(()=>businessElapsed({...proof,treeReaped:false},packet));
});
test('generation hit alone cannot prove physical Arrow use',()=>{const cache={status:'hit',generation:'a'.repeat(64),object:{sha256:'b'.repeat(64),bytes:128},reused:true,publicationThisCommand:false,representationSelection:'authenticated_arrow_session_snapshot',representationReason:'complete_native_ipc'},expected={phase:'first-after-prepare',generation:cache.generation,object:cache.object},sessionSnapshot={reused:false,sourceReadingScope:'current_request',coverage:'whole_physical_source',requestControl:'fresh_request_not_cached_deadline',representation:'native_arrow_with_physical_id_index'};assert.doesNotThrow(()=>generationProof({sessionSnapshot,typedGenerationCache:cache},expected));assert.throws(()=>generationProof({sessionSnapshot,typedGenerationCache:{...cache,representationSelection:'authenticated_wdc_demand_stream',representationReason:'demand_work_boundary'}},expected));assert.throws(()=>generationProof({sessionSnapshot,typedGenerationCache:{...cache,reused:false}},expected));assert.throws(()=>generationProof({sessionSnapshot:{...sessionSnapshot,coverage:'resolved_physical_id_set'},typedGenerationCache:cache},expected));});
function custody(){const root=mkdtempSync(join(tmpdir(),'paired-custody-declaration-')),id='11111111-1111-4111-8111-111111111111',stem=join(root,'process-'+id),receipt=join(root,'receipt.json'),binary='fixture-only.exe',resolvedBinary=join(root,binary),args=['fixture'],inputs={'tools/rust-owned-process.ps1':'f'.repeat(64)},q={schema:'lycheedev.owned-request.v2',runId:id,coordinatorPid:10,command:binary,args,cwd:root,timeoutMs:1000,orphanMs:30000,cargoBuild:false,msvcCompile:false,identity:stem+'.identity.json',rootIdentity:stem+'.root.json',receipt:stem+'.receipt.json',stdout:stem+'.stdout.bin',stderr:stem+'.stderr.bin'},request=stem+'.request.json',raw=Buffer.from(JSON.stringify(q)),proof={schema:'lycheedev.owned-process.v1',runId:id,binary:resolvedBinary,pid:11,creationFileTime:'100',startupIdentity:q.identity,rootIdentity:q.rootIdentity,exitCode:0,forced:false,knownAuxiliaryCleanup:false,rootReaped:true,treeReaped:true,handlesClosed:true,toolIdentityLocksClosed:true,outputReadersComplete:true,activeProcesses:0,timing:{frequency:1000,startTicks:'100',treeReapedTicks:'200'}},start={schema:'lycheedev.owned-start.v1',wrapperPid:12,wrapperCreationFileTime:'99',identitySource:'request-v2',runId:id,requestPath:request,coordinatorPid:10,rootIdentity:q.rootIdentity,state:'wrapper-started',requestSHA256:digest(raw),wrapperObservedSHA256:inputs['tools/rust-owned-process.ps1'],rootAssignment:'PROC_THREAD_ATTRIBUTE_JOB_LIST',historicalReapProof:false},job={schema:'lycheedev.owned-job.v1',runId:id,requestSHA256:digest(raw),wrapperPid:12,wrapperCreationFileTime:'99'},native={schema:'lycheedev.owned-root.v1',runId:id,requestSHA256:digest(raw),wrapperPid:12,wrapperCreationFileTime:'99',jobAssignedAtCreation:true,historicalReapProof:false,pid:11,creationFileTime:'100'},data=[[request,raw],[request+'.wrapper.json',JSON.stringify({status:0})],[q.identity,JSON.stringify(start)],[stem+'.job.json',JSON.stringify(job)],[q.rootIdentity,JSON.stringify(native)],[q.stdout,'{}'],[q.stderr,''],[q.receipt,JSON.stringify(proof)]];writeFileSync(resolvedBinary,'synthetic identity bytes, never executable');for(const[p,b]of data)writeFileSync(p,b);const logs=data.map(([path,b])=>({path,original:path,sha256:digest(b)})),sample={binary,args,exit:0,handlesReaped:true,proof,logs:data.map(([p])=>p),stdoutSHA256:digest('{}'),stderrSHA256:digest('')},options={resolvedIdentity:fileIdentity(resolvedBinary),owner:{sealedLogs:logs},receipt,root,binary,args,inputs,ids:new Set(),outer:{pid:10,timing:{frequency:1000,startTicks:'1',treeReapedTicks:'300'}},measured:false};return{root,sample,options};}
test('current custody rejects borrowed command, duplicate invocation and missing native leaf',()=>{const f=custody();try{authenticate(f.sample,f.options);assert.throws(()=>authenticate(f.sample,f.options));assert.throws(()=>authenticate(f.sample,{...f.options,ids:new Set(),binary:'other.exe'}));assert.throws(()=>authenticate(f.sample,{...f.options,ids:new Set(),owner:{sealedLogs:f.options.owner.sealedLogs.filter(l=>!l.path.endsWith('.root.json'))}}));}finally{rmSync(f.root,{recursive:true,force:true});}});
test('current custody requires the sealed Job record and its original wrapper binding',()=>{
 const f=custody();try{
  const path=f.sample.logs.find(path=>path.endsWith('.job.json'));
  assert.throws(()=>authenticate(f.sample,{...f.options,ids:new Set(),owner:{sealedLogs:f.options.owner.sealedLogs.filter(row=>row.path!==path)}}),/unsealed path/);
  const job=parse(readFileSync(path));job.wrapperPid=0;job.wrapperCreationFileTime='';
  const bytes=JSON.stringify(job);writeFileSync(path,bytes);f.options.owner.sealedLogs.find(row=>row.path===path).sha256=digest(bytes);
  assert.throws(()=>authenticate(f.sample,{...f.options,ids:new Set()}));
 }finally{rmSync(f.root,{recursive:true,force:true});}
});

test('archived CAP bytes require owner-sealed chunk identity, not metadata booleans',()=>{const f=custody();try{const path=join(f.root,'archive.json'),doc={schema:'lycheedev.paired-archive.recovered.v1',sources:[],captures:[{metadata:{id:'CAP-fake',blob:{sha256:digest('{}'),bytes:2}},body:{sha256:digest('{}'),bytes:2,chunks:[{path:join(f.root,'unsealed.bin'),sha256:digest('{}'),bytes:2}]}}]},bytes=Buffer.from(JSON.stringify(doc));writeFileSync(path,bytes);writeFileSync(join(f.root,'unsealed.bin'),'{}');const owner={sealedLogs:[...f.options.owner.sealedLogs,{path,original:path,sha256:digest(bytes)}]};assert.throws(()=>openArchive(owner,f.options.receipt,{path,sha256:digest(bytes),bytes:bytes.length}));}finally{rmSync(f.root,{recursive:true,force:true});}});

test('cold/warm cannot borrow another pair or opposite engine home',()=>{const owner={directory:join(tmpdir(),'explicit-fake-pair')},go=join(owner.directory,'go-workflow');assert.equal(sampleHome(owner,'go','cold',go),sampleHome(owner,'go','warm',go));assert.throws(()=>sampleHome(owner,'rust','warm',go));assert.throws(()=>sampleHome({directory:join(tmpdir(),'different-fake-pair')},'go','cold',go));assert.throws(()=>sampleHome(owner,'go','fresh-wdc',go));});

test('content inspection archives content and Root when source is the installation label',t=>{
 const home=mkdtempSync(join(tmpdir(),'paired-content-archive-'));t.after(()=>rmSync(home,{recursive:true,force:true}));
 const evidence=join(home,'evidence');mkdirSync(evidence);
 const references=[Buffer.from('content bytes'),Buffer.from('root bytes')].map(bytes=>{
  const sha256=digest(bytes),directory=join(home,'blobs',sha256.slice(0,2));mkdirSync(directory,{recursive:true});writeFileSync(join(directory,sha256.slice(2)),bytes);return{sha256,bytes:bytes.length};
 });
 const archive=archiveSample({home,envelope:{captures:[],result:{source:'installation',content:references[0],root:references[1]}},engine:'go',evidence,label:'content',metadata:()=>assert.fail('no CAP in this fixture')});
 assert.deepEqual(parse(readFileSync(archive.path)).sources.map(row=>row.blob),references);
});

test('bare request and resolved native identity remain distinct and byte-bound',()=>{const f=custody();try{assert.notEqual(f.sample.binary,f.sample.proof.binary);authenticate(f.sample,f.options);assert.throws(()=>authenticate(f.sample,{...f.options,ids:new Set(),resolvedIdentity:undefined}));writeFileSync(f.options.resolvedIdentity.path,'changed');assert.throws(()=>verifyTool(f.options.resolvedIdentity));}finally{rmSync(f.root,{recursive:true,force:true});}});

test('fixture execute timeout consumes one shared group allowance',()=>{assert.equal(preparationTimeout(1000,301000,600000),300000);assert.equal(preparationTimeout(1000,500001,600000),100999);assert.equal(preparationTimeout(1000,301000,900000),300000);assert.equal(preparationTimeout(1000,301000,120000),120000);assert.throws(()=>preparationTimeout(1000,600001,900000));});
test('Windows native tool path casing retains the same authenticated file',{skip:process.platform!=='win32'},()=>{
 const f=custody();try{
  f.sample.proof.binary=f.sample.proof.binary.toUpperCase();
  const path=f.sample.logs.at(-1),bytes=JSON.stringify(f.sample.proof);writeFileSync(path,bytes);
  f.options.owner.sealedLogs.find(row=>row.path===path).sha256=digest(bytes);
  authenticate(f.sample,f.options);
 }finally{rmSync(f.root,{recursive:true,force:true});}
});
test('Windows PATHEXT casing resolves to the same actual executable identity',{skip:process.platform!=='win32'},t=>{
 const root=mkdtempSync(join(tmpdir(),'paired-tool-case-'));t.after(()=>rmSync(root,{recursive:true,force:true}));
 const executable=join(root,'cargo.exe');writeFileSync(executable,'synthetic executable bytes; never launched');
 assert.deepEqual(resolvedTool('cargo',{PATH:root,PATHEXT:'.EXE'}),fileIdentity(executable));
});
test('paired child environments isolate users while keeping explicit business homes and fixtures independent',t=>{
 const root=mkdtempSync(join(tmpdir(),'paired-environment-'));t.after(()=>rmSync(root,{recursive:true,force:true}));
 const scratch=join(root,'controller'),businessHome=join(root,'pair/rust-workflow'),fixtureHome=join(root,'fixture/home');
 const parent={PATH:'unchanged-tools',HOME:'real-user',userprofile:'real-user',Codex_Home:'real-agent',APPDATA:'real-apps',LOCALAPPDATA:'real-local',TEMP:'real-temp',TMP:'real-temp',LYCHEEDEV_BIN:'real-tool',lycheedev_home:'real-product',CARGO_HOME:join(root,'compiler-cache'),RUSTUP_HOME:join(root,'toolchains'),LYCHEEDEV_AGENT_IMPORT:join(root,'import.json')};
 const before={...parent},env=agentEnvironment(root,parent,scratch);
 for(const key of ['HOME','USERPROFILE','CODEX_HOME','APPDATA','LOCALAPPDATA','TEMP','TMP','LYCHEEDEV_HOME'])assert(env[key].startsWith(scratch),key);
 for(const key of ['HOME','APPDATA','LOCALAPPDATA','TEMP'])assert(existsSync(env[key]));
 assert.equal(env.userprofile,undefined);assert.equal(env.Codex_Home,undefined);assert.equal(env.lycheedev_home,undefined);assert.equal(env.LYCHEEDEV_BIN,undefined);
 for(const key of ['PATH','CARGO_HOME','RUSTUP_HOME','LYCHEEDEV_AGENT_IMPORT'])assert.equal(env[key],parent[key]);
 assert.notEqual(env.LYCHEEDEV_HOME,businessHome);assert.notEqual(env.LYCHEEDEV_HOME,fixtureHome);
 assert(!existsSync(businessHome));assert(!existsSync(fixtureHome));assert.deepEqual(parent,before);
 assert.throws(()=>agentEnvironment(root,{...parent,LYCHEEDEV_DISK_SCRATCH_GIB:'6'},scratch),/disk overrides forbidden/);
 assert.throws(()=>agentEnvironment(root,{...parent,RUST_MIN_STACK:'9999999'},scratch),/default compiler\/target\/stack required/);
});
test('current paired preparation and controller use the existing isolated owners',()=>{
 const source=name=>readFileSync(new URL(name,import.meta.url),'utf8');
 assert(/oneShot: false, isolateUser: true, environment/.test(source('./agent-prepare-fixtures.mjs')));
 assert(/oneShot: false, isolateUser: true, onSuccess/.test(source('./agent-importer.mjs')));
 assert(/const env=agentEnvironment\(root,environment,job\.directory\)/.test(source('./agent-paired-owned.mjs')));
 assert(/const env=agentEnvironment\(root,process\.env,c\.scratch\)/.test(source('./agent-paired-run.mjs')));
});
test('decoder package and native byte mutations invalidate the complete closure',()=>{const root=mkdtempSync(join(tmpdir(),'paired-decoder-declaration-'));try{const dir=join(root,'node_modules','sharp');mkdirSync(dir,{recursive:true});const entry=join(dir,'index.js'),native=join(dir,'vendor.node');writeFileSync(join(dir,'package.json'),JSON.stringify({name:'sharp',version:'fixture',main:'index.js'}));writeFileSync(entry,'module.exports = null; // never imported by this declaration');writeFileSync(native,'synthetic native bytes');const identity=freezeDecoder(entry);verifyDecoder(identity);writeFileSync(native,'mutated');assert.throws(()=>verifyDecoder(identity));}finally{rmSync(root,{recursive:true,force:true});}});

test('Windows decoder native paths use the same file identity with extended prefixes',{skip:process.platform!=='win32'},t=>{
 const root=mkdtempSync(join(tmpdir(),'paired-decoder-namespace-'));t.after(()=>rmSync(root,{recursive:true,force:true}));
 const dir=join(root,'node_modules','sharp');mkdirSync(dir,{recursive:true});
 const entry=join(dir,'index.js'),native=join(dir,'vendor.node'),outside=join(root,'outside.node');
 writeFileSync(join(dir,'package.json'),JSON.stringify({name:'sharp',version:'fixture',main:'index.js'}));
 writeFileSync(entry,"module.exports = {cache(){}, versions:{sharp:'fixture'}};");writeFileSync(native,'fixture native identity');writeFileSync(outside,'outside native identity');
 let calls=0,reported=toNamespacedPath(native);t.mock.method(process.report,'getReport',()=>({sharedObjects:calls++===0?[]:[reported]}));
 const decoder=loadDecoder(freezeDecoder(entry));assert.deepEqual(decoder.finish().native,[fileIdentity(native)]);
 reported=toNamespacedPath(outside);assert.throws(()=>decoder.finish(),/outside frozen closure/);
});

test('known Cargo auxiliary cleanup requires policy, held identities and reviewed descendants',()=>{const root=mkdtempSync(join(tmpdir(),'paired-cargo-declaration-')),envKey=Object.keys(process.env).find(k=>k.toLowerCase()==='programfiles')??'ProgramFiles',previous=process.env[envKey];try{process.env[envKey]=root;mkdirSync(join(root,'tools'));const cargo=join(root,'cargo.exe'),helper=join(root,'vctip.exe');writeFileSync(cargo,'synthetic cargo identity');writeFileSync(helper,'synthetic helper identity');const identity=fileIdentity(cargo),h=fileIdentity(helper),entry={programFilesPath:'vctip.exe',bytes:h.bytes,sha256:h.sha256,kind:'msvc-vctip',fileVersion:'fixture',signerSubject:'fixture',signerThumbprint:'fixture'},policy={schema:'lycheedev.build-auxiliary-policy.v1',cargoSHA256:identity.sha256,helpers:[entry]},raw=JSON.stringify(policy);writeFileSync(join(root,'tools/rust-build-auxiliary.json'),raw);const admitted={...h,kind:entry.kind,fileVersion:entry.fileVersion,signerSubject:entry.signerSubject,signerThumbprint:entry.signerThumbprint,signatureStatus:'WinVerifyTrust:0'},proof={forced:true,knownAuxiliaryCleanup:true,creationFileTime:'100',helperAdmission:{state:'enabled',cargo:identity,helpers:[admitted]},auxiliaryProcesses:[{pid:42,creationFileTime:'101',image:helper,jobMember:true,reviewedAuxiliary:true}]},options={root,policySHA256:digest(raw),identity,allowCleanup:true};assert.doesNotThrow(()=>cargoAdmission(proof,options));assert.throws(()=>cargoAdmission(proof,{...options,allowCleanup:false}));assert.throws(()=>cargoAdmission({...proof,auxiliaryProcesses:[{...proof.auxiliaryProcesses[0],reviewedAuxiliary:false}]},options));writeFileSync(helper,'changed');assert.throws(()=>cargoAdmission(proof,options));}finally{if(previous===undefined)delete process.env[envKey];else process.env[envKey]=previous;rmSync(root,{recursive:true,force:true});}});

test('ordinary successful Cargo does not require auxiliary-policy Cargo approval',()=>{const root=mkdtempSync(join(tmpdir(),'paired-cargo-disabled-declaration-'));try{mkdirSync(join(root,'tools'));const cargo=join(root,'cargo.exe');writeFileSync(cargo,'synthetic current Cargo, not policy helper Cargo');const identity=fileIdentity(cargo),raw=JSON.stringify({schema:'lycheedev.build-auxiliary-policy.v1',cargoSHA256:'0'.repeat(64),helpers:[]});writeFileSync(join(root,'tools/rust-build-auxiliary.json'),raw);const proof={forced:false,knownAuxiliaryCleanup:false,helperAdmission:{state:'disabled',reason:'owned.unreviewed_cargo_identity',helpers:[]}},options={root,policySHA256:digest(raw),identity,allowCleanup:true};assert.doesNotThrow(()=>cargoAdmission(proof,options));assert.throws(()=>cargoAdmission({...proof,forced:true},options));assert.throws(()=>cargoAdmission({...proof,knownAuxiliaryCleanup:true},options));assert.throws(()=>cargoAdmission({...proof,helperAdmission:{state:'enabled',cargo:identity,helpers:[]}},options));writeFileSync(cargo,'changed');assert.throws(()=>cargoAdmission(proof,options));}finally{rmSync(root,{recursive:true,force:true});}});

test('preparation and receipt replay share current fixture commands without migration features',()=>{
 assert.deepEqual(selectedFixturePreparers([]).map(e=>e.kind),['catalog','content','images']);
 for(const [kind,target,name] of [['catalog','agent_fixture_prepare','prepare_catalog_agent_fixture'],['content','content_budget','prepare_compressed_content_public_fixture'],['images','image_export_business','prepare_large_image_export_public_fixture'],['csv-numeric','agent_fixture_prepare','prepare_numeric_agent_fixture']]){
  const recipe=fixturePreparationRecipe(kind);
  assert.deepEqual(selectedFixturePreparers(['--'+kind]),[recipe]);
  assert.deepEqual(fixtureArgs(recipe),['test','--locked','--offline','-j','1','-p','lycheedev-data','--test',target,name,'--','--ignored','--exact','--nocapture','--test-threads=1']);
 }
 assert.throws(()=>selectedFixturePreparers(['--unknown']),/selects one fixture family/);
});
function imageFixtures(){return {images:{images:{schema:'lycheedev.large-image-paired-fixture.v1',encodings:['png','webp'],channels:['rgba'],cases:['palette','bgra8','bc1','bc2','bc3'].map((storage,i)=>({fileDataID:701+i,mipmap:0,texture:{width:1031,height:519,storage,format:'BLP2',mipmap:0,levels:1},pixelsBytes:1031*519*4,pixelsSHA256:'a'.repeat(64),pixelsPath:`pixels/${701+i}.rgba`}))}}};}
test('each selected Data case needs only its own fixture and preserves complete image expectations',()=>{
 const content={mode:'data',caseId:'B04-blte-17MiB'},catalog={mode:'data',caseId:'B04-sql-correctness'};
 assert.deepEqual(requiredFixtureKinds(content),['content']);assert.equal(selection(content,{content:{}}).kind,'content');
 assert.deepEqual(requiredFixtureKinds(catalog),['catalog']);assert.equal(selection(catalog,{catalog:{}}).performance,false);
 for(const mode of ['generation','csv'])assert.deepEqual(requiredFixtureKinds({mode}),['numeric']);
 for(const storage of ['palette','bgra8','bc1','bc2','bc3'])for(const encoding of ['png','webp']){
  const request={mode:'data',caseId:`B04-${storage}-${encoding}`},fixtures=imageFixtures();
  assert.deepEqual(requiredFixtureKinds(request),['images']);const entry=selection(request,fixtures);assert.equal(entry.kind,'image');assert.equal(entry.encoding,encoding);assert.equal(entry.image.texture.storage,storage);
  fixtures.images.images.cases[4].pixelsBytes--;assert.throws(()=>selection(request,fixtures));
 }
 assert.throws(()=>requiredFixtureKinds({mode:'data',caseId:'unknown'}));
 assert.throws(()=>selection({mode:'data',caseId:'B04-palette-png'},{}));
});
test('selected asset acceptance still requires 20 pairs and exact fifty percent target',()=>{
 const request={mode:'data',caseId:'B04-blte-17MiB',acceptance:true},entry=selection(request,{content:{}});
 const rows=Array.from({length:20},()=>({go:{cold:{controllableMillis:100},warm:{controllableMillis:100}},rust:{cold:{controllableMillis:50},warm:{controllableMillis:50}}}));
 assert(groups(rows,request,entry).every(g=>g.performanceAcceptanceEvaluated&&g.state==='passed_selected_path'&&g.fullB04Acceptance===false));
 assert.throws(()=>groups(rows.slice(1),request,entry));
 for(const row of rows)row.rust.warm.controllableMillis=51;
 assert.equal(groups(rows,request,entry)[1].state,'failed_performance');
});
