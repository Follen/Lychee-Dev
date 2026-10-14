// Dependency-only Cargo runner. POSIX children own a new process group. A group
// that cannot be proven absent is retained as interrupted, never called reaped.
import {spawn} from 'node:child_process';
import {appendFileSync,existsSync,readFileSync,writeFileSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import {resolve,dirname,join} from 'node:path';
import {acquireChannel,beginScratch,sha256} from '../../tools/rust-disk.mjs';
import {ownedSpawnSync} from '../../tools/rust-owned-process.mjs';
import {inspectEmmy} from '../../tools/rust-emmy-identity.mjs';
const root=resolve(dirname(fileURLToPath(import.meta.url)),'../..');
const source=join(root,'.tmp/emmy-reconstruction-v1/upstream');
const args=process.argv.slice(2);
if(args.length===1&&args[0]==='--verify'){
  const result=inspectEmmy(root);
  console.log(JSON.stringify({state:'verified',identity:result.identity,patchedTreeSHA256:result.patchedTreeSHA256,recipeSHA256:result.recipeSHA256,files:result.files.length}));
  process.exit(0);
}

if(!['test','check','fetch'].includes(args[0])||args.some(x=>/^--(?:target-dir|manifest-path|config)(?:=|$)/.test(x)))throw Error('emmy.invalid_cargo_arguments');
if(process.env.CARGO_TARGET_DIR)throw Error('emmy.target_override');
const verify=()=>inspectEmmy(root);
const before=verify();
// Controlled package profile overrides match the project's dependency-debug=0
// policy without modifying the shared Cargo toolchain configuration.
const invocationArgs=[...args,'--config','profile.dev.package."*".debug=0','--config','profile.test.package."*".debug=0'];
const env={...process.env,CARGO_TARGET_DIR:join(root,'target'),CARGO_PROFILE_DEV_DEBUG:'1',CARGO_PROFILE_TEST_DEBUG:'1'};
const environmentSHA256=sha256(Buffer.from(JSON.stringify(Object.entries(env).sort(([a],[b])=>a.localeCompare(b)))));
const inputs={tree:before.patchedTreeSHA256,recipe:before.recipeSHA256,identity:before.identitySHA256,environment:environmentSHA256,verifier:sha256(readFileSync(join(root,'tools/rust-emmy-identity.mjs'))),manifest:sha256(readFileSync(join(source,'Cargo.toml'))),lock:sha256(readFileSync(join(source,'Cargo.lock')))};
const job=beginScratch(root,'emmy-reconstruction',{command:['cargo',...invocationArgs],inputs,admissionOptions:{requiredBytes:2*1024**3,cacheReserveBytes:2*1024**3}});
let channel,terminal={state:'failed',handlesReaped:true,logs:[]};
const stdout=join(job.evidence,'stdout.log'),stderr=join(job.evidence,'stderr.log'),metadata=join(job.evidence,'command.json');
try{
  channel=acquireChannel(root,'ordinary','emmy-reconstruction');
  if(channel.target!==env.CARGO_TARGET_DIR)throw Error('emmy.channel_target_differs');
  let result;
  terminal={state:'interrupted',handlesReaped:false,logs:[]};
  if(process.platform==='win32'){
    result=ownedSpawnSync('cargo',invocationArgs,{cwd:source,env,evidence:job.evidence,cargoBuild:true,windowsHide:true,shell:false,timeout:1800000,maxBuffer:8<<20});
    writeFileSync(stdout,result.stdout||'');writeFileSync(stderr,result.stderr||'');
  }else{
    result=await new Promise((done,reject)=>{
      let out=[],err=[],bytes=0,forced=false,spawnError;
      const child=spawn('cargo',invocationArgs,{cwd:source,env,detached:true,stdio:['ignore','pipe','pipe']});
      const groupExists=()=>{if(!child.pid)return false;try{process.kill(-child.pid,0);return true;}catch(e){if(e.code==='ESRCH')return false;throw e;}};
      const stop=()=>{forced=true;if(child.pid){try{process.kill(-child.pid,'SIGKILL');}catch(e){if(e.code!=='ESRCH')spawnError=e;}}};
      const timer=setTimeout(stop,1800000);
      writeFileSync(stdout,'');writeFileSync(stderr,'');
      const collect=(list,path)=>chunk=>{bytes+=chunk.length;if(bytes>8<<20){stop();return;}list.push(chunk);appendFileSync(path,chunk);};
      child.stdout.on('data',collect(out,stdout));child.stderr.on('data',collect(err,stderr));
      child.on('error',e=>{spawnError=e;});
      child.on('close',async(code,signal)=>{
        clearTimeout(timer);
        try{
          // A successful cargo process must not leave its process group alive.
          let alive=groupExists();
          if(alive){stop();for(let n=0;n<20&&alive;n++){await new Promise(r=>setTimeout(r,50));alive=groupExists();}}
          writeFileSync(stdout,Buffer.concat(out));writeFileSync(stderr,Buffer.concat(err));
          done({status:code,signal,error:spawnError,forced,handlesReaped:!alive});
        }catch(e){reject(e);}
      });
    });
  }
  writeFileSync(metadata,JSON.stringify({command:'cargo',args:invocationArgs,cwd:source,inputs,environmentSHA256,timeoutMillis:1800000,target:channel.target,status:result.status,signal:result.signal,forced:result.forced,handlesReaped:result.handlesReaped,error:result.error?.message},null,2)+'\n');
  terminal={state:result.handlesReaped?(result.status===0&&!result.error&&!result.forced?'succeeded':'failed'):'interrupted',handlesReaped:result.handlesReaped===true,logs:[stdout,stderr,metadata,...(result.logs||[])]};
  const after=verify();
  if(after.patchedTreeSHA256!==before.patchedTreeSHA256||after.recipeSHA256!==before.recipeSHA256||after.identitySHA256!==before.identitySHA256||sha256(readFileSync(join(root,'tools/rust-emmy-identity.mjs')))!==inputs.verifier)throw Error('emmy.source_or_recipe_changed_during_build');
  process.stdout.write(readFileSync(stdout));process.stderr.write(readFileSync(stderr));
  console.log(JSON.stringify({state:terminal.state,evidence:job.evidence}));
  if(terminal.state!=='succeeded')process.exitCode=1;
}catch(error){
  if(terminal.handlesReaped)terminal.state='failed';
  const previous=existsSync(metadata)?JSON.parse(readFileSync(metadata,'utf8')):{};
  writeFileSync(metadata,JSON.stringify({...previous,error:error.message,inputs},null,2)+'\n');
  terminal.logs=[...new Set([...terminal.logs,metadata,...[stdout,stderr].filter(existsSync)])];
  console.error(error);process.exitCode=1;
}finally{
  job.seal(terminal);
  if(terminal.handlesReaped){job.cleanup();channel?.release();}
}
