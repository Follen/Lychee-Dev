// Owned execution, fixed Lua51 stdin and independent shared-fact checks.
import assert from 'node:assert/strict';
import {copyFileSync,existsSync,mkdirSync,readFileSync,writeFileSync} from 'node:fs';
import {dirname,join,resolve,sep} from 'node:path';
import {spawn,spawnSync} from 'node:child_process';
import {randomUUID} from 'node:crypto';
import {hashFile,sourceDigest} from '../../tools/rust-build.mjs';
import {verifyFrozenContext} from './context-identity.mjs';
import {hostFactsContract} from './host-facts-contract.mjs';
import {verifyHostRuntime} from './host-runtime-independent.mjs';
import {entryCases,entryInputCases,entryAssertions,verifyEntryRuntime} from './entry-runtime-contract.mjs';
import {boundaryInputs,boundaryAssertions,verifyEntryBoundaries} from './entry-boundary-contract.mjs';
const root=resolve('.'),[output,buildName,observerFreezePath]=process.argv.slice(2),out=resolve(output);
const names=['direct','shadow','colon','slots','transfers','phi','open-arity','unknown-effects','capture-mutation','cardinality','return-join','unknown-tail'];
if(buildName==='--worker'){
 const a=JSON.parse(readFileSync(join(out,'artifact.json'))),started=Date.now();
 async function child(command,args,input,max){
  const left=25000-(Date.now()-started);assert(left>0,'shared worker deadline');
  const running=spawn(command,args,{cwd:root,windowsHide:true,stdio:['pipe','pipe','pipe']});
  const stdout=[],stderr=[];let bytes=0,error;
  for(const[chunks,stream]of[[stdout,running.stdout],[stderr,running.stderr]])stream.on('data',part=>{bytes+=part.length;if(bytes>max){error='host-runtime.output_budget';running.kill();}else chunks.push(part);});
  running.stdin.on('error',e=>{error??=String(e);});running.stdin.end(input);
  const timer=setTimeout(()=>{error='host-runtime.deadline';running.kill();},left);
  const exit=await new Promise(done=>{running.once('error',e=>{error=String(e);done(null);});running.once('close',done);});clearTimeout(timer);
  return{exit,error,stdout:Buffer.concat(stdout).toString('utf8'),stderr:Buffer.concat(stderr).toString('utf8')};
 }
 assert.equal(hashFile(a.binary),a.binarySHA256);assert.equal(hashFile(a.luac),a.runtime.sha256);
 const analysis=await child(a.binary,[],null,2**21);writeFileSync(join(out,'analysis.stdout.jsonl'),analysis.stdout);writeFileSync(join(out,'analysis.stderr.txt'),analysis.stderr);
 const boundaries=await child(a.binary,['--entry-boundaries'],null,2**21);writeFileSync(join(out,'entry-boundaries.stdout.jsonl'),boundaries.stdout);writeFileSync(join(out,'entry-boundaries.stderr.txt'),boundaries.stderr);
 const syntax=[];
 for(const row of a.inputs){const path=join(dirname(a.sealedBuild.binary),'inputs',row.diskPath);assert.equal(hashFile(path),row.sha256);const result=await child(a.luac,['-p','-'],readFileSync(path),65536);syntax.push({...row,...result,sourceExecuted:false,passed:result.exit===0&&!result.error&&result.stdout===''});}
 writeFileSync(join(out,'worker.json'),JSON.stringify({analysis:{exit:analysis.exit,error:analysis.error},boundaries:{exit:boundaries.exit,error:boundaries.error},syntax,elapsedMs:Date.now()-started},null,2));
 if(analysis.exit!==0||analysis.error||boundaries.exit!==0||boundaries.error||!syntax.every(x=>x.passed))process.exitCode=1;
}else{
 assert(output&&buildName&&out.startsWith(root+sep)&&!existsSync(out));
 const b=JSON.parse(readFileSync(buildName));assert.equal(b.example,'emmy_source_host_execution_probe');assert.equal(b.sameInvocation,true);assert.equal(sourceDigest(root),b.sourceTree);
 const frozen=verifyFrozenContext(root,observerFreezePath??b.contextIdentity.path);
 let observerIdentity;
 if(observerFreezePath){
  const originalPath=join(dirname(b.binary),'frozen-context.json');
  assert.equal(hashFile(originalPath),b.contextIdentity.sha256,'sealed original compile manifest');
  const original=JSON.parse(readFileSync(originalPath));
  for(const key of ['baseIdentity','fork','changes','patchSHA256','rulesVersion'])assert.deepEqual(frozen.record[key],original[key],'observer cannot change engine/patch');
  assert.deepEqual(frozen.record.inputs.map(x=>x.path),original.inputs.map(x=>x.path),'observer cannot alter the input closure');
  const allowed=new Set(['tests/rust-source-engine/host-runtime-owned.mjs','tests/rust-source-engine/entry-runtime-contract.mjs']);
  const changed=[];
  for(let i=0;i<original.inputs.length;i++)if(JSON.stringify(original.inputs[i])!==JSON.stringify(frozen.record.inputs[i])){
   assert(allowed.has(original.inputs[i].path),'no Rust/example/material/fixture compile input may change');changed.push(original.inputs[i].path);
  }
  assert(changed.length>0);
  observerIdentity={path:frozen.path,sha256:frozen.sha256,originalCompileManifestSHA256:b.contextIdentity.sha256,changedObserverFiles:changed,
   scope:'same compiled Rust/patch/Lua/material bytes; only external observer serialization/custody logic changed'};
 }else assert.equal(frozen.sha256,b.contextIdentity.sha256);
 const receipt=JSON.parse(readFileSync(b.buildReceipt));assert.equal(receipt.state,'retired');assert.equal(receipt.handlesReaped,true);assert.deepEqual(receipt.command,b.command);assert.equal(receipt.inputHashes.sourceTree,b.sourceTree);
 for(const log of receipt.sealedLogs)assert.equal(hashFile(log.path),log.sha256);
 const contract=hostFactsContract(root),cases=contract.cases.filter(x=>names.includes(x.name));assert.equal(cases.length,names.length);
 const syntaxCases=contract.cases.filter(x=>names.includes(x.name)||entryInputCases.includes(x.name));
 const inputs=[...contract.inputs.filter(x=>syntaxCases.some(c=>c.files.includes(x.file))),...boundaryInputs(root)];
 for(const row of inputs){const f=frozen.record.inputs.find(x=>x.path===row.diskPath);assert(f&&f.sha256===row.sha256&&f.bytes===row.bytes);}
 for(const name of ['tests/rust-source-engine/host-runtime-owned.mjs','tests/rust-source-engine/host-runtime-independent.mjs','tests/rust-source-engine/entry-runtime-contract.mjs','tests/rust-source-engine/entry-boundary-contract.mjs'])assert.equal(frozen.record.inputs.find(x=>x.path===name)?.sha256,hashFile(join(root,name)));
 mkdirSync(out);const binary=join(out,'emmy_source_host_execution_probe.exe'),pdb=binary.replace(/\.exe$/,'.pdb');
 for(const[from,to,sha]of[[b.binary,binary,b.binarySHA256],[b.pdb,pdb,b.pdbSHA256]]){assert.equal(hashFile(from),sha);copyFileSync(from,to);assert.equal(hashFile(to),sha);}
 const wrapperSource=readFileSync(join(root,'tools/rust-owned-process.ps1'),'utf8'),anchor='var limits=new LIMITS();limits.basic.flags=0x2000;Set(job,9,limits);';assert.equal(wrapperSource.split(anchor).length,2);
 const wrapper=join(out,'owned-host-runtime-2gib.ps1');writeFileSync(wrapper,wrapperSource.replace(anchor,'var limits=new LIMITS();limits.basic.flags=0x2000|0x200;limits.jobMemory=new UIntPtr(2147483648UL);Set(job,9,limits);'));
 copyFileSync(import.meta.filename,join(out,'runner-snapshot.mjs'));writeFileSync(join(out,'fixed-contract.json'),JSON.stringify({...contract,explicitEntryCases:entryCases,explicitEntryAssertions:entryAssertions()},null,2));
 const luac=join(root,'.tmp/rust-luals-suspended-release-01/payload/tool/luac/luac.exe'),runtime=JSON.parse(readFileSync(join(root,'release/tools/luac.json'))).runtime;assert.equal(hashFile(luac),runtime.sha256);
 const artifact={schema:'lycheedev.source-host-runtime-artifact.v1',sealedBuild:b,observerIdentity,binary,pdb,binarySHA256:b.binarySHA256,pdbSHA256:b.pdbSHA256,
  wrapperSHA256:hashFile(wrapper),runnerSHA256:hashFile(import.meta.filename),contractSHA256:hashFile(join(out,'fixed-contract.json')),inputs,luac,runtime,totalDeadlineMs:30000,jobMemoryLimitBytes:2**31};
 writeFileSync(join(out,'artifact.json'),JSON.stringify(artifact,null,2));
 const base=join(out,'process-'+randomUUID()),request={command:process.execPath,args:[import.meta.filename,out,'--worker'],cwd:root,timeoutMs:30000,orphanMs:5000,cargoBuild:false,stdout:base+'.stdout.txt',stderr:base+'.stderr.txt',receipt:base+'.receipt.json'};
 writeFileSync(base+'.request.json',JSON.stringify(request,null,2));
 const run=spawnSync('powershell.exe',['-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',wrapper,'-Request',base+'.request.json'],{cwd:root,windowsHide:true,timeout:55000,maxBuffer:1<<20});
 writeFileSync(join(out,'wrapper-result.json'),JSON.stringify({status:run.status,error:run.error?.message,stdout:run.stdout?.toString(),stderr:run.stderr?.toString()},null,2));
 let proof,worker,frames,boundaryFrames,error;
 try{proof=JSON.parse(readFileSync(request.receipt));worker=JSON.parse(readFileSync(join(out,'worker.json')));const bytes=readFileSync(join(out,'analysis.stdout.jsonl'));assert(bytes.length>0&&bytes.length<=2**21&&bytes.at(-1)===10);frames=bytes.toString('utf8').trimEnd().split('\n').map(x=>JSON.parse(x));}catch(e){error=String(e);}
 try{const bytes=readFileSync(join(out,'entry-boundaries.stdout.jsonl'));assert(bytes.length>0&&bytes.length<=2**21&&bytes.at(-1)===10);boundaryFrames=bytes.toString('utf8').trimEnd().split('\n').map(x=>JSON.parse(x));}catch(e){error??=String(e);}
 const checks=[];function check(name,expected,actual){let passed=true,message;try{assert.deepEqual(actual,expected);}catch(e){passed=false;message=e.message;}checks.push({name,expected,actual,passed,error:message});}
 const owned=run.status===0&&!run.error&&proof?.rootReaped===true&&proof.treeReaped===true&&proof.handlesClosed===true&&proof.toolIdentityLocksClosed===true&&proof.outputReadersComplete===true&&proof.activeProcesses===0;
 check('owned process terminal',true,owned);check('analysis exit',0,worker?.analysis.exit);check('boundary analysis exit',0,worker?.boundaries.exit);check('native process exit',0,proof?.exitCode);
 check('all original and explicit-entry Lua51 stdin checks',inputs.map(x=>x.file),worker?.syntax.map(x=>x.file));check('syntax complete without execution',true,worker?.syntax.every(x=>x.passed&&x.sourceExecuted===false));
 check('compiled patch',[frozen.record.patchSHA256],frames?.filter(x=>x.event==='build_identity').map(x=>x.compiledPatchSHA256));
 check('fixed source tree',b.sourceTree,sourceDigest(root));check('fixed EXE',b.binarySHA256,hashFile(binary));check('same-invocation symbols',b.pdbSHA256,hashFile(pdb));
 check('frame schema',true,frames?.every(x=>x.schema==='lycheedev.source-host-execution-probe.v1'));
 const scenarios=frames?.filter(x=>x.event==='scenario')??[],assertions=frames?.filter(x=>x.event==='assertion')??[];
 const expected=[];for(const c of names){const g=scenarios.find(x=>x.case===c)?.graph;for(const[key,value]of [['complete_bounded_analysis',true],['initial_environment_origins',7],['all_call_slots_recorded',g?.calls.length],['capture_reference_count',g?.native_invocations.capture_facts.length],['captures_not_value_transfers',true],['fresh_host_return_slots',true],['host_type_unavailable',true],['environment_no_source_snippet',true],['native_host_type_unknown',true],['same_control_cancel',7]])expected.push({name:c+':'+key,expected:value});if(!['unknown-effects','unknown-tail'].includes(c))expected.push({name:c+':nonretaining_host_calls',expected:0});}
 for(const[name,value]of [['transfers:factory_capture_cells_distinct',2],['transfers:factory_invocations_read_own_values',[{host:'Source',slot:1},{host:'Pair',slot:1}]],['capture-mutation:invocation_returns_current_nil',true],['capture-mutation:read_uses_latest_cell_write',true],['capture-mutation:consumer_receives_nil',true],['cardinality:bounded_zero_or_two_returns',true],['cardinality:bare_and_parenthesized_arguments',[[0,2],[1]]],['return-join:bounded_one_or_two_returns',[1,2]],['return-join:first_slot_preserves_both_host_sources',true],['return-join:candidate_path_polarities',[false,true]],['unknown-tail:bare_parenthesis_relay_arities',[null,[1],[2]]],['unknown-tail:relay_argument_tail_preserved',true],['unknown-tail:relay_missing_formal_unavailable',true],['unknown-tail:relay_returns_two_unknown_values',true]])expected.push({name,expected:value});
 const entryRows=frames?.filter(row=>row.event==='execution_scenario')??[];
 expected.push(...entryAssertions());
 for(const name of ['entry','entry-empty','entry-literal']){
  const candidate=entryRows.find(row=>row.case===name)?.candidates[0];
  const arguments_=name==='entry'?{kind:'unspecified'}:{kind:'exact',values:name==='entry-empty'?[]:[{kind:'boolean',value:true}]};
  expected.push({name:'execution:'+name+':request_in_snapshot',expected:{candidate:candidate?.id,load_occurrence:null,creation_context:null,arguments:arguments_}});
 }
 for(const [index,name]of [[0,'entry-exact-a'],[1,'entry-exact-b']])expected.push({name:'execution:'+name+':exact_source_closure',expected:entryRows.find(row=>row.case===name)?.candidates[index]?.closure});
 check('original assertion count retained',144,assertions.filter(row=>!row.name.startsWith('execution:')).length);
 check('entry assertion count',57,assertions.filter(row=>row.name.startsWith('execution:')).length);
 const sort=rows=>[...rows].sort((a,b)=>a.name.localeCompare(b.name));
 check('exact assertion contract',sort(expected),sort(assertions.map(({name,expected})=>({name,expected}))));
 for(const row of assertions){check(row.name,row.expected,row.actual);check(row.name+':reported',true,row.passed);}
 check('actual native host typing',names,frames?.filter(x=>x.event==='native_host_type'&&x.result.unknown===true&&x.result.type==='Unknown').map(x=>x.case));
 check('honest terminal',[{complete:true,error:null,checked:201,failed:0,fullSourceAcceptance:false,explicitEntryImplemented:true}],frames?.filter(x=>x.event==='end').map(({complete,error,checked,failed,fullSourceAcceptance,explicitEntryImplemented})=>({complete,error,checked,failed,fullSourceAcceptance,explicitEntryImplemented})));
 const boundaryChecks=boundaryFrames?.filter(frame=>frame.event==='assertion')??[];
 check('boundary compiled patch',[frozen.record.patchSHA256],boundaryFrames?.filter(frame=>frame.event==='build_identity').map(frame=>frame.compiledPatchSHA256));
 check('boundary frame schema',true,boundaryFrames?.every(frame=>frame.schema==='lycheedev.source-host-execution-probe.v1'));
 check('boundary assertion count',36,boundaryChecks.length);
 check('boundary exact assertion contract',sort(boundaryAssertions(boundaryFrames??[],root)),sort(boundaryChecks.map(({name,expected})=>({name,expected}))));
 for(const row of boundaryChecks){check(row.name,row.expected,row.actual);check(row.name+':reported',true,row.passed);}
 check('boundary honest terminal',[{mode:'entry-boundaries',complete:true,error:null,checked:36,failed:0,fullSourceAcceptance:false}],boundaryFrames?.filter(frame=>frame.event==='end').map(({mode,complete,error,checked,failed,fullSourceAcceptance})=>({mode,complete,error,checked,failed,fullSourceAcceptance})));
 let independent,entryIndependent,boundaryIndependent;try{independent=verifyHostRuntime(root,frames??[]);entryIndependent=verifyEntryRuntime(root,frames??[]);boundaryIndependent=verifyEntryBoundaries(root,boundaryFrames??[]);}catch(e){error??=String(e);}
 writeFileSync(join(out,'entry-boundaries-independent.json'),JSON.stringify(boundaryIndependent??{passed:false,error},null,2));
 writeFileSync(join(out,'entry-independent.json'),JSON.stringify(entryIndependent??{passed:false,error},null,2));
 writeFileSync(join(out,'independent.json'),JSON.stringify(independent??{passed:false,error},null,2));
 const report={schema:'lycheedev.source-host-runtime-owned.v1',passed:!error&&checks.every(x=>x.passed)&&independent?.passed===true&&entryIndependent?.passed===true&&boundaryIndependent?.passed===true,artifact,process:proof,handlesReaped:owned,worker,checks,frames,boundaryFrames,independent,entryIndependent,boundaryIndependent,error,
  scope:'fixed host/common facts and explicit entry/load-plan seam; guard/operation secrecy flow, all-order joins, actual API/full Retail and default migration remain unaccepted'};
 writeFileSync(join(out,'report.json'),JSON.stringify(report,null,2));console.log(JSON.stringify({report:join(out,'report.json'),passed:report.passed,assertions:assertions.length,boundaryAssertions:boundaryChecks.length,checks:checks.length,independentChecks:independent?.checks.length,entryIndependentChecks:entryIndependent?.checks.length,handlesReaped:owned,error}));if(!report.passed)process.exitCode=1;
}
