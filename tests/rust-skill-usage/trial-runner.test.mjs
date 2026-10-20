// Formal Node safety/coordination tests. Never native CLI or blind-agent evidence.
import test from 'node:test';
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import {mkdtempSync,mkdirSync,readFileSync,writeFileSync,rmSync,symlinkSync,linkSync,existsSync,cpSync} from 'node:fs';
import {join,resolve} from 'node:path';
import {tmpdir} from 'node:os';
import {Readable} from 'node:stream';
import {DatabaseSync} from 'node:sqlite';
import {checkedArgv,observeResult,SOURCE,LIMITS,PRODUCT_COMMIT} from './trial-policy.mjs';
import {atomicNew,childOutcome,decodeRequest,makeBudget,publishRequest,publishResponse,takeRequest,readBounded} from './trial-protocol.mjs';
import {guideContract,retireTrial,checkProduct,verifyTrialArchive} from './trial-owned.mjs';
import {seedDataHome,digest,MISSING_KEYS,RESTORED_KEYS} from './data-inputs.mjs';
import {dataSkillIdentity} from './data-skill-contract.mjs';
import {beginScratch} from '../../tools/rust-disk.mjs';
import {activeSession,relay} from '../../tools/rust-skill-trial.mjs';
const root=resolve(import.meta.dirname,'../..');
function temporary(t){const path=mkdtempSync(join(tmpdir(),'skill-trial-node-'));t.after(()=>rmSync(path,{recursive:true,force:true}));return path;}
const context=()=>({kind:'catalog',home:'C:\\owner\\home',installation:'C:\\owner\\fixture\\game\\不是身份 client',pins:['PIN-'+'1'.repeat(64)],captures:['CAP-'+'2'.repeat(64)]});
const data=(c,sql='SELECT ID FROM HouseDecor')=>['data','sql','--sql',sql,'--snapshot',c.pins[0],'--installation',c.installation,'--offline'];
test('fixed product commit and runner identity remain separate; unchanged gate rejects tampered shared tools',t=>{
  assert.equal(PRODUCT_COMMIT,'86f2bbb0f65234bba1bc0454d916e85fc6855ad9');const fake=temporary(t);mkdirSync(join(fake,'tools'));writeFileSync(join(fake,'tools/rust-build.mjs'),'tampered');
  assert.throws(()=>checkProduct(fake,root,'missing','missing'),/unchanged product identity/);
  const text=readFileSync(join(root,'tests/rust-skill-usage/trial-owned.mjs'),'utf8');assert.match(text,/artifact\.inputHashes\.sourceTree, sourceTree/);assert.match(text,/verifyCandidate\(root, candidatePath, artifact, workflow\)/);
});
test('exact current and historical guides are package-external inputs; arbitrary guide cannot pass',t=>{
  assert.equal(guideContract(join(root,'skills/lycheedev')).candidateTree,dataSkillIdentity().candidateTree);
  const dir=temporary(t),guide=join(dir,'guide');cpSync(join(root,'skills/lycheedev'),guide,{recursive:true});writeFileSync(join(guide,'SKILL.md'),'fake');assert.throws(()=>guideContract(guide),/known_readonly_guide/);
  const old=JSON.parse(readFileSync(join(root,'tests/rust-skill-usage/baselines/rust-candidate-0002ce-files.json')));const historical=join(dir,'historical');mkdirSync(historical);
  for(const row of old.files){const path=join(historical,row.path);mkdirSync(resolve(path,'..'),{recursive:true});writeFileSync(path,row.text);}
  assert.equal(guideContract(historical).candidateTree,'5edd025cae923fb2235086f250862696db7a2483');
});
test('actual argv is retained and only fixed home/default JSON are appended',()=>{const c=context(),argv=data(c);const result=checkedArgv(argv,c);assert.deepEqual(result.args,[...argv,'--home',c.home,'--format','json']);assert.deepEqual(argv,data(c));});
test('stale page-size reaches actual CLI rather than invented parser success',()=>{const c=context();assert(checkedArgv([...data(c),'--page-size','2'],c).args.includes('--page-size'));});
test('mutation SQL remains a real CLI business attempt, while shell and filesystem commands are excluded',()=>{const c=context();checkedArgv(data(c,"UPDATE HouseDecor SET Name_lang = '测试名称' WHERE ID = 20"),c);
  for(const argv of [['cmd','/c','whoami'],['powershell','-Command','x'],['live','status'],['addon','install'],['cache','prune'],['source','validate','--path','C:\\external'],['data','set'],['evidence','remove',c.captures[0]]])assert.throws(()=>checkedArgv(argv,c));});
