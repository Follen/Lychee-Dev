// Root-owned custody and immutable evidence for the isolated WoW binding example.
import assert from 'node:assert/strict';
import {copyFileSync,existsSync,mkdirSync,readFileSync,readdirSync,statSync,writeFileSync} from 'node:fs';
import {basename,dirname,join,resolve,sep} from 'node:path';
import {spawn,spawnSync} from 'node:child_process';
import {createHash,randomUUID} from 'node:crypto';
import {hashFile,sourceDigest} from '../../tools/rust-build.mjs';
import {verifyFrozenContext} from './context-identity.mjs';
import {originalV7Contract,originalAssertionContract} from './wow-v7-contract.mjs';

const root=resolve('.'),out=resolve(process.argv[2]),buildArtifactPath=process.argv[3];
if(buildArtifactPath==='--worker'){
 const artifact=JSON.parse(readFileSync(join(out,'artifact.json')));
 if(artifact.sealedBuild.contextIdentity){
  const context=verifyFrozenContext(root,artifact.sealedBuild.contextIdentity.path);
  assert.equal(context.sha256,artifact.sealedBuild.contextIdentity.sha256);
  assert.equal(context.record.patchSHA256,artifact.sealedBuild.contextIdentity.patchSHA256);
 }
 const began=Date.now();
 async function child(command,args,input,maxBytes){
  assert(Date.now()-began<25000,'worker total deadline');
  const process=spawn(command,args,{cwd:root,windowsHide:true,stdio:['pipe','pipe','pipe']});
  let stdout=[],stderr=[],bytes=0,error;
  for(const [chunks,stream] of [[stdout,process.stdout],[stderr,process.stderr]])stream.on('data',chunk=>{bytes+=chunk.length;if(bytes>maxBytes){error='child.output_budget';process.kill();}else chunks.push(chunk);});
  process.stdin.on('error',e=>{error??=String(e);});process.stdin.end(input);
  const timer=setTimeout(()=>{error='child.deadline';process.kill();},Math.min(5000,25000-(Date.now()-began)));
  const exit=await new Promise(done=>{process.once('error',e=>{error=String(e);done(null);});process.once('close',done);});clearTimeout(timer);
  return {args,exit,stdout:Buffer.concat(stdout).toString('utf8'),stderr:Buffer.concat(stderr).toString('utf8'),error};
 }
 assert.equal(hashFile(artifact.binary),artifact.binarySHA256);assert.equal(hashFile(artifact.luac),artifact.runtime.sha256);
 const analysis=await child(artifact.binary,['golden'],null,1<<20);
 writeFileSync(join(out,'analysis.stdout.jsonl'),analysis.stdout);writeFileSync(join(out,'analysis.stderr.txt'),analysis.stderr);
 const syntax=[];
 for(const fixture of artifact.fixtures){
  assert.equal(hashFile(fixture.path),fixture.sha256);assert.equal(statSync(fixture.path).size,fixture.bytes);
  const input=readFileSync(fixture.path),result=await child(artifact.luac,['-p','-'],input,65536);
  writeFileSync(join(out,fixture.name+'.stdin.lua'),input);writeFileSync(join(out,fixture.name+'.luac.stderr.txt'),result.stderr);
  syntax.push({...fixture,...result,sourceExecuted:false,passed:result.exit===0&&!result.error&&result.stdout===''});
 }
 writeFileSync(join(out,'worker.json'),JSON.stringify({analysis,syntax,elapsedMs:Date.now()-began},null,2));
 if(analysis.exit!==0||analysis.error||!syntax.every(s=>s.passed))process.exitCode=1;
}else{
assert(out.startsWith(root+sep)&&!existsSync(out),'new evidence directory must be inside current workspace');
const sealedBuild=JSON.parse(readFileSync(resolve(buildArtifactPath)));
assert.equal(sealedBuild.schema,'lycheedev.emmy-wow-build.v1');assert.equal(sealedBuild.sameInvocation,true);
let frozenContext;
if(sealedBuild.contextIdentity){
 frozenContext=verifyFrozenContext(root,sealedBuild.contextIdentity.path);
 assert.equal(frozenContext.sha256,sealedBuild.contextIdentity.sha256);
 assert.equal(frozenContext.record.patchSHA256,sealedBuild.contextIdentity.patchSHA256);
}
const expectRed=process.argv[4]==='--expect-red';assert(!expectRed||sealedBuild.matrixMode==='regressions');
const failureCases={
 'effect-mutation':'effect-mutation-afterload:mutation_reaches_later_field_call',
 'parallel-assignment':'parallel-assignment-afterload:all_lhs_addresses_captured_before_writes',
 'colon-call':'colon-call-afterload:unsupported_invocation_has_honest_check_coverage',
 'conditional-call':'conditional-call-afterload:unknown_branch_call_invalidates_field_proof',
};
const expectedRedCases=expectRed?(process.argv[5]??Object.keys(failureCases).join(',')).split(','):[];
assert(expectedRedCases.every(id=>Object.hasOwn(failureCases,id))&&new Set(expectedRedCases).size===expectedRedCases.length);
assert(!expectRed||expectedRedCases.length>0);
const receiptPath=resolve(sealedBuild.buildReceipt),receiptId=basename(dirname(receiptPath));
assert(/^[a-f0-9-]{36}$/.test(receiptId),'explicit governed build receipt required');
assert.equal(receiptPath,join(root,'.tmp/rust-disk/evidence',receiptId,'receipt.json'));
const build=JSON.parse(readFileSync(receiptPath));
assert.equal(build.state,'retired');assert.equal(build.handlesReaped,true);
const expectedExample='emmy_wow_binding_probe';
assert.equal(build.command[0],'cargo');assert.equal(build.command[1],'build');
assert(build.command.some((v,i)=>v==='--example'&&build.command[i+1]===expectedExample));
assert(build.command.some((v,i)=>v==='-p'&&build.command[i+1]==='lycheedev-source'));
assert(build.command.some((v,i)=>v==='--features'&&build.command[i+1]==='emmy-prototype'));
// Reject a build made from a different tree before touching the current binary.
assert.equal(sourceDigest(root),build.inputHashes.sourceTree);
assert.equal(sealedBuild.sourceTree,build.inputHashes.sourceTree);
assert.deepEqual(sealedBuild.command,build.command);
for(const log of build.sealedLogs)assert.equal(hashFile(log.path),log.sha256);
const commandLog=build.sealedLogs.find(log=>basename(log.path)==='command.json');assert(commandLog);
const invocation=JSON.parse(readFileSync(commandLog.path));
assert.equal(invocation.exit,0);assert.equal(resolve(invocation.cwd),root);
assert.equal(invocation.inputHashes.sourceTree,build.inputHashes.sourceTree);
assert(/^[a-f0-9]{64}$/.test(sealedBuild.environmentSHA256??'')&&/^[a-f0-9]{64}$/.test(invocation.environmentSHA256??''),'both environment digests are required');
assert.equal(sealedBuild.environmentSHA256,invocation.environmentSHA256,'sealed environment must match the actual Cargo invocation');
if(sealedBuild.contextIdentity){
 const environment=sealedBuild.compileEnvironmentProof;
 assert(environment&&environment.compileDefine.name==='LYCHEEDEV_EMMY_CONTEXT_PATCH_SHA256');
 assert.equal(environment.compileDefine.value,sealedBuild.contextIdentity.patchSHA256);
 assert.equal(environment.expectedEnvironmentSHA256,invocation.environmentSHA256);
 assert.equal(environment.observedInvocationEnvironmentSHA256,invocation.environmentSHA256);
 assert.equal(environment.target,invocation.target);
 const identity={legacySnapshotScope:'original-v7-primitive-profile',freezeSHA256:sealedBuild.contextIdentity.sha256,
  patchSHA256:sealedBuild.contextIdentity.patchSHA256,binarySHA256:sealedBuild.binarySHA256,pdbSHA256:sealedBuild.pdbSHA256,
  observedInvocationEnvironmentSHA256:invocation.environmentSHA256,compileDefine:environment.compileDefine};
 assert.deepEqual(identity,sealedBuild.executionIdentity);
 assert.equal(createHash('sha256').update(JSON.stringify(identity)).digest('hex'),sealedBuild.executionIdentitySHA256);
}
const buildProofs=build.sealedLogs.filter(log=>log.path.endsWith('.receipt.json')).map(log=>JSON.parse(readFileSync(log.path)));
assert(buildProofs.some(p=>p.exitCode===0&&p.rootReaped&&p.treeReaped&&p.handlesClosed&&p.activeProcesses===0));

mkdirSync(out);
const sourceBinary=sealedBuild.binary,sourcePdb=sealedBuild.pdb;
const binary=join(out,expectedExample+'.exe'),pdb=binary.replace(/\.exe$/,'.pdb');
const binarySHA256=hashFile(sourceBinary),pdbSHA256=hashFile(sourcePdb);
assert.equal(binarySHA256,sealedBuild.binarySHA256);assert.equal(pdbSHA256,sealedBuild.pdbSHA256);
copyFileSync(sourceBinary,binary);copyFileSync(sourcePdb,pdb);
assert.equal(hashFile(sourceBinary),binarySHA256);assert.equal(hashFile(sourcePdb),pdbSHA256);
assert.equal(hashFile(binary),binarySHA256);assert.equal(hashFile(pdb),pdbSHA256);
const sourceWrapper=readFileSync(join(root,'tools/rust-owned-process.ps1'),'utf8');
const anchor='var limits=new LIMITS();limits.basic.flags=0x2000;Set(job,9,limits);';
assert.equal(sourceWrapper.split(anchor).length,2);
const wrapper=join(out,'owned-wow-2gib.ps1');
writeFileSync(wrapper,sourceWrapper.replace(anchor,'var limits=new LIMITS();limits.basic.flags=0x2000|0x200;limits.jobMemory=new UIntPtr(2147483648UL);Set(job,9,limits);'));
writeFileSync(join(out,'runner-snapshot.mjs'),readFileSync(import.meta.filename));
const luac=join(root,'.tmp/rust-luals-suspended-release-01/payload/tool/luac/luac.exe');
const runtime=JSON.parse(readFileSync(join(root,'release/tools/luac.json'))).runtime;
assert.equal(runtime.version,'5.1.5');assert.equal(hashFile(luac),runtime.sha256);assert.equal(statSync(luac).size,runtime.bytes);
const fixtureRoot=join(root,'tests/rust-source-engine/wow');
const fixtures=readdirSync(fixtureRoot).filter(name=>name.endsWith('.lua')).sort().map(name=>({name,path:join(fixtureRoot,name),sha256:hashFile(join(fixtureRoot,name)),bytes:statSync(join(fixtureRoot,name)).size}));
assert.equal(fixtures.length,sealedBuild.matrixMode==='regressions'?11:7,'complete explicitly selected fixture matrix');
for(const fixture of fixtures){const built=sealedBuild.fixtures.find(row=>row.file===fixture.name);assert(built);assert.equal(fixture.sha256,built.sha256);assert.equal(fixture.bytes,built.bytes);}
const artifact={schema:'lycheedev.emmy-wow-artifact.v1',binary,binarySHA256,pdb,pdbSHA256,buildReceipt:receiptId,sealedBuild,sourceTree:build.inputHashes.sourceTree,wrapperSHA256:hashFile(wrapper),runnerSHA256:hashFile(import.meta.filename),nodeSHA256:hashFile(process.execPath),jobMemoryLimitBytes:2**31,totalDeadlineMs:30000,luac,runtime,fixtures,expectedRedCases};
writeFileSync(join(out,'artifact.json'),JSON.stringify(artifact,null,2)+'\n');
const base=join(out,'process-'+randomUUID());
const spec={command:process.execPath,args:[import.meta.filename,out,'--worker'],cwd:root,timeoutMs:30000,orphanMs:5000,cargoBuild:false,stdout:base+'.stdout.txt',stderr:base+'.stderr.txt',receipt:base+'.receipt.json'};
writeFileSync(base+'.request.json',JSON.stringify(spec,null,2));
const run=spawnSync('powershell.exe',['-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',wrapper,'-Request',base+'.request.json'],{cwd:root,windowsHide:true,timeout:55000,maxBuffer:1<<20});
writeFileSync(join(out,'wrapper-result.json'),JSON.stringify({status:run.status,error:run.error?.message,stdout:run.stdout?.toString(),stderr:run.stderr?.toString()},null,2));
let proof,frames,worker,error;
try{
 proof=JSON.parse(readFileSync(spec.receipt));
 worker=JSON.parse(readFileSync(join(out,'worker.json')));
 const raw=readFileSync(join(out,'analysis.stdout.jsonl'));assert(raw.length<=1<<20,'bounded stdout');assert(raw.length>0&&raw.at(-1)===10,'complete newline-delimited output');
 frames=raw.toString('utf8').trimEnd().split('\n').map(line=>JSON.parse(line));
}catch(e){error=String(e);}
const reaped=run.status===0&&!run.error&&proof?.rootReaped===true&&proof.treeReaped===true&&proof.handlesClosed===true&&proof.activeProcesses===0;
let contextStable=true;
if(frozenContext){try{assert.deepEqual(verifyFrozenContext(root,sealedBuild.contextIdentity.path),frozenContext);}catch(e){contextStable=false;error??=String(e);}}
const artifactStable=contextStable&&hashFile(binary)===binarySHA256&&hashFile(pdb)===pdbSHA256&&hashFile(wrapper)===artifact.wrapperSHA256;
const assertions=frames?.filter(f=>f.event==='assertion')??[];
const terminal=frames?.at(-1);
const inputFrames=frames?.filter(f=>f.event==='input_identity')??[];
const buildIdentityFrames=frames?.filter(f=>f.event==='build_identity')??[];
const compiledContextIdentityComplete=!sealedBuild.contextIdentity||(buildIdentityFrames.length===1&&buildIdentityFrames[0].compiledPatchSHA256===sealedBuild.contextIdentity.patchSHA256);
const expectedScenarios=['future-ab-unknown','future-ab-afterload','future-ba-afterload','mixins-afterload','shadow-afterload','unsupported-afterload'];
if(sealedBuild.matrixMode==='regressions')expectedScenarios.push('effect-mutation-afterload','parallel-assignment-afterload','colon-call-afterload','conditional-call-afterload');
const scenarios=frames?.filter(f=>f.event==='scenario')??[];
const inputIdentityComplete=inputFrames.length===fixtures.length&&new Set(inputFrames.map(f=>f.file)).size===fixtures.length&&fixtures.every(input=>inputFrames.some(frame=>frame.file===input.name&&frame.bytes===input.bytes&&frame.sha256===input.sha256));
const failedAssertions=assertions.filter(a=>a.passed!==true);
const assertionContractComplete=sealedBuild.matrixMode!=='regressions'||originalAssertionContract(assertions,originalV7Contract(root));
const frameContractComplete=frames?.every(frame=>frame.schema==='lycheedev.emmy-wow-binding-probe.v1')===true&&frames.filter(f=>f.event==='end').length===1&&terminal?.event==='end'&&terminal.complete===true&&terminal.businessBaselinePassed===(failedAssertions.length===0)&&terminal.fullSourceAcceptance===false&&terminal.error===null&&terminal.checked===assertions.length&&terminal.failed===failedAssertions.length&&scenarios.length===expectedScenarios.length&&JSON.stringify(scenarios.map(s=>s.name))===JSON.stringify(expectedScenarios)&&new Set(assertions.map(a=>a.name)).size===assertions.length;
const transportComplete=reaped&&!error&&artifactStable&&frameContractComplete&&inputIdentityComplete&&assertionContractComplete&&compiledContextIdentityComplete;
// These are probe-reported expectations. Root additionally inspects exact facts
// against the independent business contract before claiming semantic correctness.
const reportedAssertionsPassed=assertions.length>0&&assertions.every(f=>f.passed===true&&JSON.stringify(f.expected)===JSON.stringify(f.actual));
const syntaxComplete=worker?.syntax?.length===fixtures.length&&worker.syntax.every(s=>s.passed&&s.sourceExecuted===false);
const expectedFailures=expectedRedCases.map(id=>failureCases[id]);
const expectedRedVerified=expectRed&&transportComplete&&proof.exitCode===1&&worker?.analysis?.exit===1&&!worker.analysis.error&&assertions.length===86&&JSON.stringify(failedAssertions.map(a=>a.name))===JSON.stringify(expectedFailures)&&assertions.filter(a=>a.passed===true).every(a=>JSON.stringify(a.expected)===JSON.stringify(a.actual));
const businessPassed=transportComplete&&proof.exitCode===0&&worker?.analysis?.exit===0&&!worker.analysis.error&&reportedAssertionsPassed;
const passed=syntaxComplete&&(expectRed?expectedRedVerified:businessPassed);
const report={schema:'lycheedev.emmy-wow-owned.v1',passed,businessPassed,expectedRedVerified,expectedRedCases,artifact,process:proof,worker,syntaxComplete,handlesReaped:reaped,artifactStable,frameContractComplete,inputIdentityComplete,assertionContractComplete,compiledContextIdentityComplete,transportComplete,reportedAssertionsPassed,terminal,assertions,frames,error,independentSemanticReview:'required',scope:expectRed?'expected native reproduction of explicitly selected precise failures; passing reproduction is NOT passing business behavior; the entire 86-assertion matrix still runs':'isolated bounded WoW graph prototype; probe assertions, actual bundled-luac stdin compilation and custody, not full Source or final engine selection'};
writeFileSync(join(out,'report.json'),JSON.stringify(report,null,2)+'\n');
console.log(JSON.stringify({report:join(out,'report.json'),passed,assertions:assertions.length,handlesReaped:reaped,error}));
if(!passed)process.exitCode=1;
}
