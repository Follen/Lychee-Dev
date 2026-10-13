// Full fixed-input syntax authority, bounded parallel stdin compilation only.
import assert from 'node:assert/strict';
import {resolve,join} from 'node:path';
import {readFileSync,writeFileSync,mkdirSync,existsSync,copyFileSync,lstatSync} from 'node:fs';
import {spawn,spawnSync} from 'node:child_process';
import {randomUUID,createHash} from 'node:crypto';
import {performance} from 'node:perf_hooks';
import {hashFile} from '../../tools/rust-build.mjs';
const root=resolve('.'),out=resolve(process.argv[2]),sealed=resolve(process.argv[3]??'.tmp/rust-emmy-xml-08');
const selection=process.argv[4]??'all';assert(['all','xml'].includes(selection));
const identity=JSON.parse(readFileSync(join(sealed,'artifact.json'))),exe=join(out,'emmy_xml_program.exe');
const luac=identity.luac,manifestPath=join(root,'.tmp/rust-source-independent-retail-01/input/manifest.json');
const sourceRoot=join(root,'.tmp/gob01/source/v1/worktrees/wow-ui-source/31c7f7b9cc79e56c986b365c06a6afbcf3c9177b');
const sha=bytes=>createHash('sha256').update(bytes).digest('hex');
const sourceDigest='88d68f2c4bdb352245cbcb53cd41ec906b294e2ea599ee6374e00be169be7928';
assert.equal(hashFile(luac),identity.runtime.sha256);
if(!process.argv.includes('--worker')){
 assert(out.startsWith(root+'\\')&&!existsSync(out));mkdirSync(out);assert.equal(hashFile(identity.binary),identity.binarySHA256);assert.equal(hashFile(identity.pdb),identity.pdbSHA256);
 copyFileSync(identity.binary,exe);copyFileSync(identity.pdb,exe.replace(/\.exe$/,'.pdb'));
 const template=readFileSync(join(root,'tools/rust-owned-process.ps1'),'utf8'),anchor='var limits=new LIMITS();limits.basic.flags=0x2000;Set(job,9,limits);';assert.equal(template.split(anchor).length,2);
 const wrapper=join(out,'owned-full-syntax-2gib.ps1');writeFileSync(wrapper,template.replace(anchor,'var limits=new LIMITS();limits.basic.flags=0x2000|0x200;limits.jobMemory=new UIntPtr(2147483648UL);Set(job,9,limits);'));
 writeFileSync(join(out,'artifact.json'),JSON.stringify({...identity,binary:exe,pdb:exe.replace(/\.exe$/,'.pdb'),runnerSHA256:hashFile(import.meta.filename),wrapperSHA256:hashFile(wrapper),manifestSHA256:hashFile(manifestPath)},null,2));writeFileSync(join(out,'runner-snapshot.mjs'),readFileSync(import.meta.filename));
 const base=join(out,'process-'+randomUUID()),spec={command:process.execPath,args:[import.meta.filename,out,sealed,selection,'--worker'],cwd:root,timeoutMs:90000,orphanMs:5000,cargoBuild:false,stdout:base+'.stdout.txt',stderr:base+'.stderr.txt',receipt:base+'.receipt.json'};
 writeFileSync(base+'.request.json',JSON.stringify(spec,null,2));const run=spawnSync('powershell.exe',['-NoProfile','-NonInteractive','-File',wrapper,'-Request',base+'.request.json'],{cwd:root,windowsHide:true,timeout:125000,maxBuffer:1<<20});
 let proof,worker,error;try{proof=JSON.parse(readFileSync(spec.receipt));worker=JSON.parse(readFileSync(join(out,'worker.json')));}catch(e){error=String(e);}
 const reaped=run.status===0&&proof?.rootReaped&&proof.treeReaped&&proof.handlesClosed&&proof.activeProcesses===0;
 const report={schema:'lycheedev.source-full-syntax.owned.v1',selection,passed:reaped&&!error&&proof.exitCode===0&&worker?.passed===true,worker,proof,error,handlesReaped:reaped,scope:'all fixed input bytes authenticated; selected Lua/XML sources via bundled luac stdin; XML selection is a parser-change regression following the preserved full all-sources run, not smaller acceptance input'};
 assert.equal(hashFile(exe),identity.binarySHA256);assert.equal(hashFile(luac),identity.runtime.sha256);
 writeFileSync(join(out,'report.json'),JSON.stringify(report,null,2));console.log(JSON.stringify({report:join(out,'report.json'),passed:report.passed,handlesReaped:reaped,summary:worker?.summary,error}));if(!report.passed)process.exitCode=1;
}else{
 const started=performance.now(),deadline=started+85000,records=[],xmlRecords=[],profileDiagnostics=[];
 let documentsVerified=0,luaFiles=0,luaBytes=0,xmlFiles=0,xmlScripts=0,sourceBytes=0,error;
 const running=new Set();let compileOutputs=0,asyncFailure;
 async function child(command,args,input,maxBytes){
  if(performance.now()>=deadline)throw new Error('syntax.total_deadline');
  const proc=spawn(command,args,{cwd:root,windowsHide:true,stdio:['pipe','pipe','pipe'],env:{...process.env,LUA_PATH:'',LUA_CPATH:'',LUA_INIT:'',LUA_INIT_5_1:''}});
  let stdout=[],stderr=[],bytes=0,failure;
  for(const [list,stream]of [[stdout,proc.stdout],[stderr,proc.stderr]])stream.on('data',chunk=>{bytes+=chunk.length;if(bytes>maxBytes){failure='syntax.child_output_budget';proc.kill();return;}list.push(chunk);});
  proc.stdin.on('error',e=>failure=String(e));proc.stdin.end(input);
  const timer=setTimeout(()=>{failure='syntax.child_deadline';proc.kill();},Math.max(1,Math.min(5000,deadline-performance.now())));
  const exit=await new Promise(done=>{proc.once('error',e=>{failure=String(e);done(null);});proc.once('close',done);});clearTimeout(timer);
  return {pid:proc.pid,exit,stdout:Buffer.concat(stdout),stderr:Buffer.concat(stderr),failure};
 }
 function admit(file){
  assert(typeof file.path==='string'&&!file.path.includes('\\')&&!file.path.split('/').some(p=>p==='..'||p==='.'||p==='')&&!/^[A-Za-z]:|^\//.test(file.path));
  const path=join(sourceRoot,file.path);let cursor=path;
  while(cursor!==sourceRoot){assert(!lstatSync(cursor).isSymbolicLink());cursor=resolve(cursor,'..');assert(cursor.startsWith(sourceRoot));}
  const bytes=readFileSync(path);assert(bytes.length===file.bytes&&sha(bytes)===file.sha256);return {path,bytes};
 }
 async function compile(spec){
  const result=await child(luac,['-p','-'],spec.bytes,65536);compileOutputs+=result.stdout.length+result.stderr.length;
  if(compileOutputs>1<<20)throw new Error('syntax.compiler_output_total_budget');
  const record={kind:spec.kind,path:spec.path,scriptId:spec.scriptId,index:spec.index,originalSHA256:spec.originalSHA256,inputSHA256:sha(spec.bytes),inputBytes:spec.bytes.length,prefixBytes:spec.prefixBytes??0,sourceMapSHA256:spec.sourceMapSHA256,pid:result.pid,exit:result.exit,stdoutBytes:result.stdout.length,stderr:result.stderr.toString(),failure:result.failure,passed:!result.failure&&result.exit===0&&result.stdout.length===0};
  if(!record.passed){
   writeFileSync(join(out,'failed-'+records.length+'-'+result.pid+'.stdin.lua'),spec.bytes);
   if(spec.kind==='xml'&&result.exit===1){const line=Number(record.stderr.match(/stdin:(\d+):/)?.[1]);if(line){const mapping=await child(exe,['line',spec.path,String(spec.index),String(line)],null,8<<20);record.diagnosticMapping=JSON.parse(mapping.stdout);record.mappingExit=mapping.exit;}}
  }
  records.push(record);
 }
 async function enqueue(spec){
  if(asyncFailure)throw asyncFailure;
  while(running.size>=4)await Promise.race(running);
  if(asyncFailure)throw asyncFailure;
  const task=compile(spec);running.add(task);task.then(()=>running.delete(task),e=>{asyncFailure??=e;running.delete(task);});
 }
 try{
  const manifest=JSON.parse(readFileSync(manifestPath));assert(manifest.commit==='31c7f7b9cc79e56c986b365c06a6afbcf3c9177b'&&manifest.files.length===4036);
  const digest=createHash('sha256'),names=new Set();
  for(const file of manifest.files){assert(!names.has(file.path.toLowerCase()));names.add(file.path.toLowerCase());const {bytes}=admit(file);sourceBytes+=bytes.length;assert(sourceBytes<=128<<20);documentsVerified++;digest.update(file.path).update(Buffer.from([0])).update(file.sha256).update(Buffer.from([0]));const size=Buffer.alloc(8);size.writeBigUInt64LE(BigInt(file.bytes));digest.update(size);}
  assert.equal(digest.digest('hex'),sourceDigest);
  for(const file of manifest.files.filter(f=>f.path.endsWith('.lua'))){
   const {path,bytes}=admit(file),prefix=bytes.subarray(0,3).equals(Buffer.from([239,187,191]))?3:0;luaFiles++;luaBytes+=bytes.length;
   if(selection==='all')await enqueue({kind:'lua',path,bytes:bytes.subarray(prefix),originalSHA256:file.sha256,prefixBytes:prefix});
  }
  await Promise.all(running);
  if(asyncFailure)throw asyncFailure;
  for(const file of manifest.files.filter(f=>f.path.endsWith('.xml'))){
   const {path}=admit(file),parsed=await child(exe,['extract',path],null,8<<20);assert(!parsed.failure&&parsed.exit===0,'XML extraction unavailable');const program=JSON.parse(parsed.stdout);assert(program.raw_sha256===file.sha256&&program.raw_bytes===file.bytes);xmlFiles++;
   const maps=[];
   for(const [index,script]of program.scripts.entries()){xmlScripts++;const bytes=Buffer.from(script.compiler.text);maps.push(script.compiler.segments);await enqueue({kind:'xml',path,index,scriptId:script.id,bytes,originalSHA256:file.sha256,sourceMapSHA256:sha(Buffer.from(JSON.stringify(script.compiler.segments)))});}
   profileDiagnostics.push(...(program.diagnostics??[]).map(d=>({path,...d})));
   xmlRecords.push({path,originalSHA256:file.sha256,scripts:program.scripts.length,mapsSHA256:sha(Buffer.from(JSON.stringify(maps))),profileValidationPassed:program.coverage.profileValidationPassed});
  }
  await Promise.all(running);
  if(asyncFailure)throw asyncFailure;
 }catch(e){error=String(e);await Promise.allSettled(running);}
 records.sort((a,b)=>a.path.localeCompare(b.path,'en')||(a.index??-1)-(b.index??-1));
 const summary={selection,documentsVerified,sourceBytes,luaFiles,luaBytes,luaCompiled:selection==='all'?luaFiles:0,xmlFiles,xmlScripts,compiled:records.length,syntaxFailures:records.filter(r=>!r.passed).length,profileDiagnostics:profileDiagnostics.length,elapsedMs:Math.round(performance.now()-started),childConcurrency:4,totalBudgetMs:85000,resultCacheHit:false};
 const passed=!error&&!asyncFailure&&documentsVerified===4036&&luaFiles===2551&&luaBytes===23354176&&xmlFiles===1091&&xmlScripts===4018&&records.length===(selection==='all'?6569:4018)&&records.every(r=>r.passed)&&profileDiagnostics.length===1;
 writeFileSync(join(out,'worker.json'),JSON.stringify({passed,error,summary,records,xmlRecords,profileDiagnostics,sourceExecuted:false,semanticComplete:false},null,2));if(!passed)process.exitCode=1;
}
