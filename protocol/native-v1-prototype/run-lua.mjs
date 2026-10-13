// Small isolated prototype runner. No build, game process, or deployment.
import {createHash,createHmac,randomUUID} from 'node:crypto';
import {spawnSync} from 'node:child_process';
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import {join,resolve} from 'node:path';
import assert from 'node:assert/strict';
const root=process.cwd(),lua=process.env.LYCHEEDEV_LUA51||'C:/Program Files (x86)/Lua/5.1/lua.exe';
const base=join(root,'protocol/native-v1-prototype');
const evidence=join(root,'.tmp/rust-supervision',`native-v1-lua-${randomUUID()}`);
mkdirSync(evidence,{recursive:true});
const hash=p=>createHash('sha256').update(readFileSync(p)).digest('hex');
const files=['crypto.lua','observation.lua','p0_fixture.lua','apply_counterexample.lua','run-lua.mjs'].map(p=>join(base,p));
files.push(lua);
writeFileSync(join(evidence,'inputs.json'),JSON.stringify({schema:'lycheedev.protocol-prototype.inputs.v1',files:files.map(p=>({path:resolve(p),sha256:hash(p)}))},null,2)+'\n',{flag:'wx'});
const runs=[];
try{
  for(const script of ['p0_fixture.lua','apply_counterexample.lua']){
    const result=spawnSync(lua,[join(base,script),root],{input:'b'.repeat(64)+'\n',encoding:'utf8',timeout:30000,maxBuffer:32768,windowsHide:true});
    writeFileSync(join(evidence,`${script}.stdout.txt`),result.stdout||'',{flag:'wx'});
    writeFileSync(join(evidence,`${script}.stderr.txt`),result.stderr||'',{flag:'wx'});
    const run={script,exit:result.status,signal:result.signal,error:result.error?.message};runs.push(run);
    assert.equal(result.status,0,JSON.stringify(run));
    const entries=result.stdout.trim().split(/\r?\n/).map(line=>{const at=line.indexOf('\t');assert(at>0);return [line.slice(0,at),line.slice(at+1)];});
    const frames=Object.fromEntries(entries);assert.equal(Object.keys(frames).length,entries.length);
    if(script==='p0_fixture.lua'){
      for(const label of ['OLD1','OLD2','CURRENT_BLOCKED','CURRENT_READY']){
        const wire=frames[label],at=wire.lastIndexOf('|'),raw=wire.slice(0,at),mac=wire.slice(at+1);
        assert.equal(mac,createHmac('sha256',Buffer.alloc(32,0x19)).update('lycheedev.p0.state\0'+raw).digest('hex'));
      }
      assert.equal(frames.PRIVATE_BLOCKED,'true');
      assert.equal(frames.CURRENT_BLOCKED.split('|')[9],'01');
      assert.equal(frames.CURRENT_READY.split('|')[9],'00');
      run.facts={realLua51:true,independentNodeAllMacsValid:true,oldReadyRetainedWhilePrivateCombat:true,currentNonce:'b'.repeat(64),cpu8KiBHmac10Seconds:frames.LUA_8K_HMAC_10_CPU_SECONDS};
    }else{
      assert.equal(frames.OLD_APPLY_NEW_STAGE,'counterexample_reproduced');
      assert.equal(frames.HOST_SAW_NEW_RECEIPT,'false');
      assert.equal(frames.UNKNOWN_APPLY_NEW_STAGE,'rejected_by_admission');
      run.facts=frames;
    }
  }
  writeFileSync(join(evidence,'result.json'),JSON.stringify({schema:'lycheedev.protocol-prototype.lua.v1',state:'completed_scoped_lua_checks',runs,rustGate:'not_run_by_this_runner',finalProtocolPassed:false},null,2)+'\n',{flag:'wx'});
  console.log(JSON.stringify({state:'completed_scoped_lua_checks',evidence,runs}));
}catch(error){
  writeFileSync(join(evidence,'result.json'),JSON.stringify({schema:'lycheedev.protocol-prototype.lua.v1',state:'failed',error:error.message,runs,finalProtocolPassed:false},null,2)+'\n',{flag:'wx'});
  console.error(JSON.stringify({state:'failed',evidence,error:error.message}));process.exitCode=1;
}
