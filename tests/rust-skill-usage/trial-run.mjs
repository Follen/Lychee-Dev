// Runs only inside the existing Windows Job. No child escapes this owner.
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import {copyFileSync, existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync} from 'node:fs';
import {dirname, join, resolve, delimiter} from 'node:path';
import {setTimeout as delay} from 'node:timers/promises';
import {hashFile} from '../../tools/rust-build.mjs';
import {admission} from '../../tools/rust-disk.mjs';
import {inventory, inventoryHash, jsonFile, within} from './assertions.mjs';
import {archiveFixture, bounded, digest, loadExporters, loadFixture, npmOfflineFlags, seedDataHome, MISSING_KEYS, RESTORED_KEYS} from './data-inputs.mjs';
import {assertDataSkillDirectory} from './data-skill-contract.mjs';
import {checkedArgv, observeResult, SOURCE, LIMITS} from './trial-policy.mjs';
import {checkProduct, guideContract} from './trial-owned.mjs';
import {findNpm} from './data-owned.mjs';
import {atomicNew, childOutcome, decodeRequest, makeBudget, publishResponse, takeRequest} from './trial-protocol.mjs';

assert.equal(process.platform,'win32'); assert.equal(process.arch,'x64');
const [configPath,...extra]=process.argv.slice(2); assert(configPath&&!extra.length);
const config=jsonFile(configPath), {scratch,evidence,root}=config;
const owner=jsonFile(join(evidence,'receipt.json'));
assert.equal(config.schema,'lycheedev.skill-trial-config.v1'); assert.equal(configPath,join(evidence,'configuration.json'));
assert.equal(resolve('.'),root); assert.equal(config.node,process.execPath); assert.equal(config.nodeSHA256,hashFile(process.execPath));
assert.equal(config.runnerRoot,resolve(import.meta.dirname,'../..')); assert.deepEqual(config.limits,LIMITS);
assert.equal(owner.owner,'skill-data-usage'); assert.equal(owner.state,'active'); assert.equal(owner.id,config.ownerId); assert.equal(owner.directory,scratch);
assert.equal(scratch,join(root,'.tmp/rust-scratch',owner.id)); assert.equal(evidence,join(root,'.tmp/rust-disk/evidence',owner.id));
assert.equal(config.home,join(scratch,'home'));assert.equal(config.prefix,join(scratch,'npm-prefix'));
assert.equal(config.launcher,join(config.prefix,'node_modules/lycheedev/payload/skill/scripts/lycheedev.mjs'));
assert.equal(owner.inputHashes.purpose,'independent-agent-trial'); assert.deepEqual(owner.inputHashes.testInputs,config.testInputs);
const verified=checkProduct(root,config.runnerRoot,config.artifactPath,config.candidatePath);
assert.equal(verified.sourceTree,config.sourceTree);assert.equal(verified.candidate.packageRoot,config.sourcePackage);
assert.equal(verified.candidate.packageSHA256,config.packageSHA256);assert.deepEqual(verified.candidate.files,config.packageFiles);
for(const [path,hash]of Object.entries(config.testInputs))assert.equal(hashFile(join(config.runnerRoot,path)),hash,'trial.runner_startup_integrity');
assert.deepEqual(findNpm(),config.npm);assert.deepEqual(guideContract(config.guideDirectory),config.guideIdentity);
if(config.kind!=='source')assert.deepEqual(loadExporters(root,config.exporters.receiptPath,config.sourceTree),config.exporters);
assert.deepEqual(readdirSync(scratch),[]); assert.equal(inventoryHash(inventory(config.sourcePackage)),config.packageSHA256);
const report={schema:'lycheedev.skill-trial.v1',config,state:'preparing',scored:false,completeSkillAcceptance:false,startedAt:new Date().toISOString(),commands:[],requests:[],archiveComplete:false};
const save=()=>writeFileSync(join(evidence,'trial-report.json'),JSON.stringify(report,null,2)+'\n');
const store=(name,bytes)=>{writeFileSync(join(evidence,name),bytes,{flag:'wx'});return {file:name,bytes:bytes.length,sha256:digest(bytes)};};
const budget=makeBudget(); let processUnsafe=false, fixture;
const inbox=join(scratch,'inbox'), guide=join(scratch,'guide');
const env=Object.fromEntries(Object.entries(process.env).filter(([name])=>!/^(LYCHEEDEV_|NPM_CONFIG_|GIT_)/i.test(name)&&!['PATH','HOME','USERPROFILE','TEMP','TMP','NODE_OPTIONS','NODE_PATH'].includes(name.toUpperCase())));
Object.assign(env,{PATH:config.prefix+delimiter+(process.env.PATH??process.env.Path??''),HOME:join(scratch,'user'),USERPROFILE:join(scratch,'user'),TEMP:join(scratch,'temp'),TMP:join(scratch,'temp')});
const context={kind:config.kind,home:config.home,pins:[],captures:[]};
function command(kind,executable,args,environment={},cwd=scratch){
  assert(!processUnsafe,'trial.prior_process_failure'); const timeout=budget.timeout(), startedAt=new Date().toISOString(), began=performance.now();
  const run=spawnSync(executable,args,{cwd,env:{...env,...environment},shell:false,windowsHide:true,timeout,maxBuffer:LIMITS.streamBytes});
  const stdout=run.stdout??Buffer.alloc(0),stderr=run.stderr??Buffer.alloc(0),id=String(report.commands.length).padStart(3,'0');
  const row={kind,executable,args,cwd,startedAt,endedAt:new Date().toISOString(),wallMillis:performance.now()-began,exit:run.status,signal:run.signal,error:run.error?.message,stdout:store(`command-${id}.stdout.bin`,stdout),stderr:store(`command-${id}.stderr.bin`,stderr)};
  report.commands.push(row); save();
  let outcome;try{outcome=childOutcome(run);}catch(error){processUnsafe=true;throw error;}
  budget.record(stdout,stderr);
  return {...outcome,stdoutSHA256:row.stdout.sha256,stderrSHA256:row.stderr.sha256};
}
function setup(executable,args,environment={},cwd=scratch){const result=command('preparation',executable,args,environment,cwd);assert.equal(result.exit,0,'trial.preparation_failed:'+result.stderr.slice(0,400));return result.stdout;}
function publicCall(invocation){
  assert(!existsSync(join(config.prefix,'lycheedev.exe')),'trial.no_prefix_exe_override');
  assert.equal(hashFile(join(config.prefix,'lycheedev.cmd')),report.npmShim.sha256,'trial.npm_shim_identity');
  const installed=join(config.prefix,'node_modules/lycheedev');
  for(const name of ['payload/skill/scripts/lycheedev.mjs','bin/lycheedev.mjs','native/windows-amd64/lycheedev.exe']){
    const expected=config.packageFiles.find(row=>row.path===name);assert(expected);assert.equal(hashFile(within(installed,name),128<<20),expected.sha256,'trial.authenticated_launcher_chain');
  }
  const result=command('agent',process.execPath,[config.launcher,...invocation.args],{LYCHEEDEV_HOME:config.home});
  observeResult(context,invocation.path,result.stdout,result.exit,invocation); return result;
}
function respond(seq,response){publishResponse(inbox,seq,response);atomicNew(evidence,'response-'+String(seq).padStart(3,'0')+'.json',Buffer.from(JSON.stringify(response)));}
function archiveDirectory(directory,label){
  if(!existsSync(directory))return null;
  const files=inventory(directory,{maxBytes:512<<20,maxFiles:20000}), rows=[];
  for(const [i,row]of files.entries()){
    const bytes=bounded(within(directory,row.path),128<<20);assert.equal(digest(bytes),row.sha256);const chunks=[];
    for(let offset=0;offset<bytes.length||offset===0;offset+=8<<20)chunks.push(store(`${label}-${String(i).padStart(5,'0')}-${offset}.bin`,bytes.subarray(offset,offset+(8<<20))));
    rows.push({...row,chunks});
  }
  assert.deepEqual(inventory(directory,{maxBytes:512<<20,maxFiles:20000}),files,'trial.archive_changed');
  return store(label+'-manifest.json',Buffer.from(JSON.stringify({directory,files:rows})));
}
try{
  save(); for(const path of [env.HOME,env.TEMP,inbox,guide])mkdirSync(path);
  for(const name of ['user.npmrc','global.npmrc'])writeFileSync(join(scratch,name),'',{flag:'wx'});
  const contract=assertDataSkillDirectory(config.guideDirectory,config.guideIdentity);
  for(const row of contract.files){const path=join(guide,row.path);mkdirSync(dirname(path),{recursive:true});copyFileSync(within(config.guideDirectory,row.path),path);}
  assertDataSkillDirectory(guide,config.guideIdentity);
  // Same offline, ignore-scripts npm installation as the scripted acceptance.
  assert.equal(hashFile(config.npm.path),config.npm.sha256);
  const packed=JSON.parse(setup(process.execPath,[config.npm.path,'pack',config.sourcePackage,'--json','--pack-destination',scratch,...npmOfflineFlags(scratch)]));
  assert.equal(packed.length,1); assert.equal(packed[0].filename,`lycheedev-${config.version}.tgz`);
  const tgz=join(scratch,packed[0].filename),raw=bounded(tgz,128<<20);
  assert.equal(digest(raw,'sha1'),packed[0].shasum);assert.equal('sha512-'+Buffer.from(digest(raw,'sha512'),'hex').toString('base64'),packed[0].integrity);
  report.tgz={path:tgz,bytes:raw.length,sha256:digest(raw)};
  setup(process.execPath,[config.npm.path,'install','--global','--prefix',config.prefix,tgz,...npmOfflineFlags(scratch)]);
  assert.deepEqual(inventory(join(config.prefix,'node_modules/lycheedev')),config.packageFiles);
  assertDataSkillDirectory(join(config.prefix,'node_modules/lycheedev/payload/skill'));
  report.npmShim=store('npm-shim.cmd',bounded(join(config.prefix,'lycheedev.cmd')));
  assert(!existsSync(join(config.prefix,'lycheedev.exe')),'trial.no_prefix_exe_override');
  assert(!existsSync(config.home),'trial.fresh_home');
  setup(process.execPath,[config.launcher,'init','--home',config.home,'--format','json'],{LYCHEEDEV_HOME:config.home});
  if(config.kind!=='source'){
    const index=config.kind==='catalog'?0:1,exporter=config.exporters.executables[index],directory=join(scratch,config.kind+'-fixture');
    assert.equal(hashFile(exporter.path,128<<20),exporter.sha256);
    setup(exporter.path,[index===0?'prepare_catalog_public_fixture':'prepare_skill_partial_fixture','--exact','--ignored','--nocapture'],index===0?{LYCHEEDEV_CATALOG_FIXTURE:directory}:{LYCHEEDEV_SKILL_PARTIAL_FIXTURE:directory,LYCHEEDEV_SKILL_OWNER_RECEIPT:join(evidence,'receipt.json')},root);
    fixture=loadFixture(directory,config.kind);report.fixture=archiveFixture(fixture,config.kind,evidence);seedDataHome(fixture,config.home);
    context.installation=fixture.identity.installation;context.pins=[fixture.identity.snapshot];
    if(config.kind==='partial'){context.keyFile=join(scratch,'sample.keys');writeFileSync(context.keyFile,MISSING_KEYS,{flag:'wx'});report.initialKeys=store('initial.keys',Buffer.from(MISSING_KEYS));}
  }
  const inputs={schema:'lycheedev.skill-trial-inputs.v1',launcher:config.launcher,home:config.home,guide, ...(fixture?{snapshot:fixture.identity.snapshot,installation:fixture.identity.installation,pin:fixture.identity.pin,...(context.keyFile?{keyFile:context.keyFile}:{})}:{source:SOURCE})};
  writeFileSync(join(scratch,'inputs.json'),JSON.stringify(inputs,null,2)+'\n',{flag:'wx'});
  // Session coordinates only. The independent agent receives inputs + guide and
  // a call tool mediated by its coordinator, never configuration/evidence/oracle.
  atomicNew(evidence,'session.json',Buffer.from(JSON.stringify({schema:'lycheedev.skill-trial-session.v1',ownerId:config.ownerId,receipt:join(evidence,'receipt.json'),inbox,inputs:join(scratch,'inputs.json'),deadline:Date.now()+Math.max(0,LIMITS.totalMillis-budget.snapshot().elapsedMillis)})));
  report.state='ready';save();
  for(let seq=1;;seq++){
    budget.check();const raw=takeRequest(inbox,seq);if(!raw){await delay(LIMITS.pollMillis);seq--;continue;}
    const request=decodeRequest(raw,seq);budget.accept(raw.length,request.op==='call');report.requests.push({request,archive:store(`request-${seq}.json`,raw),at:new Date().toISOString()});save();
    let response;
    if(request.op==='finish'||request.op==='cancel'){
      report.state=request.op==='finish'?'finished':'cancelled';respond(seq,{seq,state:request.op==='finish'?'finish_requested':'cancel_requested'});break;
    }
    if(request.op==='restore-key'){
      assert.equal(config.kind,'partial','trial.partial_key_only');assert.equal(readFileSync(context.keyFile,'utf8'),MISSING_KEYS,'trial.key_already_restored');
      writeFileSync(context.keyFile,RESTORED_KEYS);report.restoredKeys=store('restored.keys',Buffer.from(RESTORED_KEYS));response={state:'key_file_available',keyFile:context.keyFile};
    }else{
      let invocation;
      try{invocation=checkedArgv(request.argv,context);}catch(error){response={state:'rejected',reason:error.message};}
      // Once a process is launched, every infrastructure or result-identity
      // failure is terminal. Only the explicit input guard can reject a call.
      if(invocation)response={state:'executed',...publicCall(invocation)};
    }
    respond(seq,{seq,...response});save();
  }
}catch(error){report.state='failed';report.error=error.stack;process.exitCode=1;}
finally{
  try{
    if(!processUnsafe){
      if(fixture)assert.deepEqual(inventory(fixture.directory,{maxFiles:256,maxBytes:64<<20}),fixture.files,'trial.fixture_unchanged');
      if(existsSync(join(guide,'SKILL.md')))assertDataSkillDirectory(guide,config.guideIdentity);
      if(existsSync(join(config.prefix,'node_modules/lycheedev')))assert.deepEqual(inventory(join(config.prefix,'node_modules/lycheedev')),config.packageFiles);
      for(const row of report.requests)assert.equal(hashFile(join(inbox,String(row.request.seq).padStart(3,'0')+'.request.json')),row.archive.sha256,'trial.consumed_request_changed');
      const archiveBytes=[config.home,guide,inbox].filter(existsSync).reduce((sum,path)=>sum+inventory(path,{maxBytes:512<<20,maxFiles:20000}).reduce((n,row)=>n+row.bytes,0),0);
      report.archiveAdmission=admission(root,{requiredBytes:archiveBytes+(16<<20)});
      report.archives={home:archiveDirectory(config.home,'home'),guide:archiveDirectory(guide,'guide'),inbox:archiveDirectory(inbox,'inbox')};
      if(existsSync(join(scratch,'inputs.json')))report.inputs=store('inputs.json',bounded(join(scratch,'inputs.json')));
      report.archiveComplete=true;
    }
  }catch(error){report.state='failed';report.archiveError=error.stack;process.exitCode=1;}
  report.budget=budget.snapshot();report.endedAt=new Date().toISOString();save();if(report.state!=='finished')process.exitCode=1;
}