test('argv structure and alternate home/binary/env/output/file/input routes cannot escape',()=>{const c=context();
  for(const argv of [null,[],['version',1],['version\0'],['version','--home','elsewhere'],['version','--home='+c.home,'--home',c.home],['version','--bin','evil'],[...data(c),'--file','secret'],[...data(c),'--output','C:\\victim'],[...data(c),'--key-file','C:\\secret'],['version','--'],['version','--format','yaml'],['version','--help=1']])assert.throws(()=>checkedArgv(argv,c));
});
test('read-only discovery, init and returned PIN/CAP remain available',()=>{const c=context();for(const argv of [['version'],['describe'],['--help'],['data','--help'],['data','sql','--help'],['source','query','--help'],['init'],['target','show',c.pins[0]],['evidence','verify',c.captures[0]]])checkedArgv(argv,c);
  assert.throws(()=>checkedArgv(['target','show','PIN-'+'f'.repeat(64)],c));assert.throws(()=>checkedArgv(['evidence','show','CAP-'+'f'.repeat(64)],c));});
test('Data calls cannot change fixture, PIN, target product or omit offline',()=>{const c=context();for(const argv of [data(c).filter(x=>x!=='--offline'),[...data(c),'--product','classic'],data({...c,installation:'C:\\elsewhere'}),data({...c,pins:['PIN-'+'3'.repeat(64)]})])assert.throws(()=>checkedArgv(argv,c));});
test('source requires exact permitted revision and actual returned PIN; relative inspect cannot escape',()=>{const c={...context(),kind:'source',pins:[]};const sync=['source','sync','--source',SOURCE.repository,'--product',SOURCE.product,'--ref',SOURCE.commit];checkedArgv(sync,c);
  assert.throws(()=>checkedArgv(sync.slice(0,-2),c));assert.throws(()=>checkedArgv([...sync.slice(0,-1),'refs/heads/live'],c));
  const pin='PIN-'+'4'.repeat(64);observeResult(c,'source sync',JSON.stringify({result:{id:pin,source:{repository:SOURCE.repository,product:SOURCE.product,exactCommit:SOURCE.commit,requestedRef:SOURCE.commit}}}),0);
  checkedArgv(['source','query','C_Timer.After','--snapshot',pin],c);checkedArgv(['source','inspect','--path','Interface/AddOns/x.lua','--snapshot',pin],c);
  for(const path of ['../secret','/secret','C:\\secret','Interface/../secret'])assert.throws(()=>checkedArgv(['source','inspect','--path',path,'--snapshot',pin],c));
  assert.throws(()=>observeResult(c,'source sync',JSON.stringify({result:{id:pin,source:{exactCommit:'bad'}}}),0));
});
test('real Node process business nonzero is observable and next correction runs',()=>{const failed=spawnSync(process.execPath,['-e','process.stdout.write("partial");process.stderr.write("bad flag");process.exit(2)']);assert.deepEqual(childOutcome(failed),{exit:2,stdout:'partial',stderr:'bad flag'});
  assert.equal(childOutcome(spawnSync(process.execPath,['-e','process.stdout.write("corrected")'])).exit,0);});
test('timeout, signal and launch failure are fatal infrastructure outcomes',()=>{for(const run of [{status:null,error:new Error('ENOENT')},{status:null,signal:'SIGTERM'},{status:0,error:new Error('ENOBUFS')}])assert.throws(()=>childOutcome(run),/child_process_failure/);});
test('request publication is exclusive, atomic and never overwrites prior sequence',t=>{const dir=temporary(t),request={seq:1,op:'call',argv:['describe']};publishRequest(dir,request);assert.deepEqual(decodeRequest(takeRequest(dir,1),1),request);assert.throws(()=>publishRequest(dir,request),/duplicate/);
  assert.throws(()=>publishRequest(dir,{seq:2,op:'finish'}),/previous_response/);publishResponse(dir,1,{seq:1,exit:2});publishRequest(dir,{seq:2,op:'finish'});assert.equal(decodeRequest(takeRequest(dir,2),2).op,'finish');});
