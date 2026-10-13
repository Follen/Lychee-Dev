import {spawnSync} from 'node:child_process';
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {resolve,join} from 'node:path';

const output=resolve(process.argv[2]??'.tmp/rust-source-matrix-packaged-public');mkdirSync(output,{recursive:true});
const release=resolve(process.env.LYCHEEDEV_RELEASE_ROOT??'.tmp/rust-full-tools-npm-stage-01/installed/node_modules/lycheedev');
const binary=resolve(process.env.LYCHEEDEV_RUST_BIN??join(release,'native/windows-amd64/lycheedev.exe'));
const home=resolve(process.env.LYCHEEDEV_ENVIRONMENT_HOME??'.tmp/rust-source-matrix-four-baseline-01/home');
const config=resolve(process.env.LYCHEEDEV_MATRIX_CONFIG??'.tmp/rust-source-matrix-four-baseline-01/actual-matrix-10312/matrix.json');
const sha=p=>createHash('sha256').update(readFileSync(p)).digest('hex');
const before=sha(binary),releaseSHA=sha(join(release,'release.json')),started=new Date().toISOString();
function call(name,args){
 const time=performance.now();
 const p=spawnSync(binary,[...args,'--home',home,'--format','json'],{cwd:resolve('.'),windowsHide:true,encoding:'utf8',timeout:180000,maxBuffer:32<<20});
 writeFileSync(join(output,name+'.stdout.json'),p.stdout??'');writeFileSync(join(output,name+'.stderr'),p.stderr??'');
 let envelope=null;try{envelope=JSON.parse(p.stdout);}catch{}
 return {exit:p.status,error:p.error?.message??null,elapsedMs:performance.now()-time,envelope};
}
const run=call('matrix',['source','validate','--matrix',config,'--semantic','--release',release]);
const result=run.envelope?.result;
const configTargets=JSON.parse(readFileSync(config,'utf8')).targets;
const targets=result?.targets?.map((t,i)=>({id:t.id,product:t.product,requestedRef:t.requestedRef,resolvedCommit:t.resolvedCommit,expectedCommit:configTargets[i]?.ref,valid:t.valid,complete:t.complete,syntax:t.syntaxCompilation?.passed,compilerSHA256:t.syntaxCompilation?.compilerSHA256,semantic:t.semantic?.passed,catalog:t.semantic?.environment?.coverage??null}))??[];
// Verify every actually embedded capture, including syntax audits and source
// input captures. Do not assume that checking only the outer matrix seals them.
const ids=new Set();
function collect(v){if(v&&typeof v==='object'){if(v.schema==='lycheedev.capture.v1'&&typeof v.id==='string')ids.add(v.id);for(const child of Object.values(v))collect(child);}}
collect(run.envelope);
const proofs=[];for(const id of ids){const p=call('verify-'+id,['evidence','verify',id]);proofs.push({id,exit:p.exit,verified:p.envelope?.result?.verified===true});}
const after=sha(binary),afterReleaseSHA=sha(join(release,'release.json'));
const passed=run.exit===0&&run.envelope?.ok===true&&result?.valid===true&&targets.length===4&&new Set(targets.map(t=>t.resolvedCommit)).size===4&&targets.every(t=>t.valid&&t.syntax&&t.semantic&&t.resolvedCommit===t.expectedCommit&&t.catalog?.complete===false&&t.catalog?.gaps>0)&&proofs.length>=17&&proofs.every(p=>p.exit===0&&p.verified)&&before===after&&releaseSHA===afterReleaseSHA;
const report={schema:'lycheedev.source-matrix-packaged-proof.v1',passed,started,ended:new Date().toISOString(),binary,release,home,config,binaryBefore:before,binaryAfter:after,artifactStable:before===after,releaseBefore:releaseSHA,releaseAfter:afterReleaseSHA,releaseStable:releaseSHA===afterReleaseSHA,exit:run.exit,error:run.error,elapsedMs:run.elapsedMs,targets,captureProofs:proofs,scope:'Actual installed npm CLI/default fixed resolver/4 distinct source baselines/authenticated luac and LuaLS/every embedded capture. Forever source-only; full Go differential, gameplay and performance matrix not_run.'};
writeFileSync(join(output,'report.json'),JSON.stringify(report,null,2));console.log(JSON.stringify(report,null,2));if(!passed)process.exitCode=1;
