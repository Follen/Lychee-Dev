// Test-only measurement of the held native process; never reopens a PID.
import assert from 'node:assert/strict';
import {readFileSync,writeFileSync,mkdirSync,existsSync,copyFileSync} from 'node:fs';
import {resolve,join} from 'node:path';
import {spawnSync} from 'node:child_process';
import {randomUUID,createHash} from 'node:crypto';
import {hashFile} from '../../tools/rust-build.mjs';

const root=resolve('.'),out=resolve(process.argv[2]??'.tmp/rust-emmy-resource-01');
const scenario=process.argv[3]??'normal';assert(['normal','deadline','memory_limit','cycles'].includes(scenario));
const jobMemoryLimitBytes=scenario==='memory_limit'?512*1024*1024:2**31;
assert(out.startsWith(root+'\\')&&!existsSync(out));mkdirSync(out);
const artifactReceipt=JSON.parse(readFileSync(resolve(process.argv[4]??'.tmp/rust-emmy-retail-03/artifact.json')));
for(const [path,digest] of [[artifactReceipt.binary,artifactReceipt.binarySHA256],[artifactReceipt.pdb,artifactReceipt.pdbSHA256]])assert.equal(hashFile(path),digest);
const exe=join(out,'emmy_library_probe.exe'),pdb=exe.replace(/\.exe$/,'.pdb');
copyFileSync(artifactReceipt.binary,exe);copyFileSync(artifactReceipt.pdb,pdb);
let wrapper=readFileSync(join(root,'tools/rust-owned-process.ps1'),'utf8');
function replaceOnce(from,to){assert.equal(wrapper.split(from).length,2,from);wrapper=wrapper.replace(from,to);}
replaceOnce('var limits=new LIMITS();limits.basic.flags=0x2000;Set(job,9,limits);',
 `var limits=new LIMITS();limits.basic.flags=0x2000|0x200;limits.jobMemory=new UIntPtr(${jobMemoryLimitBytes}UL);Set(job,9,limits);`);
replaceOnce('public sealed class Result {',`[StructLayout(LayoutKind.Sequential)] struct MEMORY {public uint cb,faults;public UIntPtr peakWorkingSet,workingSet,peakPaged,paged,peakNonPaged,nonPaged,pagefile,peakPagefile,privateBytes;}
 public sealed class ResourceSample {public long processElapsedMs,stdoutBytes;public ulong workingSetBytes,peakWorkingSetBytes,privateBytes;public uint handles;}
 [DllImport("psapi.dll",SetLastError=true)] static extern bool GetProcessMemoryInfo(IntPtr process,out MEMORY memory,uint size);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool GetProcessHandleCount(IntPtr process,out uint count);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool GetProcessIoCounters(IntPtr process,out IOCOUNTERS io);
 [DllImport("kernel32.dll",SetLastError=true,EntryPoint="QueryInformationJobObject")] static extern bool QueryLimits(IntPtr job,int kind,out LIMITS limits,uint size,IntPtr returned);
 static ulong Ticks(FT value){return ((ulong)value.high<<32)|value.low;}
 public sealed class Result {public List<ResourceSample> resources=new List<ResourceSample>();public List<uint> jobMemoryMessagePids=new List<uint>();public bool resourceSamplerComplete;public string resourceError;public ulong peakJobCommitBytes,userCpu100ns,kernelCpu100ns,readBytes,writeBytes,otherBytes;`);
replaceOnce('long outputBytes=0;string readerError=null;',
 'long outputBytes=0;string readerError=null;Task sampler=null;var sampleStop=new ManualResetEventSlim(false);var processClock=new Stopwatch();');
replaceOnce('file.Write(buffer,0,count);','file.Write(buffer,0,count);file.Flush();');
replaceOnce('if(message==4)break;', 'if(message==10){if(result.jobMemoryMessagePids.Count>=1024)throw new Exception("resource.memory_message_budget");result.jobMemoryMessagePids.Add((uint)value.ToInt64());}if(message==4)break;');
replaceOnce('if(ResumeThread(process.thread)==UInt32.MaxValue)throw Error("owned.resume");',
 `processClock.Start();if(ResumeThread(process.thread)==UInt32.MaxValue)throw Error("owned.resume");
   sampler=Task.Run(()=>{try{while(!sampleStop.IsSet&&WaitForSingleObject(process.process,0)!=0){
    MEMORY memory;uint count;
    if(!GetProcessMemoryInfo(process.process,out memory,(uint)Marshal.SizeOf(typeof(MEMORY)))||!GetProcessHandleCount(process.process,out count)){if(WaitForSingleObject(process.process,0)==0)break;throw Error("resource.native_sample");}
    if(result.resources.Count>=3000)throw new Exception("resource.sample_budget");
    long length=File.Exists(stdout)?new FileInfo(stdout).Length:0;
    result.resources.Add(new ResourceSample{processElapsedMs=processClock.ElapsedMilliseconds,stdoutBytes=length,workingSetBytes=memory.workingSet.ToUInt64(),peakWorkingSetBytes=memory.peakWorkingSet.ToUInt64(),privateBytes=memory.privateBytes.ToUInt64(),handles=count});
    sampleStop.Wait(40);
   }}catch(Exception failure){result.resourceError=failure.ToString();}});`);
