import {ownedSpawnSync} from '../../tools/rust-owned-process.mjs';
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {resolve,join} from 'node:path';
import {isDeepStrictEqual} from 'node:util';

const fixture=resolve(process.env.LYCHEEDEV_LOCAL_DOMAIN_FIXTURE??process.argv[2]??'.tmp/rust-local-domain-public-fixture');
const output=resolve(process.argv[3]??'.tmp/rust-local-domain-public');
mkdirSync(output,{recursive:true});
const identity=JSON.parse(readFileSync(join(fixture,'input-identity.json'),'utf8'));
if(identity.schema!=='lycheedev.local-domain-fixture.v1')throw new Error('An explicitly generated authenticated fixture is required');
const binaries={go:resolve('.tmp/lycheedev-oracle.exe'),rust:resolve(process.env.LYCHEEDEV_RUST_BIN??'target/release/lycheedev.exe')};
const hash=path=>createHash('sha256').update(readFileSync(path)).digest('hex');
const before=Object.fromEntries(Object.entries(binaries).map(([name,path])=>[name,hash(path)]));
const common=['--snapshot',identity.snapshot,'--installation',identity.installation,'--home',identity.home,'--format','json','--offline'];
const inputs=[
 ['aura-present',['data','spell','auras','--spell-id','100']],
 ['aura-non-aura',['data','spell','auras','--spell-id','300']],
 ['aura-absent',['data','spell','auras','--spell-id','999']],
 ['summons-all',['data','spell','summons','--spell-id','300']],
 ['summons-npc-first',['data','spell','summons','--spell-id','300','--npc-id','5000']],
 ['summons-npc-not-second',['data','spell','summons','--spell-id','300','--npc-id','9000']],
 ['aura-missing',['data','spell','auras']],
 ['aura-zero',['data','spell','auras','--spell-id','0']],
 ['summons-negative',['data','spell','summons','--spell-id','-1']],
 ['summons-npc-invalid',['data','spell','summons','--spell-id','300','--npc-id','4294967296']],
];
const parse=raw=>JSON.parse(raw,(key,value,context)=>{
 if(typeof value==='number'&&context?.source&&/^-?\d+$/.test(context.source)){
  const integer=BigInt(context.source);
  if(integer>BigInt(Number.MAX_SAFE_INTEGER)||integer<BigInt(Number.MIN_SAFE_INTEGER))return integer;
 }
 return value;
});
const cases=[];
const processProof=[];
const retainedResult=result=>result&&Object.fromEntries(Object.entries(result).filter(([key])=>!['resources','resourceAccounting','tableVerification'].includes(key)));
function execute(binary,args){
 const run=ownedSpawnSync(binary,args,{cwd:resolve('.'),env:process.env,evidence:output,timeout:180000});
 processProof.push({binary,args,exit:run.status,handlesReaped:run.handlesReaped,error:run.error?.message,proof:run.proof,logs:run.logs});
 if(run.error||!run.handlesReaped)throw new Error(run.error?.message??'owned.tree_reap_unverified');
 return {...run,stdout:run.stdout.toString('utf8'),stderr:run.stderr.toString('utf8')};
}
try{
for(const [name,args]of inputs){
 const runs={};
 for(const [engine,binary]of Object.entries(binaries)){
  const run=execute(binary,[...args,...common]);
  writeFileSync(join(output,`${name}-${engine}.json`),run.stdout??'');
  writeFileSync(join(output,`${name}-${engine}.stderr`),run.stderr??'');
  const envelope=parse(run.stdout);
  const result=envelope.result;
  // Only mutable accounting/capture data and named additive Rust declarations
  // are excluded. All legacy result fields, tables, pin, full rows, relations,
  // error stage/details and completeness remain exact comparisons.
  const retained=retainedResult(result);
  const captures=(envelope.captures??[]).map(({id,blob,capturedAt,...captureIdentity})=>{
   if(!/^CAP-[0-9a-f]{64}$/.test(id)||!Number.isFinite(Date.parse(capturedAt))||!/^([0-9a-f]{64})$/.test(blob?.sha256))throw new Error('Invalid capture identity');
   const raw=readFileSync(join(identity.home,'blobs',blob.sha256.slice(0,2),blob.sha256.slice(2)));
   if(raw.length!==blob.bytes||createHash('sha256').update(raw).digest('hex')!==blob.sha256)throw new Error('Capture blob reference differs from bytes');
   const payload=parse(raw.toString('utf8'));
   if(!isDeepStrictEqual(payload,result))throw new Error('Captured result differs from the public result');
   return {...captureIdentity,payload:retainedResult(payload)};
  });
  const envelopeContract={...envelope,result:retained,captures};
  runs[engine]={exit:run.status,ok:envelope.ok,envelopeContract,...(retained?{result:retained}:{error:envelope.error})};
  if(engine==='rust'&&envelope.ok){
   const proof=result.tableVerification;
   runs[engine].verification={passed:Array.isArray(proof)&&proof.length===1&&proof.every(p=>p.encodingFramingEKey===true&&p.encodingRequestedPagesMD5===true&&p.encodingWholeDecodedCKey===false&&p.rootWholeDecodedCKey===true&&p.contentWholeDecodedCKey===true),value:proof};
   runs[engine].captureProof=[];
   for(const capture of envelope.captures??[]){
    const check=execute(binary,['evidence','verify',capture.id,'--home',identity.home,'--format','json']);
    writeFileSync(join(output,`${name}-${capture.id}-verify.json`),check.stdout??'');
    runs[engine].captureProof.push({id:capture.id,exit:check.status,verified:parse(check.stdout).result?.verified===true});
   }
  }
 }
 const {captureProof,verification,...rust}=runs.rust;
 cases.push({name,args,equal:isDeepStrictEqual(runs.go,rust)&&(verification===undefined||verification.passed)&&(captureProof===undefined||captureProof.length>0&&captureProof.every(p=>p.exit===0&&p.verified)),...runs});
}
const after=Object.fromEntries(Object.entries(binaries).map(([name,path])=>[name,hash(path)]));
const report={schema:'lycheedev.local-domain-public-parity.v2',scope:'Six sequential success and four admission profiles on one genuine authenticated synthetic local CASC/DBD/WDC install. Legacy result/error fields and envelope schema/operation/context/warnings/capture provenance are compared; mutable resource counters and additive verification declarations are excluded, the latter separately asserted. Capture IDs, timestamps and content-derived blob hashes/sizes differ because capture JSON includes mutable counters; every Rust capture is independently verified. Not real-game, partial/CDN, full domain or performance acceptance.',inputIdentity:identity,binarySHA256:before,artifactStable:isDeepStrictEqual(before,after),cases,passed:isDeepStrictEqual(before,after)&&cases.every(c=>c.equal)};
const stringify=value=>JSON.stringify(value,(key,item)=>typeof item==='bigint'?`${item}n`:item,2)+'\n';
writeFileSync(join(output,'report.json'),stringify(report));
console.log(stringify({report:join(output,'report.json'),passed:report.passed,differences:cases.filter(c=>!c.equal)}));
if(!report.passed)process.exitCode=1;
}finally{
 writeFileSync(join(output,'process-proof.json'),JSON.stringify({schema:'lycheedev.local-domain-process-proof.v1',runs:processProof},null,2)+'\n');
}