test('incomplete atomic publication is not consumed and a second publisher cannot take it',t=>{const dir=temporary(t);writeFileSync(join(dir,'001.request.json.tmp'),'{');assert.equal(takeRequest(dir,1),null);assert.throws(()=>publishRequest(dir,{seq:1,op:'finish'}),/EEXIST/);});
test('out-of-order, replay seq, unexpected fields and alternate executable are rejected',t=>{const dir=temporary(t);writeFileSync(join(dir,'002.request.json'),'{}');assert.throws(()=>takeRequest(dir,1),/out_of_order/);
  for(const request of [{seq:2,op:'finish'},{seq:1,op:'call',argv:['version'],executable:'evil'},{seq:1,op:'finish',argv:[]},{seq:1,op:'shell'},{seq:1,op:'restore-key',reason:''}])assert.throws(()=>decodeRequest(Buffer.from(JSON.stringify(request)),1));});
test('hardlink requests and oversized pending files are refused',t=>{const dir=temporary(t),source=join(dir,'original');writeFileSync(source,'{}');const inbox=join(dir,'inbox');mkdirSync(inbox);linkSync(source,join(inbox,'001.request.json'));assert.throws(()=>takeRequest(inbox,1),/regular_unlinked/);rmSync(join(inbox,'001.request.json'));writeFileSync(join(inbox,'001.request.json.tmp'),Buffer.alloc(LIMITS.requestBytes+1));assert.throws(()=>takeRequest(inbox,1),/inbox_file_bytes/);assert.throws(()=>readBounded(source,1),/file_bytes/);});
test('symlink requests and reparse inbox are refused when host permits symlinks',t=>{const dir=temporary(t),source=join(dir,'original');writeFileSync(source,'{}');const inbox=join(dir,'inbox');mkdirSync(inbox);
  try{symlinkSync(source,join(inbox,'001.request.json'));}catch(error){if(['EPERM','EACCES'].includes(error.code)){t.skip('Host denies symlink creation: '+error.code);return;}throw error;}
  assert.throws(()=>takeRequest(inbox,1),/regular_unlinked/);const linked=join(dir,'linked');symlinkSync(inbox,linked,'dir');assert.throws(()=>takeRequest(linked,1),/regular_unlinked/);
});
test('command, input, output, idle and total budgets independently fail closed',()=>{const small={...LIMITS,commands:1,inputBytes:5,outputBytes:5,streamBytes:4,idleMillis:10,totalMillis:20};let now=0;const budget=()=>makeBudget(()=>now,small);
  let b=budget();b.accept(1,true);assert.throws(()=>b.accept(1,true),/command_count/);b=budget();assert.throws(()=>b.accept(6,false),/input_bytes/);
  b=budget();assert.throws(()=>b.record(Buffer.alloc(5),Buffer.alloc(0)),/output_budget/);b=budget();b.record(Buffer.alloc(3),Buffer.alloc(0));assert.throws(()=>b.record(Buffer.alloc(3),Buffer.alloc(0)),/output_budget/);
  b=budget();now=11;assert.throws(()=>b.check(),/idle_deadline/);now=0;b=budget();now=9;b.accept(0,false);now=18;b.accept(0,false);now=21;assert.throws(()=>b.check(),/total_deadline/);
});
test('EOF, malformed input and explicit cancellation close one-shot relay without retry',async()=>{for(const chunks of [[],['not json\n']]){const calls=[];const work=relay('unused',Readable.from(chunks),()=>{},async(_path,r)=>{calls.push(r);return {seq:r.seq};});if(chunks.length)await assert.rejects(work);else await work;assert.deepEqual(calls,[{seq:1,op:'cancel'}]);}
  const calls=[];await relay('unused',Readable.from(['{"op":"call","argv":["version"]}\n','{"op":"finish"}\n']),()=>{},async(_path,r)=>{calls.push(r);return {seq:r.seq,exit:2};});assert.deepEqual(calls.map(r=>r.op),['call','finish']);
});
test('terminal owner or altered session inbox is not admitted',t=>{const dir=temporary(t),scratch=join(dir,'scratch');mkdirSync(scratch);mkdirSync(join(scratch,'inbox'));const receipt=join(dir,'receipt.json'),path=join(dir,'session.json'),owner={schema:'lycheedev.disk-owner.v1',id:'id',owner:'skill-data-usage',state:'active',directory:scratch};writeFileSync(receipt,JSON.stringify(owner));const session={schema:'lycheedev.skill-trial-session.v1',ownerId:'id',receipt,inbox:join(scratch,'inbox'),inputs:join(scratch,'inputs.json'),deadline:Date.now()+10000};writeFileSync(path,JSON.stringify(session));assert.equal(activeSession(path).ownerId,'id');writeFileSync(receipt,JSON.stringify({...owner,state:'retired'}));assert.throws(()=>activeSession(path),/owner_terminal/);});
test('fresh seeded homes share input identity but cannot carry cache/key mutations across arms',t=>{const dir=temporary(t),fixture={documents:[{key:'pin/test',bytes:Buffer.from('{"id":"same"}')}],blobs:[]},homes=['a','b'].map(name=>join(dir,name));for(const home of homes){mkdirSync(join(home,'state'),{recursive:true});writeFileSync(join(home,'workspace.json'),'{}');seedDataHome(fixture,home);}
  const db=new DatabaseSync(join(homes[0],'state/toolkit.sqlite'));db.exec("UPDATE documents SET value=x'7b7d'");db.close();const other=new DatabaseSync(join(homes[1],'state/toolkit.sqlite'));assert.equal(Buffer.from(other.prepare('SELECT value FROM documents').get().value).toString(),'{"id":"same"}');other.close();
  writeFileSync(join(homes[0],'keys'),RESTORED_KEYS);writeFileSync(join(homes[1],'keys'),MISSING_KEYS);assert.notEqual(digest(readFileSync(join(homes[0],'keys'))),digest(readFileSync(join(homes[1],'keys'))));assert.equal(fixture.documents[0].bytes.toString(),'{"id":"same"}');assert.throws(()=>seedDataHome(fixture,homes[0]));
});
test('existing owner seals evidence before cleanup; unresolved or unarchived scratch retained',t=>{const root=temporary(t),job=beginScratch(root,'skill-data-usage');const log=join(job.evidence,'test.json');writeFileSync(log,'{}');retireTrial(job,{state:'failed',handlesReaped:true,logs:[log],archiveComplete:true});assert(!existsSync(job.directory));assert.equal(JSON.parse(readFileSync(job.receipt)).state,'retired');
  for(const handlesReaped of [false,true]){const retained=beginScratch(root,'skill-data-usage');retireTrial(retained,{state:'failed',handlesReaped,logs:[],archiveComplete:false});assert(existsSync(retained.directory));assert.equal(JSON.parse(readFileSync(retained.receipt)).state,handlesReaped?'failed':'interrupted');}
});
test('scripted 42-command workflow is untouched and trial does not prepare answer queries or automatically restore keys',()=>{const run=readFileSync(join(root,'tests/rust-skill-usage/trial-run.mjs'),'utf8');assert(!run.includes('CASES'));assert(!run.includes('dataCase('));assert.match(run,/request\.op==='restore-key'/);assert.match(run,/fixture=loadFixture/);assert.match(run,/seedDataHome\(fixture,config.home\)/);assert.match(run,/owned/);});
test('response serialization expansion stays bounded before publication',t=>{const dir=temporary(t);assert.throws(()=>publishResponse(dir,1,{stdout:'\0'.repeat(3<<20)}),/response_bytes/);assert(!existsSync(join(dir,'001.response.json')));});
test('relay preserves split UTF-8 command bytes and EOF cancels after one response',async()=>{const raw=Buffer.from('{"op":"call","argv":["data","sql","--sql","SELECT 名称"]}\n'),at=raw.indexOf(Buffer.from('名称'))+1,calls=[];
  await relay('unused',Readable.from([raw.subarray(0,at),raw.subarray(at)]),()=>{},async(_path,r)=>{calls.push(r);return {seq:r.seq};});assert.equal(calls[0].argv.at(-1),'SELECT 名称');assert.equal(calls[1].op,'cancel');});
