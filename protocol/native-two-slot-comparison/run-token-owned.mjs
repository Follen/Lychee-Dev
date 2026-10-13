// The parent wrapper owns the whole Node/Lua fixture tree in the existing Job.
// No compiler or client input. Keep this separate from the fixture transcript.
import {ownedSpawnSync} from '../../tools/rust-owned-process.mjs';
import {mkdirSync,readFileSync,writeFileSync} from 'node:fs';
import {randomUUID,createHash} from 'node:crypto';
import {join,resolve,basename} from 'node:path';
import assert from 'node:assert/strict';
const root=process.cwd(),out=join(root,'.tmp/rust-supervision',`two-slot-token-owned-${randomUUID()}`);mkdirSync(out,{recursive:true});
const inputs=['protocol/native-two-slot-comparison/token-receiver.lua','protocol/native-two-slot-comparison/run-token-lua.mjs',
  'protocol/native-two-slot-comparison/run-token-owned.mjs','protocol/native-v1-prototype/crypto.lua',
  'protocol/native-v1-prototype/observation.lua','tools/rust-owned-process.mjs','tools/rust-owned-process.ps1'];
const sha=path=>createHash('sha256').update(readFileSync(join(root,path))).digest('hex');
const snapshot=()=>inputs.map(path=>({path,sha256:sha(path)})),before=snapshot();
writeFileSync(join(out,'inputs-before.json'),JSON.stringify(before,null,2)+'\n',{flag:'wx'});
const sourceSnapshot=join(out,'source-snapshot');mkdirSync(sourceSnapshot);
for(const input of before){const bytes=readFileSync(join(root,input.path));assert.equal(createHash('sha256').update(bytes).digest('hex'),input.sha256);writeFileSync(join(sourceSnapshot,basename(input.path)),bytes,{flag:'wx'});}
const run=ownedSpawnSync(process.execPath,['protocol/native-two-slot-comparison/run-token-lua.mjs'],{cwd:root,evidence:out,env:process.env,timeout:30000});
const after=snapshot();writeFileSync(join(out,'inputs-after.json'),JSON.stringify(after,null,2)+'\n',{flag:'wx'});
writeFileSync(join(out,'process.json'),JSON.stringify({status:run.status,error:run.error?.message,handlesReaped:run.handlesReaped,proof:run.proof,logs:run.logs},null,2)+'\n',{flag:'wx'});
try{
  assert.equal(run.status,0,run.stderr.toString());assert(run.handlesReaped);assert.deepEqual(after,before);
  const lines=run.stdout.toString().trim().split(/\r?\n/);assert.equal(lines.length,1);const result=JSON.parse(lines[0]);assert.equal(result.state,'completed_scoped_token_lua_checks');
  const fixture=resolve(result.evidence);assert(fixture.startsWith(join(root,'.tmp/rust-supervision')+'\\'));
  const record={schema:'lycheedev.protocol-comparison.token-owned.v1',state:'completed_scoped_owned_token_fixture',fixtureEvidence:fixture,
    fixtureResultSHA256:createHash('sha256').update(readFileSync(join(fixture,'result.json'))).digest('hex'),before,after,inputsUnchanged:true,
    process:{status:run.status,handlesReaped:run.handlesReaped,proof:run.proof,logs:run.logs},facts:result.facts,costs:result.costs,
    scope:'one owned Node/real Lua51 fixture tree; modeled text/focus/clock; no real OS or game input',finalProtocolPassed:false,durableResultAndCleanup:'not_run'};
  writeFileSync(join(out,'result.json'),JSON.stringify(record,null,2)+'\n',{flag:'wx'});console.log(JSON.stringify({state:record.state,evidence:out,fixtureEvidence:fixture,facts:record.facts,costs:record.costs}));
}catch(error){writeFileSync(join(out,'failure.json'),JSON.stringify({state:'failed',error:error.stack,status:run.status,proof:run.proof,logs:run.logs,before,after,finalProtocolPassed:false},null,2)+'\n',{flag:'wx'});console.error(JSON.stringify({state:'failed',evidence:out,error:error.message}));process.exitCode=1;}
