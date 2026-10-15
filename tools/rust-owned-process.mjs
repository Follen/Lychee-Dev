import {spawnSync,spawn} from 'node:child_process';
import {existsSync,lstatSync,readFileSync,writeFileSync} from 'node:fs';
import {join,resolve} from 'node:path';
import {randomUUID} from 'node:crypto';

export function auxiliaryRequest({cargoBuild=false,msvcCompile=false}={}){
  if(typeof cargoBuild!=='boolean'||typeof msvcCompile!=='boolean')throw new Error('owned.invalid_auxiliary_request');
  if(cargoBuild&&msvcCompile)throw new Error('owned.conflicting_auxiliary_request');
  return {cargoBuild,msvcCompile};
}
function request(command,args,{cwd,env,evidence,timeout=600000,orphanMs=30000,cargoBuild=false,msvcCompile=false,memoryObservation=false}){
  if(process.platform!=='win32')throw new Error('owned.windows_required');
  const admission=auxiliaryRequest({cargoBuild,msvcCompile});
  if(typeof memoryObservation!=='boolean')throw new Error('owned.invalid_memory_request');
  const id=randomUUID(),base=join(evidence,`process-${id}`),path=`${base}.request.json`;
  const spec={command,args,cwd:resolve(cwd),timeoutMs:timeout,orphanMs,...admission,stdout:`${base}.stdout.bin`,stderr:`${base}.stderr.bin`,receipt:`${base}.receipt.json`};
  if(memoryObservation){spec.memoryPrefix=`${base}.memory`;spec.memoryMarker=`${base}.phase`;env={...env,LYCHEEDEV_OWNED_MEMORY_PHASE:spec.memoryMarker};}
  writeFileSync(path,JSON.stringify(spec,null,2)+'\n',{flag:'wx'});
  return {spec,path,env,wrapper:join(import.meta.dirname,'rust-owned-process.ps1')};
}
function result(job,wrapper){
  const {spec,path}=job,read=path=>existsSync(path)&&lstatSync(path).size<=16<<20?readFileSync(path):Buffer.alloc(0);
  let proof;try{proof=JSON.parse(readFileSync(spec.receipt));}catch{}
  const handlesReaped=wrapper.status===0&&proof?.rootReaped===true&&proof.treeReaped===true&&proof.handlesClosed===true&&proof.toolIdentityLocksClosed===true&&proof.outputReadersComplete===true&&proof.activeProcesses===0;
  const error=wrapper.error||(!handlesReaped?new Error('owned.tree_reap_unverified'):proof.error||(proof.forced&&!proof.knownAuxiliaryCleanup)?new Error(proof.error||'owned.forced_tree_termination'):undefined);
  const diagnostic=`${path}.wrapper.json`;writeFileSync(diagnostic,JSON.stringify({status:wrapper.status,signal:wrapper.signal,error:wrapper.error?.message,stdout:wrapper.stdout?.toString(),stderr:wrapper.stderr?.toString()},null,2)+'\n');
  return {status:handlesReaped?proof.exitCode:null,signal:wrapper.signal,error,handlesReaped,proof,stdout:read(spec.stdout),stderr:read(spec.stderr),logs:[path,diagnostic,...[spec.stdout,spec.stderr,spec.receipt].filter(existsSync)]};
}
export function ownedSpawnSync(command,args,options){
  const job=request(command,args,options);
  const wrapper=spawnSync('powershell.exe',['-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',job.wrapper,'-Request',job.path],{cwd:options.cwd,env:job.env,windowsHide:true,shell:false,timeout:(options.timeout||600000)+60000,maxBuffer:1<<20});
  return result(job,wrapper);
}
export async function ownedSpawn(command,args,options){
  const job=request(command,args,options),chunks=[],errors=[];
  const child=spawn('powershell.exe',['-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',job.wrapper,'-Request',job.path],{cwd:options.cwd,env:job.env,windowsHide:true,shell:false,stdio:['ignore','pipe','pipe']});
  let error,bytes=0;child.once('error',failure=>error=failure);
  function receive(list,chunk){bytes+=chunk.length;if(bytes>1<<20){error=new Error('owned.wrapper_output_budget');child.kill();}else list.push(chunk);}
  child.stdout.on('data',chunk=>receive(chunks,chunk));child.stderr.on('data',chunk=>receive(errors,chunk));
  const timeout=setTimeout(()=>{error=new Error('owned.wrapper_timeout');child.kill();},(options.timeout||600000)+60000);
  const wrapper=await new Promise(ok=>child.once('close',(status,signal)=>ok({status,signal})));clearTimeout(timeout);
  return result(job,{...wrapper,error,stdout:Buffer.concat(chunks),stderr:Buffer.concat(errors)});
}
