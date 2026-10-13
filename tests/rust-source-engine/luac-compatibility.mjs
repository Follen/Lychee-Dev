// Pinned syntax authority only: stdin compilation, never Lua execution.
import assert from 'node:assert/strict';
import {spawn,spawnSync} from 'node:child_process';
import {readFileSync,writeFileSync,mkdirSync,existsSync,lstatSync} from 'node:fs';
import {resolve,join} from 'node:path';
import {createHash,randomUUID} from 'node:crypto';
const root=resolve('.'),out=resolve(process.argv[2]??'.tmp/rust-emmy-luac-compat-01');
const release=resolve(process.env.LYCHEEDEV_RELEASE_ROOT??'.tmp/rust-luals-suspended-release-01');
const luac=join(release,'payload/tool/luac/luac.exe'),manifest=JSON.parse(readFileSync(join(root,'release/tools/luac.json'))).runtime;
const hash=raw=>createHash('sha256').update(raw).digest('hex');
assert.equal(lstatSync(luac).size,manifest.bytes);assert.equal(hash(readFileSync(luac)),manifest.sha256);assert.equal(manifest.version,'5.1.5');
const cases=[
 {id:'goto_identifier',source:'local goto = 7; return goto\n',exit:0},
 {id:'goto_statement',source:'goto target\n',exit:1},
 {id:'label_statement',source:'::target::\n',exit:1},
 {id:'goto_member_identifier',source:'local t = { goto = function() end }; t.goto()\n',exit:0},
 {id:'floor_division',source:'return 8 // 2\n',exit:1},
 {id:'bom_unicode_original_positions',source:'\uFEFFlocal text = "😀中文"\nlocal goto = 7; return goto\n',exit:0},
 {id:'not_executed',source:'error("compilation must never execute this")\n',exit:0},
];
if(!process.argv.includes('--worker')){
 assert(!existsSync(out),'fresh evidence required');mkdirSync(out,{recursive:true});
 const template=readFileSync(join(root,'tools/rust-owned-process.ps1'),'utf8');
 const old='var limits=new LIMITS();limits.basic.flags=0x2000;Set(job,9,limits);';assert(template.includes(old));
 const wrapper=join(out,'owned-syntax-2gib.ps1');writeFileSync(wrapper,template.replace(old,'var limits=new LIMITS();limits.basic.flags=0x2000|0x200;limits.jobMemory=new UIntPtr(2147483648UL);Set(job,9,limits);'));
 const base=join(out,'process-'+randomUUID()),spec={command:process.execPath,args:[import.meta.filename,out,'--worker'],cwd:root,timeoutMs:15000,orphanMs:5000,cargoBuild:false,stdout:base+'.stdout.txt',stderr:base+'.stderr.txt',receipt:base+'.receipt.json'};
 const request=base+'.request.json';writeFileSync(request,JSON.stringify(spec,null,2));writeFileSync(join(out,'runner-snapshot.mjs'),readFileSync(import.meta.filename));
 const run=spawnSync('powershell.exe',['-NoProfile','-NonInteractive','-File',wrapper,'-Request',request],{cwd:root,windowsHide:true,timeout:75000,maxBuffer:1<<20});
 let proof,worker,error;try{proof=JSON.parse(readFileSync(spec.receipt));worker=JSON.parse(readFileSync(join(out,'worker.json')));}catch(e){error=String(e);}
 const reaped=run.status===0&&proof?.rootReaped&&proof.treeReaped&&proof.handlesClosed&&proof.activeProcesses===0;
 const stable=hash(readFileSync(luac))===manifest.sha256;
 const report={schema:'lycheedev.source-luac-compatibility.v1',passed:reaped&&stable&&proof.exitCode===0&&worker?.passed===true,manifest,luac,worker,error,process:{proof,handlesReaped:reaped,wrapperExit:run.status,error:run.error?.message},scope:'seven exact Lua5.1 syntax cases, stdin, BOM source-map rule, actual owned child exits; not complete Source or XML integration'};
 writeFileSync(join(out,'report.json'),JSON.stringify(report,null,2));console.log(JSON.stringify({passed:report.passed,report:join(out,'report.json')}));if(!report.passed)process.exitCode=1;
}else{
 const records=[];
 for(const c of cases){
  const original=Buffer.from(c.source),prefix=original.subarray(0,3).equals(Buffer.from([239,187,191]))?3:0,input=original.subarray(prefix);
  writeFileSync(join(out,c.id+'.original.lua'),original);writeFileSync(join(out,c.id+'.stdin.lua'),input);
  const child=spawn(luac,['-p','-'],{cwd:root,windowsHide:true,stdio:['pipe','pipe','pipe']});let stdout='',stderr='',bytes=0,failure;
  for(const [name,stream]of [['stdout',child.stdout],['stderr',child.stderr]])stream.on('data',chunk=>{bytes+=chunk.length;if(bytes>65536){failure='compiler output budget';child.kill();return;}if(name==='stdout')stdout+=chunk;else stderr+=chunk;});
  child.stdin.on('error',e=>failure=String(e));child.stdin.end(input);
  const timer=setTimeout(()=>{failure='compiler deadline';child.kill();},5000);
  const exit=await new Promise(done=>{child.once('error',e=>{failure=String(e);done(null);});child.once('close',done);});clearTimeout(timer);
  writeFileSync(join(out,c.id+'.stderr.txt'),stderr);
  records.push({id:c.id,pid:child.pid,argv:['-p','-'],originalBytes:original.length,originalSHA256:hash(original),inputBytes:input.length,inputSHA256:hash(input),sourceMap:{prefixBytes:prefix,rule:prefix?'analysisByte+3=originalByte':'identity'},exit,expectedExit:c.exit,stdout,stderr,failure,passed:!failure&&exit===c.exit&&stdout===''});
 }
 const report={passed:records.every(r=>r.passed),records,sourceExecuted:false,syntaxAuthority:manifest.sha256};writeFileSync(join(out,'worker.json'),JSON.stringify(report,null,2));if(!report.passed)process.exitCode=1;
}
