// Owned native query execution and independent source-derived result contract.
import assert from 'node:assert/strict';
import {copyFileSync,existsSync,readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import {join,resolve,sep,dirname} from 'node:path';
import {spawn,spawnSync} from 'node:child_process';
import {randomUUID} from 'node:crypto';
import {hashFile,sourceDigest} from '../../tools/rust-build.mjs';
import {verifyFrozenContext} from './context-identity.mjs';
import {queryProbeContract} from './query-facade-contract.mjs';
const root=resolve('.'),[output,buildName]=process.argv.slice(2),out=resolve(output);
if(buildName==='--worker'){
 const a=JSON.parse(readFileSync(join(out,'artifact.json')));const began=Date.now();
 async function child(command,args,input,max){
  const process=spawn(command,args,{cwd:root,windowsHide:true,stdio:['pipe','pipe','pipe']});let stdout=[],stderr=[],bytes=0,error;
  for(const[chunks,stream]of[[stdout,process.stdout],[stderr,process.stderr]])stream.on('data',chunk=>{bytes+=chunk.length;if(bytes>max){error='query.output_budget';process.kill();}else chunks.push(chunk);});
  process.stdin.on('error',e=>{error??=String(e)});process.stdin.end(input);
  const timer=setTimeout(()=>{error='query.deadline';process.kill()},Math.max(1,25000-(Date.now()-began)));
  const exit=await new Promise(done=>{process.once('error',e=>{error=String(e);done(null)});process.once('close',done)});clearTimeout(timer);return{exit,error,stdout:Buffer.concat(stdout).toString('utf8'),stderr:Buffer.concat(stderr).toString('utf8')};
 }
 assert.equal(hashFile(a.binary),a.binarySHA256);const analysis=await child(a.binary,[],null,2**21);writeFileSync(join(out,'analysis.stdout.jsonl'),analysis.stdout);writeFileSync(join(out,'analysis.stderr.txt'),analysis.stderr);
 const syntax=[];for(const row of a.contract.inputs){const path=join(dirname(a.sealedBuild.binary),'inputs',row.diskPath);assert.equal(hashFile(path),row.sha256);const result=await child(a.luac,['-p','-'],readFileSync(path),65536);syntax.push({...row,...result,sourceExecuted:false,passed:result.exit===0&&!result.error});}
 writeFileSync(join(out,'worker.json'),JSON.stringify({analysis:{exit:analysis.exit,error:analysis.error},syntax},null,2));process.exitCode=analysis.exit;
}else{
 assert(output&&buildName&&out.startsWith(root+sep)&&!existsSync(out));
 const b=JSON.parse(readFileSync(buildName));assert.equal(b.example,'emmy_source_query_probe');assert.equal(b.sameInvocation,true);assert.equal(sourceDigest(root),b.sourceTree);
 const frozen=verifyFrozenContext(root,b.contextIdentity.path);assert.equal(frozen.sha256,b.contextIdentity.sha256);
 const receipt=JSON.parse(readFileSync(b.buildReceipt));assert.equal(receipt.state,'retired');assert.equal(receipt.handlesReaped,true);assert.deepEqual(receipt.command,b.command);assert.equal(receipt.inputHashes.sourceTree,b.sourceTree);for(const log of receipt.sealedLogs)assert.equal(hashFile(log.path),log.sha256);
 const contract=queryProbeContract(root);for(const row of contract.inputs){const input=frozen.record.inputs.find(x=>x.path===row.diskPath);assert(input&&input.sha256===row.sha256&&input.bytes===row.bytes);}
 mkdirSync(out);const binary=join(out,'emmy_source_query_probe.exe'),pdb=join(out,'emmy_source_query_probe.pdb');for(const[from,to,sha]of[[b.binary,binary,b.binarySHA256],[b.pdb,pdb,b.pdbSHA256]]){assert.equal(hashFile(from),sha);copyFileSync(from,to);assert.equal(hashFile(to),sha);}
 const wrapperText=readFileSync(join(root,'tools/rust-owned-process.ps1'),'utf8'),anchor='var limits=new LIMITS();limits.basic.flags=0x2000;Set(job,9,limits);';assert.equal(wrapperText.split(anchor).length,2);const wrapper=join(out,'owned-query-2gib.ps1');writeFileSync(wrapper,wrapperText.replace(anchor,'var limits=new LIMITS();limits.basic.flags=0x2000|0x200;limits.jobMemory=new UIntPtr(2147483648UL);Set(job,9,limits);'));
 const luac=join(root,'.tmp/rust-luals-suspended-release-01/payload/tool/luac/luac.exe'),runtime=JSON.parse(readFileSync(join(root,'release/tools/luac.json'))).runtime;assert.equal(hashFile(luac),runtime.sha256);assert.equal(runtime.version,'5.1.5');
 copyFileSync(import.meta.filename,join(out,'runner-snapshot.mjs'));writeFileSync(join(out,'fixed-contract.json'),JSON.stringify(contract,null,2));
 const a={schema:'lycheedev.source-query-artifact.v1',sealedBuild:b,binary,binarySHA256:b.binarySHA256,pdb,pdbSHA256:b.pdbSHA256,contract,luac,runtime,wrapperSHA256:hashFile(wrapper),jobMemoryLimitBytes:2**31,totalDeadlineMs:30000};writeFileSync(join(out,'artifact.json'),JSON.stringify(a,null,2));
 const base=join(out,'process-'+randomUUID()),spec={command:process.execPath,args:[import.meta.filename,out,'--worker'],cwd:root,timeoutMs:30000,orphanMs:5000,cargoBuild:false,stdout:base+'.stdout.bin',stderr:base+'.stderr.bin',receipt:base+'.receipt.json'};writeFileSync(base+'.request.json',JSON.stringify(spec,null,2));
 const run=spawnSync('powershell.exe',['-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',wrapper,'-Request',base+'.request.json'],{cwd:root,windowsHide:true,timeout:55000,maxBuffer:1<<20});writeFileSync(join(out,'wrapper-result.json'),JSON.stringify({status:run.status,error:run.error?.message,stdout:run.stdout?.toString(),stderr:run.stderr?.toString()},null,2));
 let proof,frames,worker,error;try{proof=JSON.parse(readFileSync(spec.receipt));worker=JSON.parse(readFileSync(join(out,'worker.json')));const bytes=readFileSync(join(out,'analysis.stdout.jsonl'));assert(bytes.length>0&&bytes.length<=2**21&&bytes.at(-1)===10);frames=bytes.toString('utf8').trimEnd().split('\n').map(line=>JSON.parse(line));}catch(e){error=String(e);}
 const checks=[];function check(name,expected,actual){let passed=true,message;try{assert.deepEqual(actual,expected)}catch(e){passed=false;message=e.message}checks.push({name,expected,actual,passed,error:message});}
 const handlesReaped=run.status===0&&!run.error&&proof?.rootReaped===true&&proof.treeReaped===true&&proof.handlesClosed===true&&proof.toolIdentityLocksClosed===true&&proof.outputReadersComplete===true&&proof.activeProcesses===0;
 check('native owner/process/handles terminal',true,handlesReaped);check('complete clean execution',0,proof?.exitCode);check('matching EXE',b.binarySHA256,hashFile(binary));check('matching PDB',b.pdbSHA256,hashFile(pdb));check('fixed source tree',b.sourceTree,sourceDigest(root));check('frozen full input/patch closure',frozen.sha256,verifyFrozenContext(root,b.contextIdentity.path).sha256);
 const assertions=frames?.filter(x=>x.event==='assertion')??[];check('all exact external assertion names',contract.assertions.map(x=>x.name),assertions.map(x=>x.name));
 for(let i=0;i<contract.assertions.length;i++){const expected=contract.assertions[i],actual=assertions[i];if(expected.expected?.relation)check(expected.name,actual?.expected,actual?.actual);else{check(expected.name+' fixed expected',expected.expected,actual?.expected);check(expected.name,expected.expected,actual?.actual);}}
 const identities=frames?.filter(x=>x.event==='input_identity')??[];check('all seven exact source byte identities',contract.inputs.map(({file,bytes,sha256})=>({file,bytes,sha256})).sort((a,b)=>a.file.localeCompare(b.file)),identities.map(({file,bytes,sha256})=>({file,bytes,sha256})).sort((a,b)=>a.file.localeCompare(b.file)));
 check('compiled patch',frozen.record.patchSHA256,frames?.find(x=>x.event==='build_identity')?.compiledPatchSHA256);check('actual Lua51 compiler and no source execution',true,worker?.syntax?.length===contract.inputs.length&&worker.syntax.every(x=>x.passed&&x.sourceExecuted===false));
 const paging=frames?.find(x=>x.event==='paging');check('independent full page equals all continued rows',paging?.full.rows.map(x=>x.id),paging?.joined.map(x=>x.id));check('independent page union has no duplicates',paging?.joined.length,new Set(paging?.joined.map(x=>x.id)).size);
 check('honest complete terminal',[{complete:true,error:null,checked:contract.assertions.length,failed:0,fullSourceAcceptance:false}],frames?.filter(x=>x.event==='end').map(({complete,error,checked,failed,fullSourceAcceptance})=>({complete,error,checked,failed,fullSourceAcceptance})));
 const report={schema:'lycheedev.source-query-owned.v1',passed:!error&&checks.every(x=>x.passed),artifact:a,process:proof,handlesReaped,checks,frames,worker,error,fullSourceAcceptance:false};writeFileSync(join(out,'report.json'),JSON.stringify(report,null,2));console.log(JSON.stringify({report:join(out,'report.json'),passed:report.passed,assertions:assertions.length,checks:checks.length,handlesReaped,error}));if(!report.passed)process.exitCode=1;
}