replaceOnce('bool closed=rootWatcher==null||rootWatcher.Wait(10000);',
 `sampleStop.Set();result.resourceSamplerComplete=sampler==null||sampler.Wait(5000);sampleStop.Dispose();
   if(job!=IntPtr.Zero){LIMITS finalLimits;if(QueryLimits(job,9,out finalLimits,(uint)Marshal.SizeOf(typeof(LIMITS)),IntPtr.Zero))result.peakJobCommitBytes=finalLimits.peakJob.ToUInt64();else result.resourceError=(result.resourceError??"")+" resource.job_peak";}
   if(process.process!=IntPtr.Zero){FT creation,exit,kernel,user;IOCOUNTERS io;if(GetProcessTimes(process.process,out creation,out exit,out kernel,out user)){result.userCpu100ns=Ticks(user);result.kernelCpu100ns=Ticks(kernel);}else result.resourceError=(result.resourceError??"")+" resource.cpu";
    if(GetProcessIoCounters(process.process,out io)){result.readBytes=io.readBytes;result.writeBytes=io.writeBytes;result.otherBytes=io.otherBytes;}else result.resourceError=(result.resourceError??"")+" resource.io";}
   bool closed=(sampler==null||result.resourceSamplerComplete)&&(rootWatcher==null||rootWatcher.Wait(10000));`);
