// Linux-only fixture/process ownership. The Windows product runner is unchanged.
import {spawn} from 'node:child_process';
import {lstatSync, readFileSync, readdirSync, readlinkSync, realpathSync, writeFileSync} from 'node:fs';
import {join, resolve} from 'node:path';
import {randomUUID} from 'node:crypto';
import {performance} from 'node:perf_hooks';
import {sha256} from './rust-disk.mjs';

export function namespaceMembers(namespace, proc='/proc') {
  if (!Number.isSafeInteger(namespace) || namespace <= 0) throw new Error('local.invalid_namespace');
  const entries=readdirSync(proc);
  if (entries.length>65536) throw new Error('local.process_inventory_budget');
  const members=[];
  for (const pid of entries.filter(name=>/^\d+$/.test(name))) {
    try { if (readlinkSync(join(proc,pid,'ns/pid'))===`pid:[${namespace}]`) members.push(Number(pid)); }
    catch (error) { if (error.code!=='ENOENT' && error.code!=='ESRCH') throw error; }
  }
  return members;
}

export async function localOwnedSpawn(command,args,{cwd,env=process.env,evidence,writable=[],timeout=300000,maxOutput=8<<20,inspectNamespace=namespaceMembers}={}) {
  if (process.platform!=='linux') throw new Error('local.linux_required');
  if (!Number.isSafeInteger(timeout)||timeout<1||timeout>600000||!Number.isSafeInteger(maxOutput)||maxOutput<1||maxOutput>16<<20) throw new Error('local.invalid_budget');
  command=realpathSync(command); cwd=realpathSync(cwd); evidence=realpathSync(evidence);
  if (!lstatSync(command).isFile() || !Array.isArray(args)||args.some(x=>typeof x!=='string')) throw new Error('local.invalid_command');
  const base=join(evidence,`local-process-${randomUUID()}`);
  const paths={request:`${base}.request.json`,stdout:`${base}.stdout.bin`,stderr:`${base}.stderr.bin`,status:`${base}.status.bin`,receipt:`${base}.receipt.json`};
  // PID 1 is bwrap's reaper. Namespace destruction kills even setsid/double-fork
  // descendants. No process-name or historical-PID kill is ever used.
  const wrapper='/usr/bin/bwrap';
  const wrapperArgs=['--unshare-user','--disable-userns','--unshare-pid','--die-with-parent','--cap-drop','ALL','--ro-bind','/','/','--proc','/proc','--dev','/dev',
    ...writable.flatMap(path=>{path=realpathSync(path);return ['--bind',path,path];}),
    '--chdir',cwd,'--json-status-fd','3','--',command,...args];
  const request={schema:'lycheedev.local-process-request.v1',command,args,cwd,timeout,maxOutput,writable,wrapper,wrapperArgs,
    toolSHA256:sha256(readFileSync(command)),wrapperSHA256:sha256(readFileSync(wrapper)),environmentSHA256:sha256(Buffer.from(JSON.stringify(Object.entries(env).sort())))};
  writeFileSync(paths.request,JSON.stringify(request,null,2)+'\n',{flag:'wx'});
  const buffers=[[],[],[]],sizes=[0,0,0],ended=[false,false,false];
  let error,forced=false,wrapperReaped=false,closeResult;
  const started=performance.now();
  const child=spawn(wrapper,wrapperArgs,{cwd,env,shell:false,stdio:['ignore','pipe','pipe','pipe']});
  function stop(reason) { error??=reason;forced=true;child.kill('SIGKILL'); }
  const streams=[child.stdout,child.stderr,child.stdio[3]];
  streams.forEach((stream,index)=>{
    stream.on('data',chunk=>{sizes[index]+=chunk.length;if(sizes[index]>(index===2?65536:maxOutput))stop('local.output_budget');else buffers[index].push(chunk);});
    stream.once('end',()=>ended[index]=true);
    stream.once('error',failure=>stop(`local.pipe_error:${failure.code||failure.message}`));
  });
  child.once('error',failure=>error??=`local.spawn_error:${failure.code||failure.message}`);
  child.once('exit',()=>wrapperReaped=true);
  const timer=setTimeout(()=>stop('local.timeout'),timeout);
  // A separately bounded close wait cannot turn a hung pipe into a pass.
  const closed=new Promise(ok=>child.once('close',(status,signal)=>ok({status,signal,closed:true})));
  const hard=new Promise(ok=>{closeResult=setTimeout(()=>{stop('local.close_timeout');ok({status:null,signal:null,closed:false});},timeout+5000);});
  const result=await Promise.race([closed,hard]);clearTimeout(timer);clearTimeout(closeResult);
  const [stdout,stderr,statusBytes]=buffers.map(parts=>Buffer.concat(parts));
  let statusLines=[],members=[],namespace,namespaceVerified=false,namespaceError;
  try {
    statusLines=statusBytes.toString('utf8').trim().split('\n').filter(Boolean).map(line=>JSON.parse(line));
    namespace=statusLines[0]?.['pid-namespace'];
    if(readlinkSync('/proc/self/ns/pid')===`pid:[${namespace}]`)throw new Error('local.not_private_namespace');
    // Injection is only for fault tests. It cannot be selected by the CLI.
    // The kernel's final namespace teardown can trail wrapper pipe closure.
    // Observe within the original timeout plus fixed cleanup budget; never
    // signal a discovered PID or extend this deadline on retries.
    do {
      namespaceVerified=false;members=inspectNamespace(namespace);namespaceVerified=true;
      if(!members.length||performance.now()>=started+timeout+5000)break;
      await new Promise(ok=>setTimeout(ok,10));
    } while(true);
  } catch(failure) {namespaceError=`local.namespace_unverified:${failure.message}`;error??=namespaceError;}
  const outputReadersComplete=ended.every(Boolean)&&result.closed;
  const treeReaped=namespaceVerified&&Number.isSafeInteger(namespace)&&members.length===0;
  const handlesReaped=wrapperReaped&&outputReadersComplete&&treeReaped;
  const terminal=statusLines.filter(row=>Object.hasOwn(row,'exit-code'));
  if(!forced&&(terminal.length!==1||terminal[0]['exit-code']!==result.status)) error??='local.exit_unverified';
  if(!handlesReaped) error??='local.tree_reap_unverified';
  const proof={schema:'lycheedev.local-process-receipt.v1',wrapperPid:child.pid,namespace,namespaceMembers:members,namespaceVerified,namespaceError,
    wrapperReaped,treeReaped,outputReadersComplete,handlesReaped,forced,status:result.status,signal:result.signal,error,outputBytes:sizes};
  writeFileSync(paths.stdout,stdout);writeFileSync(paths.stderr,stderr);writeFileSync(paths.status,statusBytes);writeFileSync(paths.receipt,JSON.stringify(proof,null,2)+'\n');
  if(!result.closed) { streams.forEach(stream=>stream.destroy());child.unref(); }
  return {status:result.status,signal:result.signal,error:error?new Error(error):undefined,handlesReaped,proof,stdout,stderr,logs:Object.values(paths)};
}
