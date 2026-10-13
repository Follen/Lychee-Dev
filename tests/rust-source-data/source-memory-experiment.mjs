// One sealed test artifact, fixed full Source inputs and unchanged 2 GiB.
// The sole diagnostic switch drops already-consumed owner-side derived data.
import {createHash} from 'node:crypto';
import {existsSync,mkdirSync,readFileSync,writeFileSync} from 'node:fs';
import {join,resolve} from 'node:path';
import {isDeepStrictEqual} from 'node:util';
import {ownedSpawnSync} from '../../tools/rust-owned-process.mjs';

const binary=resolve(process.env.LYCHEEDEV_SOURCE_TEST_BIN??'');
if(!process.env.LYCHEEDEV_SOURCE_TEST_BIN)throw new Error('Exact sealed test binary required');
for(const name of ['LYCHEEDEV_ENVIRONMENT_HOME','LYCHEEDEV_ENVIRONMENT_PIN','LYCHEEDEV_LUALS_ZIP','LYCHEEDEV_SOURCE_INPUT_MANIFEST'])if(!process.env[name])throw new Error(`${name} required`);
const output=resolve(process.argv[2]??'.tmp/rust-source-memory-experiment');
if(existsSync(output))throw new Error('Use a fresh evidence directory');
mkdirSync(output,{recursive:true});
const hash=path=>createHash('sha256').update(readFileSync(path)).digest('hex');
const before=hash(binary),zipSHA256=hash(process.env.LYCHEEDEV_LUALS_ZIP),manifestSHA256=hash(process.env.LYCHEEDEV_SOURCE_INPUT_MANIFEST),runs=[];
const expected=JSON.parse(readFileSync(process.env.LYCHEEDEV_SOURCE_INPUT_MANIFEST,'utf8'));
if(expected.schema!=='lycheedev.source-memory-input.v1'||expected.commit!=='31c7f7b9cc79e56c986b365c06a6afbcf3c9177b'||expected.documents!==4036||expected.files?.length!==4036||expected.luaFiles!==2551||expected.documentPathSHA256SizeDigest!=='88d68f2c4bdb352245cbcb53cd41ec906b294e2ea599ee6374e00be169be7928')throw new Error('Independent fixed Retail manifest differs');
const configurationIdentity=value=>{
 if(!value)return null;
 const config=structuredClone(value);
 // The authenticated temporary workspace location contains each process PID.
 // The number of roots and every other effective LuaLS setting stay exact.
 config['workspace.library']=config['workspace.library'].map((_,index)=>`<locked-library-${index}>`);
 return config;
};
for(const released of [false,true]){
 const name=released?'released':'retained',path=join(output,name);
 mkdirSync(path);
 const report=join(path,'memory.json');
 const run=ownedSpawnSync(binary,['--ignored','--exact','actual_two_full_workspace_batches_under_aggregate_commit_limit','--nocapture'],{cwd:resolve('.'),evidence:path,env:{...process.env,LYCHEEDEV_SOURCE_RELEASE_DERIVED:released?'1':'0',LYCHEEDEV_SOURCE_MEMORY_REPORT:report},timeout:240000});
 const memory=existsSync(report)?JSON.parse(readFileSync(report,'utf8')):null;
 runs.push({name,releaseConsumedDerived:released,exit:run.status,error:run.error?.message,handlesReaped:run.handlesReaped,proof:run.proof,logs:run.logs,report:memory});
 writeFileSync(join(output,'partial.json'),JSON.stringify({binary,binarySHA256:before,runs},null,2)+'\n');
 if(!run.handlesReaped)break;
}
const identity=run=>{
 const report=run.report,input=report?.inputIdentity,locked=input?.actualLockedConfiguration,coverage=locked?.coverage,library=input?.lockedLibrary;
 if(!input||report.sourcePin?.exactCommit!==expected.commit||report.sourcePin?.product!=='retail'||report.sourcePin?.repository!=='wow-ui-source'||input.documents!==expected.documents||input.documentPathSHA256SizeDigest!==expected.documentPathSHA256SizeDigest||coverage?.luaFiles!==expected.luaFiles||coverage?.luaBytes!==expected.luaBytes||coverage?.excludedFiles!==0||!/^([0-9a-f]{64})$/.test(input.definitionsSHA256??'')||library?.sha256!==input.definitionsSHA256||!Number.isSafeInteger(library?.bytes)||library.bytes<=0||!Array.isArray(locked?.configuration?.['workspace.library'])||locked.configuration['workspace.library'].length!==1||report.aggregateCommitLimitBytes!==2**31)return null;
 return {
 sourcePin:run.report.sourcePin,
 documents:run.report.inputIdentity.documents,
 source:run.report.inputIdentity.documentPathSHA256SizeDigest,
 definitions:run.report.inputIdentity.definitionsSHA256,
 library,
 configuration:configurationIdentity(run.report.inputIdentity.actualLockedConfiguration.configuration),
 coverage:run.report.inputIdentity.actualLockedConfiguration.coverage,
 limit:run.report.aggregateCommitLimitBytes,
};};
const after=hash(binary),zipStable=zipSHA256===hash(process.env.LYCHEEDEV_LUALS_ZIP),manifestStable=manifestSHA256===hash(process.env.LYCHEEDEV_SOURCE_INPUT_MANIFEST),identities=runs.map(identity),inputsEqual=identities.length===2&&identities.every(Boolean)&&isDeepStrictEqual(identities[0],identities[1]);
const report={schema:'lycheedev.source-memory-experiment.v1',scope:'Full pinned SDK two-batch session under unchanged 2 GiB; owner derived-object release is the sole switch. This diagnostic does not establish public broker AV repair, stability or performance acceptance.',binary,binarySHA256:before,binaryStable:before===after,zipSHA256,zipStable,manifestSHA256,manifestStable,inputManifest:process.env.LYCHEEDEV_SOURCE_INPUT_MANIFEST,home:process.env.LYCHEEDEV_ENVIRONMENT_HOME,pin:process.env.LYCHEEDEV_ENVIRONMENT_PIN,inputsEqual,runs,passed:before===after&&zipStable&&manifestStable&&inputsEqual&&runs.every(run=>run.exit===0&&!run.error&&run.handlesReaped)};
writeFileSync(join(output,'report.json'),JSON.stringify(report,null,2)+'\n');
console.log(JSON.stringify({report:join(output,'report.json'),passed:report.passed,inputsEqual,runs:runs.map(run=>({name:run.name,exit:run.exit,error:run.error,handlesReaped:run.handlesReaped}))}));
if(!report.passed)process.exitCode=1;
