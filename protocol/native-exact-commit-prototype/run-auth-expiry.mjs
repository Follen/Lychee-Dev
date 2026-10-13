// Incremental real Lua51 authenticated outcomes and clock model. No OS input.
import assert from 'node:assert/strict';
import {createHash,createHmac,randomBytes,randomUUID} from 'node:crypto';
import {spawn} from 'node:child_process';
import {mkdirSync,readFileSync,writeFileSync,openSync,writeSync,fsyncSync,closeSync,linkSync} from 'node:fs';
import {join} from 'node:path';
import {performance} from 'node:perf_hooks';
const root=process.cwd(),base=join(root,'protocol/native-exact-commit-prototype'),lua=process.env.LYCHEEDEV_LUA51||'C:/Program Files (x86)/Lua/5.1/lua.exe';
const evidence=join(root,'.tmp/rust-supervision',`native-exact-auth-expiry-${randomUUID()}`);mkdirSync(evidence,{recursive:true});
const sha=x=>createHash('sha256').update(x).digest('hex'),key=Buffer.alloc(32,0x19),traces=[],facts={};
const files=[join(base,'receiver-auth-expiry.lua'),join(base,'run-auth-expiry.mjs'),join(root,'protocol/native-v1-prototype/crypto.lua'),join(root,'protocol/native-v1-prototype/observation.lua'),lua];
const inputs=()=>files.map(path=>({path,sha256:sha(readFileSync(path))}));const before=inputs();writeFileSync(join(evidence,'inputs-before.json'),JSON.stringify(before,null,2)+'\n',{flag:'wx'});
function durable(path,bytes){const fd=openSync(path,'wx');try{writeSync(fd,bytes);fsyncSync(fd);}finally{closeSync(fd);}}
function publish(path,bytes){const temp=path+'.sealed';durable(temp,bytes);linkSync(temp,path);}
function request(number=1,stage=1){const x={binding:'1'.repeat(32),number,stage,commit:stage+1,nonce:randomBytes(32).toString('hex'),payload:Buffer.from('return 7').toString('hex'),started:performance.now()};x.raw=[x.binding,number.toString(16).padStart(8,'0'),x.nonce,String(stage).padStart(3,'0'),String(stage+1).padStart(3,'0'),'RUN',x.payload].join('|');x.digest=sha(x.raw);return x;}
function source(kind,x){return `return "${[kind,x.binding,x.number.toString(16).padStart(8,'0'),x.nonce,x.digest,String(x.stage).padStart(3,'0'),String(x.commit).padStart(3,'0'),'RUN',x.payload].join('|')}"\n`;}
function decodeReceipt(wire,x){
  assert(wire.length<=8192);const f=wire.split('|');assert.equal(f.length,16);assert.equal(f[0],'LDEC1');
  const raw=f.slice(0,-1).join('|');assert.equal(f[15],createHmac('sha256',key).update('lycheedev.exact.receipt\0'+raw).digest('hex'));
  assert.deepEqual(f.slice(1,7),[x.binding,'2'.repeat(32),'3'.repeat(32),x.number.toString(16).padStart(8,'0'),x.nonce,x.digest]);
  assert.deepEqual(f.slice(7,9),[String(x.stage).padStart(3,'0'),String(x.commit).padStart(3,'0')]);
  assert(['0','1'].includes(f[10]));return {outcome:f[9],consumed:f[10]==='1',clock:parseInt(f[11],16),deadline:parseInt(f[12],16),executions:Number(f[13]),loads:Number(f[14])};
}
const children=[];
async function world(name,body){
  const dir=join(evidence,name);mkdirSync(dir);const child=spawn(lua,[join(base,'receiver-auth-expiry.lua'),root,dir],{stdio:['pipe','pipe','pipe'],windowsHide:true});children.push(child);
  let output='',stderr='',buffer='',waiter=null;const lines=[];
  const closed=new Promise(resolve=>{child.once('error',error=>resolve({error:error.message}));child.once('close',(exit,signal)=>resolve({exit,signal}));});
  child.stdout.setEncoding('utf8');child.stdout.on('data',s=>{output+=s;buffer+=s;if(output.length>65536)child.kill();let at;while((at=buffer.indexOf('\n'))>=0){const line=buffer.slice(0,at).replace(/\r$/,'');buffer=buffer.slice(at+1);if(waiter){const fn=waiter;waiter=null;fn(line);}else lines.push(line);}});
  child.stderr.setEncoding('utf8');child.stderr.on('data',s=>{stderr+=s;if(stderr.length>8192)child.kill();});
  async function read(){if(lines.length)return lines.shift();return await Promise.race([new Promise(resolve=>waiter=resolve),closed.then(result=>{throw Error(JSON.stringify(result));}),new Promise((_,reject)=>{const t=setTimeout(()=>reject(Error('response_deadline')),5000);t.unref();})]);}
  let sends=0;async function send(command){assert(sends++<64);child.stdin.write(command+'\n');const response=await read();traces.push({world:name,command,response});return response;}
  const path=slot=>join(dir,`slot-${String(slot).padStart(3,'0')}.lua`);
  const host={dir,send,mailbox:async x=>decodeReceipt(await send('MAILBOX'),x),
    prepare(x){durable(join(dir,'intent.json'),JSON.stringify(x));publish(path(x.stage),source('STAGE',x));},
    authorize(x,answer){assert(answer.startsWith('STATE|'));const f=answer.slice(6).split('|');const raw=f.slice(0,10).join('|');assert.equal(f[10],createHmac('sha256',key).update('lycheedev.p0.state\0'+raw).digest('hex'));assert.deepEqual(f.slice(1,7),[x.binding,'2'.repeat(32),'3'.repeat(32),x.number.toString(16).padStart(8,'0'),x.nonce,x.digest]);assert.equal(f[9],'00');assert(performance.now()-x.started<=750);durable(join(dir,'authorized.json'),JSON.stringify({request:x,observation:answer}));},
    commit(x){assert(performance.now()-x.started<=750,'host_current_time_expired');publish(path(x.commit),source('COMMIT',x));},
    currentInputGuard(x){return performance.now()-x.started<=750;},
    injectMismatch(x){const b={...x,nonce:'a'.repeat(64)};b.raw=[b.binding,b.number.toString(16).padStart(8,'0'),b.nonce,String(b.stage).padStart(3,'0'),String(b.commit).padStart(3,'0'),'RUN',b.payload].join('|');b.digest=sha(b.raw);publish(path(x.commit),source('COMMIT',b));},
  };
  assert((await read()).startsWith('READY|Lua51'));
  try{await body(host);assert.equal(await send('EXIT'),'EXIT');child.stdin.end();assert.equal((await closed).exit,0);}
  finally{if(child.exitCode===null){child.kill();await closed;}writeFileSync(join(dir,'stdout.txt'),output,{flag:'wx'});writeFileSync(join(dir,'stderr.txt'),stderr,{flag:'wx'});children.splice(children.indexOf(child),1);}
}
try{
  await world('signed-empty-consumption',async w=>{const x=request();w.prepare(x);const observation=await w.send('STAGE');assert.equal((await w.mailbox(x)).outcome,'staged');assert.equal(await w.send('COMMIT'),'ERR|empty_slot');const receipt=await w.mailbox(x);assert.deepEqual({outcome:receipt.outcome,consumed:receipt.consumed,executions:receipt.executions,loads:receipt.loads},{outcome:'commit_empty_slot',consumed:true,executions:0,loads:2});w.authorize(x,observation);w.commit(x);assert.equal(await w.send('COMMIT'),'ERR|no_pending');assert.equal((await w.mailbox(x)).executions,0);facts.signedEmptyConsumptionAndNoLateExecution=true;});
  await world('signed-exact-rejection',async w=>{const x=request();w.prepare(x);w.authorize(x,await w.send('STAGE'));w.injectMismatch(x);assert.equal(await w.send('COMMIT'),'ERR|commit_exact_mismatch');const receipt=await w.mailbox(x);assert.equal(receipt.outcome,'commit_exact_mismatch');assert(receipt.consumed);assert.equal(receipt.executions,0);facts.signedMismatchUsesExpectedIdentity=true;});
  await world('signed-private-block',async w=>{const x=request();w.prepare(x);w.authorize(x,await w.send('STAGE'));w.commit(x);await w.send('COMBAT_ON');assert.equal(await w.send('COMMIT'),'ERR|private_blocked');const receipt=await w.mailbox(x);assert.equal(receipt.outcome,'private_blocked');assert.equal(receipt.consumed,false);assert.equal(receipt.loads,1);facts.signedPrivateBlockedNoConsumption=true;});
  await world('cross-pause-clock-model-rejects',async w=>{const x=request();w.prepare(x);w.authorize(x,await w.send('STAGE'));w.commit(x);await w.send('CLOCK|10751');assert.equal(await w.send('COMMIT'),'ERR|private_expired');const receipt=await w.mailbox(x);assert.equal(receipt.outcome,'private_expired');assert.equal(receipt.clock-receipt.deadline,1);assert(!receipt.consumed);assert.equal(receipt.executions,0);facts.advancingGameClockRejectsPublishedExpiredCommit=true;});
  await world('frozen-game-clock-counterexample',async w=>{const x=request();w.prepare(x);w.authorize(x,await w.send('STAGE'));w.commit(x);await new Promise(resolve=>setTimeout(resolve,800));assert.equal(w.currentInputGuard(x),false);assert.throws(()=>w.commit(x),/host_current_time_expired/);const oldQueuedKey=await w.send('COMMIT');assert(oldQueuedKey.startsWith('ACCEPTED|'));const receipt=await w.mailbox(x);assert.equal(receipt.outcome,'accepted');assert.equal(receipt.clock,10000);assert.equal(receipt.deadline,10750);assert.equal(receipt.executions,1);facts.frozenGameClockCanAcceptExpiredPreviouslyQueuedKeyCounterexample=true;facts.hostRechecksActualCurrentTimeBeforeNewInput=true;});
  const after=inputs();assert.deepEqual(after,before);writeFileSync(join(evidence,'inputs-after.json'),JSON.stringify(after,null,2)+'\n',{flag:'wx'});
  const result={schema:'lycheedev.protocol-prototype.auth-expiry.v1',state:'completed_scoped_checks_and_expected_counterexample',facts,traces,scope:'real Lua51 privately signed receipt bytes; Node crypto verification; declared game-clock/queued-key models; no WoW/OS input',finalProtocolPassed:false,clockPrimitive:'not_proven_available_in_WoW',outerJob:'not_used_by_Node_runner; no parent-abort cleanup claim'};
  durable(join(evidence,'result.json'),JSON.stringify(result,null,2)+'\n');console.log(JSON.stringify({state:result.state,evidence,facts}));
}catch(error){for(const child of children){child.kill();}durable(join(evidence,'result.json'),JSON.stringify({state:'failed',error:error.stack,facts,traces,finalProtocolPassed:false},null,2)+'\n');console.error(JSON.stringify({state:'failed',evidence,error:error.message}));process.exitCode=1;}
