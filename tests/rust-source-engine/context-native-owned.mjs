// Custody and fixed expectations for the isolated native typed-call profile.
import assert from 'node:assert/strict';
import {createHash,randomUUID} from 'node:crypto';
import {copyFileSync,existsSync,lstatSync,mkdirSync,readFileSync,writeFileSync} from 'node:fs';
import {basename,dirname,join,resolve,sep} from 'node:path';
import {spawn,spawnSync} from 'node:child_process';
import {hashFile,sourceDigest} from '../../tools/rust-build.mjs';
import {verifyFrozenContext} from './context-identity.mjs';

const root=resolve('.'),out=resolve(process.argv[2]);
function verifiedContext(identity){
 const verified=verifyFrozenContext(root,identity.path);
 assert.equal(verified.sha256,identity.sha256);assert.equal(verified.record.patchSHA256,identity.patchSHA256);return verified;
}
if(process.argv[3]==='--worker'){
 const artifact=JSON.parse(readFileSync(join(out,'artifact.json')));verifiedContext(artifact.sealedBuild.contextIdentity);
 const began=Date.now();
 async function child(command,args,input,maxBytes){
  assert(Date.now()-began<25000,'worker total deadline');
  const running=spawn(command,args,{cwd:root,windowsHide:true,stdio:['pipe','pipe','pipe']});
  const stdout=[],stderr=[];let bytes=0,error;
  for(const [chunks,stream] of [[stdout,running.stdout],[stderr,running.stderr]])stream.on('data',chunk=>{
   bytes+=chunk.length;if(bytes>maxBytes){error='child.output_budget';running.kill();}else chunks.push(chunk);
  });
  running.stdin.on('error',e=>{error??=String(e);});running.stdin.end(input);
  const timer=setTimeout(()=>{error='child.deadline';running.kill();},Math.min(5000,25000-(Date.now()-began)));
  const exit=await new Promise(done=>{running.once('error',e=>{error=String(e);done(null);});running.once('close',done);});clearTimeout(timer);
  return {args,exit,stdout:Buffer.concat(stdout).toString('utf8'),stderr:Buffer.concat(stderr).toString('utf8'),error};
 }
 assert.equal(hashFile(artifact.binary),artifact.binarySHA256);assert.equal(hashFile(artifact.luac),artifact.runtime.sha256);
 const analysis=await child(artifact.binary,[artifact.probeCommand],null,4<<20);
 writeFileSync(join(out,'analysis.stdout.jsonl'),analysis.stdout);writeFileSync(join(out,'analysis.stderr.txt'),analysis.stderr);
 const syntax=[];
 for(const fixture of artifact.fixtures){
  assert.equal(hashFile(fixture.path),fixture.sha256);assert.equal(lstatSync(fixture.path).size,fixture.bytes);
  const input=readFileSync(fixture.path),result=await child(artifact.luac,['-p','-'],input,65536);
  assert(/^fixture-[0-9]{2}$/.test(fixture.evidenceBasename));
  writeFileSync(join(out,fixture.evidenceBasename+'.stdin.lua'),input);
  writeFileSync(join(out,fixture.evidenceBasename+'.luac.stderr.txt'),result.stderr);
  syntax.push({...fixture,...result,sourceExecuted:false,passed:result.exit===0&&!result.error&&result.stdout===''});
 }
 writeFileSync(join(out,'worker.json'),JSON.stringify({analysis,syntax,elapsedMs:Date.now()-began},null,2));
 if(analysis.exit!==0||analysis.error||!syntax.every(row=>row.passed))process.exitCode=1;
}else{
 const [,,output,buildPath,contractPath]=process.argv;
 assert(output&&buildPath&&contractPath,'usage: context-native-owned.mjs <new-out> <build-artifact.json> <fixed-contract.json>');
 assert(out.startsWith(root+sep)&&!existsSync(out),'fresh evidence directory required');
 const sealedBuild=JSON.parse(readFileSync(resolve(buildPath)));
 assert.equal(sealedBuild.schema,'lycheedev.emmy-context-build.v1');assert.equal(sealedBuild.example,'emmy_native_context_probe');assert.equal(sealedBuild.sameInvocation,true);
 const frozen=verifiedContext(sealedBuild.contextIdentity),contractFile=resolve(contractPath);
 assert(contractFile.startsWith(root+sep));
 const contract=JSON.parse(readFileSync(contractFile));assert.equal(contract.schema,'lycheedev.emmy-native-context-contract.v1');
 const frozenContract=frozen.record.inputs.find(row=>join(root,row.path)===contractFile);
 assert(frozenContract&&frozenContract.sha256===hashFile(contractFile),'fixed expected contract must be frozen before compilation');
 assert(Array.isArray(contract.assertions)&&contract.assertions.length>0&&contract.assertions.length<=512);
 assert.equal(new Set(contract.assertions.map(row=>row.name)).size,contract.assertions.length);
 const probeCommand=contract.command??'golden';assert(['golden','coverage-red','object-red','object-audit-red','invocation-lifecycle','invocation-callsite-red','invocation-return-slots'].includes(probeCommand));
 assert.deepEqual(contract.scenarios,probeCommand==='golden'?['after-a','unknown-a','after-b']:probeCommand==='coverage-red'?['fixed-return-unknown-argument']:probeCommand==='object-red'?['object-versions']:probeCommand==='object-audit-red'?['rebind','colon','field-closure','expression','record-union']:probeCommand==='invocation-return-slots'?['return-slots']:probeCommand==='invocation-callsite-red'?['call-capture']:['captures','reassign','short-circuit','selector','escape','escape-closure','escape-rebind','call-capture']);
 const expectedRedFailures=contract.expectedRedFailures??[];
 assert(Array.isArray(expectedRedFailures)&&expectedRedFailures.length<=12&&new Set(expectedRedFailures).size===expectedRedFailures.length);
 assert(expectedRedFailures.every(name=>contract.assertions.some(row=>row.name===name)));
 assert(Array.isArray(contract.inputs)&&contract.inputs.length>0&&contract.inputs.length<=32);
 const receiptPath=resolve(sealedBuild.buildReceipt),receiptId=basename(dirname(receiptPath));
 assert(/^[a-f0-9-]{36}$/.test(receiptId));assert.equal(receiptPath,join(root,'.tmp/rust-disk/evidence',receiptId,'receipt.json'));
 const build=JSON.parse(readFileSync(receiptPath));assert.equal(build.state,'retired');assert.equal(build.handlesReaped,true);
 assert.deepEqual(build.command,sealedBuild.command);
 assert(build.command.some((value,index)=>value==='--example'&&build.command[index+1]==='emmy_native_context_probe'));
 assert.equal(sourceDigest(root),build.inputHashes.sourceTree);assert.equal(sealedBuild.sourceTree,build.inputHashes.sourceTree);
 for(const log of build.sealedLogs)assert.equal(hashFile(log.path),log.sha256);
 const commandLog=build.sealedLogs.find(log=>basename(log.path)==='command.json');assert(commandLog);
 const invocation=JSON.parse(readFileSync(commandLog.path));assert.equal(invocation.exit,0);assert.equal(resolve(invocation.cwd),root);
 assert.equal(invocation.inputHashes.sourceTree,sealedBuild.sourceTree);
 assert(/^[a-f0-9]{64}$/.test(invocation.environmentSHA256??'')&&/^[a-f0-9]{64}$/.test(sealedBuild.environmentSHA256??''));
 assert.equal(invocation.environmentSHA256,sealedBuild.environmentSHA256);
 const environment=sealedBuild.compileEnvironmentProof;assert(environment);
 assert.deepEqual(environment.compileDefine,{name:'LYCHEEDEV_EMMY_CONTEXT_PATCH_SHA256',value:frozen.record.patchSHA256});
 assert.equal(environment.expectedEnvironmentSHA256,invocation.environmentSHA256);
 assert.equal(environment.observedInvocationEnvironmentSHA256,invocation.environmentSHA256);assert.equal(environment.target,invocation.target);
 const executionIdentity={legacySnapshotScope:null,freezeSHA256:frozen.sha256,patchSHA256:frozen.record.patchSHA256,
  binarySHA256:sealedBuild.binarySHA256,pdbSHA256:sealedBuild.pdbSHA256,observedInvocationEnvironmentSHA256:invocation.environmentSHA256,compileDefine:environment.compileDefine};
 assert.deepEqual(executionIdentity,sealedBuild.executionIdentity);
 assert.equal(createHash('sha256').update(JSON.stringify(executionIdentity)).digest('hex'),sealedBuild.executionIdentitySHA256);
 const proofs=build.sealedLogs.filter(log=>log.path.endsWith('.receipt.json')).map(log=>JSON.parse(readFileSync(log.path)));
 assert(proofs.some(proof=>proof.exitCode===0&&proof.rootReaped&&proof.treeReaped&&proof.handlesClosed&&proof.activeProcesses===0));
 const fixtures=contract.inputs.map((input,index)=>{
  assert(typeof input.file==='string'&&typeof input.diskFile==='string');
  assert(!input.diskFile.split('/').some(part=>!part||part==='.'||part==='..')&&!input.diskFile.includes('\\'));
  const path=join(root,'tests/rust-source-engine/wow',input.diskFile);
  const frozenInput=frozen.record.inputs.find(row=>row.path===`tests/rust-source-engine/wow/${input.diskFile}`);assert(frozenInput);
  assert.equal(input.sha256,frozenInput.sha256);assert.equal(input.bytes,frozenInput.bytes);
  assert.equal(hashFile(path),input.sha256);return {...input,path,evidenceBasename:`fixture-${String(index).padStart(2,'0')}`};
 });
 assert.equal(new Set(fixtures.map(row=>row.file)).size,fixtures.length);
 mkdirSync(out);
 const binary=join(out,'emmy_native_context_probe.exe'),pdb=binary.replace(/\.exe$/,'.pdb');
 for(const [source,destination,hash] of [[sealedBuild.binary,binary,sealedBuild.binarySHA256],[sealedBuild.pdb,pdb,sealedBuild.pdbSHA256]]){
  assert.equal(hashFile(source),hash);copyFileSync(source,destination);assert.equal(hashFile(destination),hash);assert.equal(hashFile(source),hash);
 }
 const wrapperSource=readFileSync(join(root,'tools/rust-owned-process.ps1'),'utf8'),anchor='var limits=new LIMITS();limits.basic.flags=0x2000;Set(job,9,limits);';
 assert.equal(wrapperSource.split(anchor).length,2);const wrapper=join(out,'owned-native-context-2gib.ps1');
 writeFileSync(wrapper,wrapperSource.replace(anchor,'var limits=new LIMITS();limits.basic.flags=0x2000|0x200;limits.jobMemory=new UIntPtr(2147483648UL);Set(job,9,limits);'));
 copyFileSync(import.meta.filename,join(out,'runner-snapshot.mjs'));copyFileSync(contractFile,join(out,'fixed-contract.json'));
 const luac=join(root,'.tmp/rust-luals-suspended-release-01/payload/tool/luac/luac.exe'),runtime=JSON.parse(readFileSync(join(root,'release/tools/luac.json'))).runtime;
 assert.equal(runtime.version,'5.1.5');assert.equal(hashFile(luac),runtime.sha256);assert.equal(lstatSync(luac).size,runtime.bytes);
 const artifact={schema:'lycheedev.emmy-native-context-artifact.v1',binary,binarySHA256:sealedBuild.binarySHA256,pdb,pdbSHA256:sealedBuild.pdbSHA256,
  sealedBuild,sourceTree:sealedBuild.sourceTree,wrapperSHA256:hashFile(wrapper),runnerSHA256:hashFile(import.meta.filename),nodeSHA256:hashFile(process.execPath),
  contractSHA256:hashFile(contractFile),jobMemoryLimitBytes:2**31,totalDeadlineMs:30000,luac,runtime,fixtures,probeCommand,expectedRedFailures};
 writeFileSync(join(out,'artifact.json'),JSON.stringify(artifact,null,2)+'\n');
 const base=join(out,'process-'+randomUUID());
 const spec={command:process.execPath,args:[import.meta.filename,out,'--worker'],cwd:root,timeoutMs:30000,orphanMs:5000,cargoBuild:false,
  stdout:base+'.stdout.txt',stderr:base+'.stderr.txt',receipt:base+'.receipt.json'};
 writeFileSync(base+'.request.json',JSON.stringify(spec,null,2));
 const run=spawnSync('powershell.exe',['-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',wrapper,'-Request',base+'.request.json'],{cwd:root,windowsHide:true,timeout:55000,maxBuffer:1<<20});
 writeFileSync(join(out,'wrapper-result.json'),JSON.stringify({status:run.status,error:run.error?.message,stdout:run.stdout?.toString(),stderr:run.stderr?.toString()},null,2));
 let proof,worker,frames,error;
 try{
  proof=JSON.parse(readFileSync(spec.receipt));worker=JSON.parse(readFileSync(join(out,'worker.json')));
  const raw=readFileSync(join(out,'analysis.stdout.jsonl'));assert(raw.length>0&&raw.length<=4<<20&&raw.at(-1)===10);
  frames=raw.toString('utf8').trimEnd().split('\n').map(line=>JSON.parse(line));
 }catch(e){error=String(e);}
 const handlesReaped=run.status===0&&!run.error&&proof?.rootReaped===true&&proof.treeReaped===true&&proof.handlesClosed===true&&proof.toolIdentityLocksClosed===true&&proof.outputReadersComplete===true&&proof.activeProcesses===0;
 let contextStable=true;try{assert.deepEqual(verifiedContext(sealedBuild.contextIdentity),frozen);}catch(e){contextStable=false;error??=String(e);}
 const artifactStable=contextStable&&hashFile(binary)===artifact.binarySHA256&&hashFile(pdb)===artifact.pdbSHA256&&hashFile(wrapper)===artifact.wrapperSHA256&&hashFile(contractFile)===artifact.contractSHA256;
 const assertions=frames?.filter(frame=>frame.event==='assertion')??[],terminal=frames?.at(-1),failed=assertions.filter(row=>row.passed!==true);
 let assertionContractComplete=false;try{
 const expectedAssertions=contract.assertions.map(row=>{
  if(!row.expectedRef)return row;
  assert.equal(row.expectedRef.kind,'snapshotKey');assert(contract.scenarios.includes(row.expectedRef.scenario));
  const scenario=frames?.find(frame=>frame.event==='scenario'&&frame.name===row.expectedRef.scenario);
  assert(scenario&&typeof scenario.graph?.snapshot_inputs_json==='string');
  const inputs=JSON.parse(scenario.graph.snapshot_inputs_json);
  assert.equal(inputs.engine.patchSHA256,frozen.record.patchSHA256);
  assert.equal(inputs.adapter.rulesVersion,contract.rulesVersion);assert.equal(inputs.adapter.capabilityProfile,contract.capabilityProfile);
  assert.deepEqual(inputs.files,fixtures.map(input=>({file:input.file,bytes:input.bytes,sha256:input.sha256})));
  const order=probeCommand==='object-red'?['profile.lua','generic-replace.lua','generic-defs.lua','generic-calls.lua','object-versions.lua']:probeCommand==='coverage-red'?fixtures.map(row=>row.file):row.expectedRef.scenario==='after-b'?['profile.lua','generic-replace.lua','generic-defs.lua','generic-calls.lua']:['profile.lua','generic-defs.lua','generic-calls.lua','generic-replace.lua'];
  assert.deepEqual(inputs.orderedScriptLoadOccurrences,order.map((file,index)=>({file,index})));
  assert.equal(inputs.executionPhase,row.expectedRef.scenario==='unknown-a'?'unknown':'after_load');
  if(probeCommand==='coverage-red')assert.equal(inputs.validationScope,'fixed-return-unknown-argument-v1');
  if(probeCommand==='object-red')assert.equal(inputs.validationScope,'object-version-structure-red-v1');
  const snapshotKey=createHash('sha256').update(scenario.graph.snapshot_inputs_json).digest('hex');
  assert.equal(snapshotKey,scenario.snapshotKey);assert.equal(snapshotKey,scenario.graph.snapshot_key);
  return {name:row.name,expected:snapshotKey};
 });
 assert.deepEqual(assertions.map(row=>({name:row.name,expected:row.expected})),expectedAssertions);assertionContractComplete=true;
 }catch(e){error??=String(e);}
 const inputFrames=frames?.filter(frame=>frame.event==='input_identity')??[],scenarios=frames?.filter(frame=>frame.event==='scenario')??[];
 const inputIdentityComplete=inputFrames.length===fixtures.length&&fixtures.every(input=>inputFrames.some(frame=>frame.file===input.file&&frame.bytes===input.bytes&&frame.sha256===input.sha256))&&new Set(inputFrames.map(frame=>frame.file)).size===fixtures.length;
 const buildFrames=frames?.filter(frame=>frame.event==='build_identity')??[];
 const compiledIdentityComplete=buildFrames.length===1&&buildFrames[0].compiledPatchSHA256===frozen.record.patchSHA256;
 const frameContractComplete=frames?.every(frame=>frame.schema==='lycheedev.emmy-native-context-probe.v1')===true&&frames.filter(frame=>frame.event==='end').length===1&&terminal?.event==='end'&&terminal.complete===true&&terminal.fullSourceAcceptance===false&&terminal.error===null&&terminal.checked===assertions.length&&terminal.failed===failed.length&&JSON.stringify(scenarios.map(row=>row.name))===JSON.stringify(contract.scenarios);
 const transportComplete=handlesReaped&&!error&&artifactStable&&inputIdentityComplete&&compiledIdentityComplete&&frameContractComplete;
 const reportedAssertionsPassed=assertions.length>0&&assertions.every(row=>row.passed===true&&JSON.stringify(row.actual)===JSON.stringify(row.expected));
 const syntaxComplete=worker?.syntax?.length===fixtures.length&&worker.syntax.every(row=>row.passed&&row.sourceExecuted===false);
 const businessPassed=transportComplete&&assertionContractComplete&&reportedAssertionsPassed&&proof.exitCode===0&&worker?.analysis?.exit===0&&!worker.analysis.error;
 const expectedRedVerified=expectedRedFailures.length>0&&transportComplete&&assertionContractComplete&&proof.exitCode===1&&worker?.analysis?.exit===1&&!worker.analysis.error&&JSON.stringify(failed.map(row=>row.name))===JSON.stringify(expectedRedFailures)&&assertions.filter(row=>row.passed===true).every(row=>JSON.stringify(row.actual)===JSON.stringify(row.expected));
 const passed=(expectedRedFailures.length>0?expectedRedVerified:businessPassed)&&syntaxComplete;
 const report={schema:'lycheedev.emmy-native-context-owned.v1',passed,businessPassed,expectedRedVerified,expectedRedFailures,artifact,process:proof,worker,handlesReaped,artifactStable,inputIdentityComplete,
  compiledIdentityComplete,frameContractComplete,transportComplete,assertionContractComplete,reportedAssertionsPassed,syntaxComplete,assertions,terminal,frames,error,
  independentSemanticReview:'required',scope:'isolated typed-call profile and fixed expectations; not full Source, final engine selection or complete prototype acceptance'};
 writeFileSync(join(out,'report.json'),JSON.stringify(report,null,2)+'\n');
 console.log(JSON.stringify({report:join(out,'report.json'),passed,assertions:assertions.length,handlesReaped,error}));if(!passed)process.exitCode=1;
}
