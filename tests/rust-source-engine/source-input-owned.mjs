// Root-owned admission gate for the dynamic immutable-input prototype.
import assert from 'node:assert/strict';
import {copyFileSync,existsSync,readFileSync,writeFileSync,mkdirSync,lstatSync} from 'node:fs';
import {join,resolve,sep} from 'node:path';
import {spawnSync} from 'node:child_process';
import {randomUUID,createHash} from 'node:crypto';
import {hashFile,sourceDigest} from '../../tools/rust-build.mjs';
import {verifyFrozenContext} from './context-identity.mjs';
const root=resolve('.'),[output,buildName,contractName]=process.argv.slice(2);assert(output&&buildName&&contractName);
const out=resolve(output);assert(out.startsWith(root+sep)&&!existsSync(out));
const build=JSON.parse(readFileSync(buildName)),contract=JSON.parse(readFileSync(contractName));
assert.equal(build.example,'emmy_source_input_probe');assert.equal(build.sameInvocation,true);
assert.equal(contract.schema,'lycheedev.source-input-contract.v1');
assert.equal(sourceDigest(root),build.sourceTree);
const frozen=verifyFrozenContext(root,build.contextIdentity.path);assert.equal(frozen.sha256,build.contextIdentity.sha256);
const receipt=JSON.parse(readFileSync(build.buildReceipt));assert.equal(receipt.state,'retired');assert.equal(receipt.handlesReaped,true);assert.deepEqual(receipt.command,build.command);
assert.equal(receipt.inputHashes.sourceTree,build.sourceTree);for(const row of receipt.sealedLogs)assert.equal(hashFile(row.path),row.sha256);
assert.equal(hashFile(build.binary),build.binarySHA256);assert.equal(hashFile(build.pdb),build.pdbSHA256);
mkdirSync(out);const binary=join(out,'emmy_source_input_probe.exe'),pdb=join(out,'emmy_source_input_probe.pdb');copyFileSync(build.binary,binary);copyFileSync(build.pdb,pdb);
copyFileSync(contractName,join(out,'fixed-contract.json'));copyFileSync(import.meta.filename,join(out,'runner-snapshot.mjs'));
const wrapperText=readFileSync(join(root,'tools/rust-owned-process.ps1'),'utf8'),anchor='var limits=new LIMITS();limits.basic.flags=0x2000;Set(job,9,limits);';assert.equal(wrapperText.split(anchor).length,2);
const wrapper=join(out,'owned-input-probe-2gib.ps1');writeFileSync(wrapper,wrapperText.replace(anchor,'var limits=new LIMITS();limits.basic.flags=0x2000|0x200;limits.jobMemory=new UIntPtr(2147483648UL);Set(job,9,limits);'));
const base=join(out,'process-'+randomUUID()),spec={command:binary,args:[],cwd:root,timeoutMs:30000,orphanMs:5000,cargoBuild:false,stdout:base+'.stdout.bin',stderr:base+'.stderr.bin',receipt:base+'.receipt.json'};
writeFileSync(base+'.request.json',JSON.stringify(spec,null,2));
const artifact={schema:'lycheedev.source-input-artifact.v1',sealedBuild:build,binary,binarySHA256:build.binarySHA256,pdb,pdbSHA256:build.pdbSHA256,contractSHA256:hashFile(contractName),wrapperSHA256:hashFile(wrapper),runnerSHA256:hashFile(import.meta.filename),jobMemoryLimitBytes:2**31,totalDeadlineMs:30000};
writeFileSync(join(out,'artifact.json'),JSON.stringify(artifact,null,2));
const run=spawnSync('powershell.exe',['-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',wrapper,'-Request',base+'.request.json'],{cwd:root,windowsHide:true,timeout:55000,maxBuffer:1<<20});
writeFileSync(join(out,'wrapper-result.json'),JSON.stringify({status:run.status,error:run.error?.message,stdout:run.stdout?.toString(),stderr:run.stderr?.toString()},null,2));
let proof,frames,error;try{proof=JSON.parse(readFileSync(spec.receipt));const bytes=readFileSync(spec.stdout);assert(bytes.length>0&&bytes.length<=1<<20&&bytes.at(-1)===10);frames=bytes.toString('utf8').trimEnd().split('\n').map(row=>JSON.parse(row));}catch(e){error=String(e);}
const handlesReaped=run.status===0&&!run.error&&proof?.rootReaped===true&&proof.treeReaped===true&&proof.handlesClosed===true&&proof.toolIdentityLocksClosed===true&&proof.outputReadersComplete===true&&proof.activeProcesses===0;
const checks=[];function check(name,expected,actual){let passed=true,message;try{assert.deepEqual(actual,expected);}catch(e){passed=false;message=e.message;}checks.push({name,expected,actual,passed,error:message});}
check('native ownership terminal',true,handlesReaped);check('clean execution',0,proof?.exitCode);
check('fixed EXE',build.binarySHA256,hashFile(binary));check('matching PDB',build.pdbSHA256,hashFile(pdb));
check('fixed source tree',build.sourceTree,sourceDigest(root));check('unchanged frozen context',frozen.sha256,verifyFrozenContext(root,build.contextIdentity.path).sha256);
const assertions=frames?.filter(x=>x.event==='assertion')??[];check('exact external assertion contract',contract.assertions,assertions.map(({name,expected})=>({name,expected})));
for(const row of assertions)check(row.name,row.expected,row.actual);
const admissions=frames?.filter(x=>x.event==='admission')??[];check('one authentic admission',1,admissions.length);
if(admissions.length===1){const a=admissions[0];check('compiled library patch',frozen.record.patchSHA256,a.compiledPatchSHA256);check('admission actual digest',createHash('sha256').update(JSON.stringify(a.identity)).digest('hex'),a.identitySHA256);check('original source independently authenticated',contract.original,a.identity.files);check('rules bind actual inputs',contract.rulesVersion,a.identity.rules.native);check('no syntax pass invented','pending',a.identity.syntaxCheck.state);check('no source/config execution',[false,false],[a.identity.sourceExecution,a.identity.configExecution]);
 const luac=join(root,'.tmp/rust-luals-suspended-release-01/payload/tool/luac/luac.exe');check('luac material actual executable',hashFile(luac),a.identity.luac.executable_sha256);check('luac material actual bytes',lstatSync(luac).size,a.identity.luac.executable_bytes);
}
check('complete honest terminal',[{event:'end',complete:true,failed:0,fullSourceAcceptance:false}],frames?.filter(x=>x.event==='end').map(({event,complete,failed,fullSourceAcceptance})=>({event,complete,failed,fullSourceAcceptance})));
const report={schema:'lycheedev.source-input-owned.v1',passed:!error&&checks.every(x=>x.passed),artifact,process:proof,handlesReaped,checks,frames,error,scope:'dynamic immutable input admission only; prepare/query, hard cancellation inside synchronous Emmy and full Source acceptance remain separate'};
writeFileSync(join(out,'report.json'),JSON.stringify(report,null,2));console.log(JSON.stringify({report:join(out,'report.json'),passed:report.passed,assertions:assertions.length,checks:checks.length,handlesReaped,error}));if(!report.passed)process.exitCode=1;
