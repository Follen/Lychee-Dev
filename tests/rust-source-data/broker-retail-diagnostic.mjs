// Own the entire public CLI/broker/LuaLS/observer lifecycle in one outer Job.
// A Job per CLI invocation would destroy the persistent broker before reuse.
import {createHash} from 'node:crypto';
import {existsSync,lstatSync,mkdirSync,readFileSync,writeFileSync} from 'node:fs';
import {resolve,join,dirname} from 'node:path';
import {isDeepStrictEqual} from 'node:util';
import {contained} from '../../tools/rust-disk.mjs';
import {hashFile} from '../../tools/rust-build.mjs';
import {ownedSpawnSync} from '../../tools/rust-owned-process.mjs';

const root=resolve('.');
for(const name of ['LYCHEEDEV_RELEASE_ROOT','LYCHEEDEV_ENVIRONMENT_HOME','LYCHEEDEV_ENVIRONMENT_PIN','LYCHEEDEV_SOURCE_INPUT_MANIFEST'])if(!process.env[name])throw new Error(`${name} required`);
const release=contained(root,resolve(process.env.LYCHEEDEV_RELEASE_ROOT));
const out=contained(root,resolve(process.argv[2]??'.tmp/rust-source-broker-diagnostic'));
if(existsSync(out))throw new Error('Fresh evidence directory required');
const read=path=>{const info=lstatSync(path);if(!info.isFile()||info.isSymbolicLink()||info.size>(128<<20))throw new Error('Invalid bounded input');return readFileSync(path);};
const hash=path=>path.endsWith('.pdb')?hashFile(path):createHash('sha256').update(read(path)).digest('hex');
const manifestPath=contained(root,resolve(process.env.LYCHEEDEV_SOURCE_INPUT_MANIFEST));
const input=JSON.parse(read(manifestPath));
if(input.schema!=='lycheedev.source-memory-input.v1'||input.commit!=='31c7f7b9cc79e56c986b365c06a6afbcf3c9177b'||input.documents!==4036||input.files?.length!==4036||input.luaFiles!==2551||input.luaBytes!==23354176||input.documentPathSHA256SizeDigest!=='88d68f2c4bdb352245cbcb53cd41ec906b294e2ea599ee6374e00be169be7928')throw new Error('Independent full Retail input manifest differs');
const identity=JSON.parse(read(join(release,'diagnostic-identity.json')));
if(identity.schema!=='lycheedev.source-diagnostic-release.v1'||!Array.isArray(identity.resources)||identity.resources.length>20000)throw new Error('Sealed diagnostic release required');
const baseline=contained(root,identity.baselineRelease);
const observerSources=[
 'tests/rust-source-data/broker-retail-diagnostic.mjs',
 'tests/rust-source-data/broker-retail-exit-observe.mjs',
 'tests/rust-source-data/observe-broker-process.ps1',
 'tools/rust-owned-process.mjs','tools/rust-owned-process.ps1',
 'tools/rust-disk.mjs','release/rust-disk-policy.json',
 'tools/rust-build-auxiliary.json',
];
function snapshot(){
 const resources=identity.resources.map(resource=>{
  const path=contained(release,join(release,'payload',resource.path)),original=contained(baseline,join(baseline,'payload',resource.path));
  const actual={path:resource.path,bytes:lstatSync(path).size,sha256:hash(path)};
  if(!isDeepStrictEqual(actual,resource)||lstatSync(original).size!==resource.bytes||hash(original)!==resource.sha256)throw new Error(`Original or copied Source tool resource changed: ${resource.path}`);
  return actual;
 });
 const current={manifestSHA256:hash(manifestPath),binarySHA256:hash(join(release,'native/windows-amd64/lycheedev.exe')),pdbSHA256:hash(join(release,'native/windows-amd64/lycheedev.pdb')),releaseManifestSHA256:hash(join(release,'release.json')),baselineManifestSHA256:hash(join(baseline,'release.json')),baselineBinarySHA256:hash(join(baseline,'native/windows-amd64/lycheedev.exe')),debugger:process.env.LYCHEEDEV_BROKER_DEBUGGER?{path:resolve(process.env.LYCHEEDEV_BROKER_DEBUGGER),sha256:hash(resolve(process.env.LYCHEEDEV_BROKER_DEBUGGER))}:null,resources};
 for(const key of ['binarySHA256','pdbSHA256','releaseManifestSHA256','baselineManifestSHA256','baselineBinarySHA256'])if(current[key]!==identity[key])throw new Error(`Sealed ${key} differs`);
 return {...current,observerSources:observerSources.map(path=>({path,bytes:lstatSync(join(root,path)).size,sha256:hash(join(root,path))}))};
}
const before=snapshot();mkdirSync(out,{recursive:true});
for(const source of before.observerSources){
 const target=contained(out,join(out,'source-snapshot',source.path));
 mkdirSync(dirname(target),{recursive:true});writeFileSync(target,read(join(root,source.path)),{flag:'wx'});
 if(hash(target)!==source.sha256)throw new Error(`Observer source snapshot changed: ${source.path}`);
}
writeFileSync(join(out,'input-before.json'),JSON.stringify(before,null,2)+'\n');
const run=ownedSpawnSync(process.execPath,['tests/rust-source-data/broker-retail-exit-observe.mjs',join(out,'observation')],{cwd:root,evidence:out,timeout:360000,env:{...process.env,LYCHEEDEV_RELEASE_ROOT:release,LYCHEEDEV_RUST_BIN:join(release,'native/windows-amd64/lycheedev.exe'),LYCHEEDEV_BROKER_OBSERVE_SECOND:'1'}});
const processResult={exit:run.status,error:run.error?.message,handlesReaped:run.handlesReaped,proof:run.proof,logs:run.logs};
writeFileSync(join(out,'process.json'),JSON.stringify(processResult,null,2)+'\n');
let observation=null,after=null,auditError;
try{observation=JSON.parse(read(join(out,'observation/report.json')));after=snapshot();}catch(error){auditError=String(error);}
const audited=['first','second'].map(name=>{
 const record=observation?.records?.find(record=>record.name===name),result=record?.envelope?.result;
 const coverage=result?.semanticAnalysis?.workspaceCoverage,runtime=result?.semanticRuntime;
 const matches=record?.exit===0&&result?.commit===input.commit&&result.product==='retail'&&result.repository==='wow-ui-source'&&result.semanticAnalysis.state==='complete'&&coverage?.excludedFiles===0&&coverage.luaFiles===input.luaFiles&&coverage.luaBytes===input.luaBytes&&coverage.sourcePin?.exactCommit===input.commit&&result.environmentManifest?.definitionsSHA256==='d5c3071e9c696cc19b02f609cae3fe8be19089ee5e434537bdf81f3f81b7c103'&&runtime?.sha256==='fdb9a59108cf62517813c97fa5549b0e16d1ef0688306bac728b08434db7e4cd'&&runtime.bytes===4453980&&runtime.version==='3.19.1';
 return {name,matches,exit:record?.exit,coverage:coverage??null,runtime:runtime??null};
});
const stable=after!==null&&isDeepStrictEqual(before,after);
const report={schema:'lycheedev.source-broker-diagnostic.v1',scope:'Full pinned Retail public two-request retained broker with held original process handle. This is one diagnostic execution, not AV causality, repeated stability, lifecycle, performance or full candidate acceptance.',debuggerAttached:!!process.env.LYCHEEDEV_BROKER_DEBUGGER,inputManifest:manifestPath,input:before,inputsStable:stable,after,process:processResult,observations:audited,observationReport:join(out,'observation/report.json'),observationPassed:observation?.passed??false,auditError,passed:run.status===0&&!run.error&&run.handlesReaped&&stable&&observation?.passed===true&&audited.every(record=>record.matches)};
writeFileSync(join(out,'report.json'),JSON.stringify(report,null,2)+'\n');
console.log(JSON.stringify({report:join(out,'report.json'),passed:report.passed,inputsStable:stable,exit:run.status,error:run.error?.message,handlesReaped:run.handlesReaped,observations:audited.map(({name,matches,exit})=>({name,matches,exit}))}));
if(!report.passed)process.exitCode=1;
