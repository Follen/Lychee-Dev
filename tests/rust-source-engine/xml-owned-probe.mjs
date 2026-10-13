// XML example plus real pinned luac children, under one native custody Job.
import assert from 'node:assert/strict';
import {readFileSync,writeFileSync,mkdirSync,copyFileSync,existsSync,statSync} from 'node:fs';
import {resolve,join,basename} from 'node:path';
import {spawn,spawnSync} from 'node:child_process';
import {randomUUID} from 'node:crypto';
import {hashFile} from '../../tools/rust-build.mjs';

const root=resolve('.'),out=resolve(process.argv[2]??'.tmp/rust-emmy-xml-01');
const exe=join(out,'emmy_xml_program.exe'),pdb=exe.replace(/\.exe$/,'.pdb');
const luac=join(root,'.tmp/rust-luals-suspended-release-01/payload/tool/luac/luac.exe');
const runtime=JSON.parse(readFileSync(join(root,'release/tools/luac.json'))).runtime;
assert.equal(runtime.version,'5.1.5');assert.equal(statSync(luac).size,runtime.bytes);assert.equal(hashFile(luac),runtime.sha256);

if(!process.argv.includes('--worker')){
 assert(out.startsWith(root+'\\')&&!existsSync(out));mkdirSync(out);
 const buildReceipt=process.argv[3]??'dab14f8b-3344-4f1b-8cfc-48ea2eb14299';
 assert(/^[a-f0-9-]{36}$/.test(buildReceipt));
 const build=JSON.parse(readFileSync(join(root,'.tmp/rust-disk/evidence',buildReceipt,'receipt.json')));
 assert.equal(build.state,'retired');assert.equal(build.handlesReaped,true);
 const buildProcess=build.sealedLogs.filter(log=>log.path.endsWith('.receipt.json')).map(log=>JSON.parse(readFileSync(log.path)));
 assert(buildProcess.some(p=>p.exitCode===0&&p.rootReaped&&p.treeReaped&&p.handlesClosed));
 const sourceExe=join(root,'target/debug/examples/emmy_xml_program.exe'),sourcePdb=sourceExe.replace(/\.exe$/,'.pdb');
 const binarySHA256=hashFile(sourceExe),pdbSHA256=hashFile(sourcePdb);copyFileSync(sourceExe,exe);copyFileSync(sourcePdb,pdb);
 const sourceWrapper=readFileSync(join(root,'tools/rust-owned-process.ps1'),'utf8');
 const anchor='var limits=new LIMITS();limits.basic.flags=0x2000;Set(job,9,limits);';assert.equal(sourceWrapper.split(anchor).length,2);
 const wrapper=join(out,'owned-xml-2gib.ps1');writeFileSync(wrapper,sourceWrapper.replace(anchor,'var limits=new LIMITS();limits.basic.flags=0x2000|0x200;limits.jobMemory=new UIntPtr(2147483648UL);Set(job,9,limits);'));
 writeFileSync(join(out,'runner-snapshot.mjs'),readFileSync(import.meta.filename));
 const artifact={schema:'lycheedev.emmy-xml-artifact.v1',binary:exe,binarySHA256,pdb,pdbSHA256,buildReceipt,sourceTree:build.inputHashes.sourceTree,wrapperSHA256:hashFile(wrapper),runnerSHA256:hashFile(import.meta.filename),luac,runtime,nodeSHA256:hashFile(process.execPath),jobMemoryLimitBytes:2**31};
 writeFileSync(join(out,'artifact.json'),JSON.stringify(artifact,null,2));
 const base=join(out,'process-'+randomUUID()),spec={command:process.execPath,args:[import.meta.filename,out,'--worker'],cwd:root,timeoutMs:30000,orphanMs:5000,cargoBuild:false,stdout:base+'.stdout.txt',stderr:base+'.stderr.txt',receipt:base+'.receipt.json'};
 writeFileSync(base+'.request.json',JSON.stringify(spec,null,2));
 const run=spawnSync('powershell.exe',['-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',wrapper,'-Request',base+'.request.json'],{cwd:root,windowsHide:true,timeout:65000,maxBuffer:1<<20});
 writeFileSync(join(out,'wrapper-result.json'),JSON.stringify({status:run.status,error:run.error?.message,stdout:run.stdout?.toString(),stderr:run.stderr?.toString()},null,2));
 let proof,worker,error;try{proof=JSON.parse(readFileSync(spec.receipt));worker=JSON.parse(readFileSync(join(out,'worker.json')));}catch(e){error=String(e);}
 const reaped=run.status===0&&proof?.rootReaped&&proof.treeReaped&&proof.handlesClosed&&proof.activeProcesses===0;
 const stable=hashFile(exe)===binarySHA256&&hashFile(pdb)===pdbSHA256&&hashFile(luac)===runtime.sha256;
 const report={schema:'lycheedev.emmy-xml-owned.v1',passed:reaped&&!error&&stable&&proof.exitCode===0&&worker?.passed===true,artifact,worker,error,process:{proof,handlesReaped:reaped},scope:'fixed XML extraction and exact maps, actual pinned luac stdin compilation; no semantic binding, implicit handler catalog, whole Retail XML or full Source acceptance'};
 writeFileSync(join(out,'report.json'),JSON.stringify(report,null,2));console.log(JSON.stringify({report:join(out,'report.json'),passed:report.passed,xmlChecks:worker?.xml?.checks?.length,syntaxChecks:worker?.syntax?.length,handlesReaped:reaped,error}));if(!report.passed)process.exitCode=1;
}else{
 async function childRun(command,args,input,limit){
  const child=spawn(command,args,{cwd:root,windowsHide:true,stdio:['pipe','pipe','pipe']});let outputBytes=0,stdout=[],stderr=[],failure;
  for(const [list,stream]of [[stdout,child.stdout],[stderr,child.stderr]])stream.on('data',chunk=>{outputBytes+=chunk.length;if(outputBytes>limit){failure='child.output_budget';child.kill();return;}list.push(chunk);});
  child.stdin.on('error',e=>failure=String(e));child.stdin.end(input);
  const timer=setTimeout(()=>{failure='child.deadline';child.kill();},5000);
  const exit=await new Promise(done=>{child.once('error',e=>{failure=String(e);done(null);});child.once('close',done);});clearTimeout(timer);
  return {pid:child.pid,args,exit,stdout:Buffer.concat(stdout).toString(),stderr:Buffer.concat(stderr).toString(),failure};
 }
 const raw=await childRun(exe,['golden'],null,8<<20);writeFileSync(join(out,'xml.stdout.json'),raw.stdout);writeFileSync(join(out,'xml.stderr.txt'),raw.stderr);
 let xml,error;try{xml=JSON.parse(raw.stdout);}catch(e){error=String(e);}
 const syntax=[],extractions=[];
 const expectedInvalid=new Set(['xml-invalid-lua.xml','xml-newline-contract.xml','xml-entity-cr-top.xml','xml-entity-cr-handler.xml']);
 const programs=(xml?.fixtures??[]).map(fixture=>({fixture,originalPath:join(root,'tests/rust-source-engine',fixture.path)}));
 for(const name of ['xml-newline-contract.xml','xml-empty-control.xml','xml-empty-bom.xml','xml-handler-catalog.xml','xml-entity-cr-top.xml','xml-entity-cr-handler.xml','xml-lexical-valid.xml','xml-bindings-394.xml','xml-bindings-root-text.xml','xml-declaration-valid.xml']){
  const originalPath=join(root,'tests/rust-source-engine',name),run=await childRun(exe,['extract',originalPath],null,8<<20);
  writeFileSync(join(out,name+'.extract.json'),run.stdout);let fixture,error;try{fixture=JSON.parse(run.stdout);}catch(e){error=String(e);}
  extractions.push({name,run,error,passed:!run.failure&&!error&&run.exit===0&&Array.isArray(fixture?.scripts)});
  if(extractions.at(-1).passed)programs.push({fixture,originalPath});
 }
 for(const {fixture,originalPath} of programs){
  for(const [index,script]of fixture.scripts.entries()){
   const fixtureName=basename(originalPath),name=fixtureName+'-'+index,input=Buffer.from(script.compiler.text);
   const record=await childRun(luac,['-p','-'],input,65536);
   // Expectations come from fixed cases, never from the analyzer under test.
   const expected=expectedInvalid.has(fixtureName)?1:0;
   assert.equal(hashFile(originalPath),fixture.raw_sha256);
   writeFileSync(join(out,name+'.stdin.lua'),input);writeFileSync(join(out,name+'.stderr.txt'),record.stderr);
   const line=Number(record.stderr.match(/stdin:(\d+):/)?.[1]);let mapping,mappingProcess,mappingError;
   if(line){
    mappingProcess=await childRun(exe,['line',originalPath,String(index),String(line)],null,8<<20);
    try{mapping=JSON.parse(mappingProcess.stdout);}catch(e){mappingError=String(e);}
    if(mappingProcess.failure||mappingProcess.exit!==0||mapping?.mapping?.columnKnown!==false)mappingError??='mapper unavailable or claims a column';
    assert.equal(mapping?.compilerInputSHA256,hashFile(join(out,name+'.stdin.lua')));
   }
   syntax.push({name,...record,expectedExit:expected,inputBytes:input.length,inputSHA256:hashFile(join(out,name+'.stdin.lua')),originalSHA256:fixture.raw_sha256,sourceMap:script.compiler.segments,diagnostic:mapping,mappingProcess,mappingError,sourceExecuted:false,passed:!record.failure&&!mappingError&&record.exit===expected&&record.stdout===''});
  }
 }
 const passed=!error&&!raw.failure&&raw.exit===0&&xml?.state==='passed'&&extractions.every(e=>e.passed)&&syntax.length>0&&syntax.every(s=>s.passed);
 const report={passed,xml,xmlProcess:raw,error,syntax,extractions,luacActualRun:true,semanticComplete:false,sourceExecuted:false};writeFileSync(join(out,'worker.json'),JSON.stringify(report,null,2));if(!passed)process.exitCode=1;
}