replaceOnce("error=$result.error}","error=$result.error;resources=$result.resources;resourceSamplerComplete=$result.resourceSamplerComplete;resourceError=$result.resourceError;jobMemoryMessagePids=$result.jobMemoryMessagePids;peakJobCommitBytes=$result.peakJobCommitBytes;userCpu100ns=$result.userCpu100ns;kernelCpu100ns=$result.kernelCpu100ns;readBytes=$result.readBytes;writeBytes=$result.writeBytes;otherBytes=$result.otherBytes}");
const wrapperPath=join(out,'owned-resource-2gib.ps1');writeFileSync(wrapperPath,wrapper,{flag:'wx'});
const id=randomUUID(),base=join(out,'process-'+id),spec={command:exe,args:['retail',join(root,'.tmp/rust-source-independent-retail-01/input/manifest.json'),join(root,'.tmp/gob01/source/v1/worktrees/wow-ui-source/31c7f7b9cc79e56c986b365c06a6afbcf3c9177b'),join(root,'.tmp/gob01/source/v1/environments/adcc31a11d79ef0170e3318277e11b17a9c4aa2173ca0adca25afab9379ade36/api.d.lua'),'20',scenario==='normal'?'250':'0'],cwd:root,timeoutMs:scenario==='deadline'?5000:90000,orphanMs:10000,cargoBuild:false,stdout:base+'.stdout.jsonl',stderr:base+'.stderr.txt',receipt:base+'.receipt.json'};
if(scenario==='cycles')spec.args=[...spec.args.slice(0,4),'3','3','250'];
if(scenario==='cycles')spec.args[0]='retail-cycles';
writeFileSync(base+'.request.json',JSON.stringify(spec,null,2));
writeFileSync(join(out,'runner-snapshot.mjs'),readFileSync(import.meta.filename));
writeFileSync(join(out,'artifact.json'),JSON.stringify({...artifactReceipt,binary:exe,pdb,wrapperSHA256:hashFile(wrapperPath),runnerSHA256:hashFile(import.meta.filename),jobMemoryLimitBytes,scenario,scope:'test-only native held-process sampling/fault injection; not paired Go/Rust performance acceptance'},null,2));
const run=spawnSync('powershell.exe',['-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',wrapperPath,'-Request',base+'.request.json'],{cwd:root,windowsHide:true,timeout:115000,maxBuffer:1<<20});
writeFileSync(join(out,'wrapper-result.json'),JSON.stringify({status:run.status,error:run.error?.message,stdout:run.stdout?.toString(),stderr:run.stderr?.toString()},null,2));
let proof,frames,error;
try{proof=JSON.parse(readFileSync(spec.receipt));const raw=readFileSync(spec.stdout);assert(raw.length<=1<<20);let offset=0;frames=raw.toString().trimEnd().split(/\n/).map(line=>{offset+=Buffer.byteLength(line)+1;return {...JSON.parse(line),stdoutEndByte:offset};});}catch(e){error=String(e);}
const milestones=frames?.filter(f=>f.event==='resource_milestone')??[],samples=proof?.resources??[];
const phaseRows=milestones.map((frame,index)=>{const next=milestones[index+1]?.stdoutEndByte??Infinity;const rows=samples.filter(s=>s.stdoutBytes>=frame.stdoutEndByte&&s.stdoutBytes<next);const first=rows[0]?.processElapsedMs;const holdSamples=rows.filter(s=>s.processElapsedMs<first+Math.max(0,frame.holdMs-80));return {phase:frame.phase,cycle:frame.cycle??null,elapsedMs:frame.elapsedMs,samples:rows,holdSamples,scope:'all samples until next milestone include intervening work; holdSamples use a conservative first observed window'};});
const terminal=frames?.at(-1),queries=frames?.filter(f=>f.event==='actual_query')??[];
const reaped=run.status===0&&proof?.rootReaped&&proof.treeReaped&&proof.handlesClosed&&proof.activeProcesses===0;
const measurementComplete=proof?.resourceSamplerComplete&&!proof.resourceError&&proof.peakJobCommitBytes>0&&phaseRows.every(p=>p.samples.length>0);
const cycleEnds=frames?.filter(f=>f.event==='cycle_end')??[];
const hashArray=values=>createHash('sha256').update(JSON.stringify(values)).digest('hex');
const cycleRecords=cycleEnds.map(f=>{const actual=queries.filter(q=>q.cycle===f.cycle).map(q=>q.factsSHA256);return {cycle:f.cycle,queries:actual.length,queryFactsSHA256:actual,reportedBodyHash:f.bodyFactsSHA256,actualBodyHash:hashArray(actual),exact:JSON.stringify(actual)===JSON.stringify(f.queryFactsSHA256)&&hashArray(actual)===f.bodyFactsSHA256};});
const drops=phaseRows.filter(p=>p.phase==='after_drop').map(p=>({cycle:p.cycle,privateBytes:p.holdSamples.map(s=>s.privateBytes),handles:p.holdSamples.map(s=>s.handles)}));
const cycleComplete=scenario==='cycles'&&cycleRecords.length===3&&cycleRecords.every((c,i)=>c.cycle===i&&c.queries===3&&c.exact&&JSON.stringify(c.queryFactsSHA256)===JSON.stringify(cycleRecords[0].queryFactsSHA256))&&drops.length===3&&drops.every(d=>d.privateBytes.length>0)&&queries.length===9;
const completedBusiness=(scenario==='normal'&&queries.length===20||cycleComplete)&&proof?.exitCode===0&&terminal?.baselinePassed===true;
const loadInterrupted=frames?.some(f=>f.event==='resource_milestone'&&f.phase==='before_load')&&!frames.some(f=>f.event==='resource_milestone'&&f.phase==='after_load')&&!frames.some(f=>f.event==='end');
const deadlineVerified=scenario==='deadline'&&proof?.deadlineHit===true&&proof.forced===true&&proof.exitCode===8&&loadInterrupted;
// A missing Job notification is inconclusive. Its positive arrival is required here.
// SDK winnt.h defines JOB_OBJECT_MSG_JOB_MEMORY_LIMIT as 10. No abnormal exit alone proves OOM.
const memoryLimitVerified=scenario==='memory_limit'&&proof?.jobMemoryMessagePids?.includes(proof.pid)&&proof.exitCode>=0x80000000&&proof.deadlineHit===false&&proof.forced===false&&loadInterrupted;
const expectedFault=deadlineVerified||memoryLimitVerified;
const report={schema:'lycheedev.emmy-resource.owned.v1',scenario,jobMemoryLimitBytes,passed:reaped&&!error&&measurementComplete&&(completedBusiness||expectedFault),businessComplete:completedBusiness,cycleComplete,cycleRecords,drops,expectedFaultVerified:expectedFault,deadlineVerified,memoryLimitVerified,process:proof,error,terminal,queries,phases:phaseRows,measurementComplete,handlesReaped:reaped,scope:scenario==='cycles'?'three complete input revalidations/new analyses/true queries/Drop cycles in one native owner under the unchanged total 90s/2GiB limit; native post-Drop observations, not infinite no-leak or interprocess reuse proof':scenario==='normal'?'20 same-session fresh models over complete Retail; Windows held native handle samples and kernel peak Job commit; no Go comparison, interprocess reuse, OOM recovery or product acceptance':scenario==='deadline'?'test-only 5s hard deadline during actual full Retail index construction, native owner retirement; no cooperative cancellation or public typed-error claim':'test-only reduced 512MiB Job allocation fault during full Retail construction; requires positive native Job limit message and abnormal exit, not evidence that supported 2GiB workload is insufficient; no public typed-error claim'};
for(const [path,digest] of [[exe,artifactReceipt.binarySHA256],[pdb,artifactReceipt.pdbSHA256]])assert.equal(hashFile(path),digest);
writeFileSync(join(out,'report.json'),JSON.stringify(report,null,2));
console.log(JSON.stringify({report:join(out,'report.json'),passed:report.passed,queries:queries.length,samples:samples.length,peakJobCommitBytes:proof?.peakJobCommitBytes,handlesReaped:reaped,measurementComplete,error:proof?.resourceError??error}));
if(!report.passed)process.exitCode=1;