test('trial fixture directory satisfies the real partial exporter owner contract',()=>{const exporter=readFileSync(join(root,'crates/lycheedev-data/tests/skill_data_fixture.rs'),'utf8'),runner=readFileSync(join(root,'tests/rust-skill-usage/trial-run.mjs'),'utf8');assert.match(exporter,/join\("partial-fixture"\)/);assert.match(runner,/directory=join\(scratch,config\.kind\+'-fixture'\)/);});
test('normal fixed-PIN source readiness and bounded relation flags pass through',()=>{const c={...context(),kind:'source'};checkedArgv(['source','list','--snapshot',c.pins[0]],c);checkedArgv(['source','refs','--snapshot',c.pins[0],'--symbol','C_Timer.After','--direction','incoming','--static-only'],c);checkedArgv(['source','context','--snapshot',c.pins[0],'--symbol','C_Timer.After','--depth','1','--max-lines','20','--max-bytes','1024','--static-only'],c);});
test('ordinary Data row lookup and named SQL parameters use the same isolated fixture',()=>{const c=context();checkedArgv(['data','db2','--table','HouseDecor','--id','20',...data(c).slice(4)],c);checkedArgv([...data(c),'--param','id=2','--param','name=hello'],c);});
test('terminal responses are atomically retained in evidence independently of scratch cleanup',()=>{const run=readFileSync(join(root,'tests/rust-skill-usage/trial-run.mjs'),'utf8'),entry=readFileSync(join(root,'tools/rust-skill-trial.mjs'),'utf8');assert.match(run,/atomicNew\(evidence,'response-'/);assert.match(entry,/join\(dirname\(resolve\(path\)\),'response-'/);assert.match(run,/finish_requested/);});
test('only machine JSON is accepted, including successful sync identity discovery',()=>{const c=context();for(const format of ['text','jsonl'])assert.throws(()=>checkedArgv(['version','--format',format],c),/machine_json_required/);checkedArgv(['version','--format','json'],c);});
test('outer independently verifies archive parts and live inventory before cleanup',t=>{
  const dir=temporary(t),scratch=join(dir,'scratch'),evidence=join(dir,'evidence');mkdirSync(scratch);mkdirSync(evidence);
  const ref=(name,bytes)=>{writeFileSync(join(evidence,name),bytes);return {file:name,bytes:bytes.length,sha256:digest(bytes)};};
  const report={archiveComplete:true,state:'finished',config:{kind:'source',scratch,home:join(scratch,'home')},archives:{},commands:[],requests:[]};
  for(const label of ['home','guide','inbox']){const directory=join(scratch,label);mkdirSync(directory);const bytes=Buffer.from(label);writeFileSync(join(directory,'item'),bytes);const chunk=ref(label+'.bin',bytes);report.archives[label]=ref(label+'.json',Buffer.from(JSON.stringify({directory,files:[{path:'item',bytes:bytes.length,sha256:digest(bytes),chunks:[chunk]}]})));}
  assert.equal(verifyTrialArchive(report,evidence),true);
  writeFileSync(join(evidence,'home.bin'),'bad');assert.throws(()=>verifyTrialArchive(report,evidence));writeFileSync(join(evidence,'home.bin'),'home');
  writeFileSync(join(scratch,'home','extra'),'new');assert.throws(()=>verifyTrialArchive(report,evidence),/archive_inventory/);rmSync(join(scratch,'home','extra'));
  const manifest=JSON.parse(readFileSync(join(evidence,'home.json')));manifest.files[0].chunks.reverse();manifest.files[0].chunks[0].bytes=0;report.archives.home=ref('home.json',Buffer.from(JSON.stringify(manifest)));assert.throws(()=>verifyTrialArchive(report,evidence));
});
test('outer never turns an unverified archiveComplete boolean into permission to clean',()=>{const owner=readFileSync(join(root,'tests/rust-skill-usage/trial-owned.mjs'),'utf8');assert.match(owner,/archiveComplete=false;/);assert.match(owner,/verifyTrialArchive\(report,job\.evidence\)/);assert(owner.indexOf('archiveComplete=true;')>owner.indexOf('verifyTrialArchive(report,job.evidence)'));const run=readFileSync(join(root,'tests/rust-skill-usage/trial-run.mjs'),'utf8');assert.match(run,/admission\(root,\{requiredBytes:archiveBytes\+\(16<<20\)\}\)/);});
test('packaged launcher cannot be shadowed by a prefix EXE or missing npm shim',()=>{const run=readFileSync(join(root,'tests/rust-skill-usage/trial-run.mjs'),'utf8');assert.equal((run.match(/trial\.no_prefix_exe_override/g)||[]).length,2);assert.match(run,/hashFile\(join\(config\.prefix,'lycheedev\.cmd'\)\),report\.npmShim\.sha256/);});
test('successful source sync help does not impersonate sync or stop later discovery',()=>{const c={...context(),kind:'source',pins:[]},help=checkedArgv(['source','sync','--help'],c);assert.equal(help.help,true);observeResult(c,help.path,JSON.stringify({result:{commands:[{path:'source sync'}]}}),0,help);assert.deepEqual(c.pins,[]);checkedArgv(['describe'],c);
  const invocation=checkedArgv(['source','sync','--source',SOURCE.repository,'--product',SOURCE.product,'--ref',SOURCE.commit],c),pin='PIN-'+'4'.repeat(64);observeResult(c,invocation.path,JSON.stringify({result:{id:pin,source:{repository:SOURCE.repository,product:SOURCE.product,exactCommit:SOURCE.commit,requestedRef:SOURCE.commit}}}),0,invocation);assert.deepEqual(c.pins,[pin]);});
