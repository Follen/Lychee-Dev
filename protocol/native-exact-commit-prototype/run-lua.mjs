// Read/write isolated fixture files only. No Cargo, OS input, or game access.
import assert from 'node:assert/strict';
import {createHash,createHmac,randomBytes,randomUUID} from 'node:crypto';
import {spawn} from 'node:child_process';
import {mkdirSync,readFileSync,writeFileSync,openSync,writeSync,fsyncSync,closeSync,linkSync,existsSync} from 'node:fs';
import {resolve,join} from 'node:path';
import {performance} from 'node:perf_hooks';
const root=process.cwd(),base=join(root,'protocol/native-exact-commit-prototype');
const lua=process.env.LYCHEEDEV_LUA51||'C:/Program Files (x86)/Lua/5.1/lua.exe';
const evidence=join(root,'.tmp/rust-supervision',`native-exact-commit-lua-${randomUUID()}`);
mkdirSync(evidence,{recursive:true});
const sha=x=>createHash('sha256').update(x).digest('hex');
const inputs=[join(base,'receiver.lua'),join(base,'run-lua.mjs'),join(root,'protocol/native-v1-prototype/crypto.lua'),join(root,'protocol/native-v1-prototype/observation.lua'),lua];
const manifest=()=>inputs.map(path=>({path:resolve(path),sha256:sha(readFileSync(path))}));
const before=manifest();writeFileSync(join(evidence,'inputs-before.json'),JSON.stringify(before,null,2)+'\n',{flag:'wx'});
const traces=[],facts={};
function record(path,bytes){const fd=openSync(path,'wx');try{writeSync(fd,bytes);fsyncSync(fd);}finally{closeSync(fd);}}
// Atomic no-replace hard-link publication from a completely written fixture.
// Directory persistence on Windows is NOT established by this experiment.
function publish(path,bytes){
  if(existsSync(path)){if(readFileSync(path,'utf8')!==bytes)throw Error('immutable_collision');return 'already_published';}
  const temp=path+`.sealed-${randomUUID()}`;record(temp,bytes);linkSync(temp,path);return 'published';
}
function request(number,stage,op='RUN',payload='return 7'){
  const x={binding:'1'.repeat(32),number,nonce:randomBytes(32).toString('hex'),stage,commit:stage+1,op,payload:Buffer.from(payload).toString('hex')};
  x.raw=[x.binding,number.toString(16).padStart(8,'0'),x.nonce,String(stage).padStart(3,'0'),String(stage+1).padStart(3,'0'),op,x.payload].join('|');
  x.digest=sha(x.raw);return x;
}
const wire=(kind,x)=>[kind,x.binding,x.number.toString(16).padStart(8,'0'),x.nonce,x.digest,String(x.stage).padStart(3,'0'),String(x.commit).padStart(3,'0'),x.op,x.payload].join('|');
const source=(kind,x)=>`return "${wire(kind,x)}"\n`;
class World{
  constructor(name){
    this.name=name;this.dir=join(evidence,name);mkdirSync(this.dir);this.lines=[];this.waiter=null;this.output='';this.errors='';this.sent=0;
    this.child=spawn(lua,[join(base,'receiver.lua'),root,this.dir],{stdio:['pipe','pipe','pipe'],windowsHide:true});
    this.closed=new Promise(resolve=>{this.child.once('error',error=>{this.error=error;resolve({error:error.message});});this.child.once('close',(exit,signal)=>resolve({exit,signal}));});
    this.child.stdout.setEncoding('utf8');this.child.stderr.setEncoding('utf8');
    this.child.stdout.on('data',s=>{this.output+=s;if(this.output.length>65536){this.child.kill();return;}this.buffer=(this.buffer||'')+s;let at;while((at=this.buffer.indexOf('\n'))>=0){const line=this.buffer.slice(0,at).replace(/\r$/,'');this.buffer=this.buffer.slice(at+1);if(this.waiter){const fn=this.waiter;this.waiter=null;fn(line);}else this.lines.push(line);}});
    this.child.stderr.on('data',s=>{this.errors+=s;if(this.errors.length>8192)this.child.kill();});
  }
  async read(){if(this.lines.length)return this.lines.shift();return await Promise.race([new Promise(resolve=>this.waiter=resolve),this.closed.then(x=>{throw Error(`Lua closed ${JSON.stringify(x)}`);}),new Promise((_,reject)=>{const timer=setTimeout(()=>reject(Error('Lua response deadline')),5000);timer.unref();})]);}
  async send(key){assert(this.sent++<128);this.child.stdin.write(key+'\n');const response=await this.read();traces.push({world:this.name,key,response});return response;}
  path(slot){return join(this.dir,`slot-${String(slot).padStart(3,'0')}.lua`);}
  prepare(x){x.started=performance.now();record(join(this.dir,`intent-${x.number}.json`),JSON.stringify(x));return publish(this.path(x.stage),source('STAGE',x));}
  qualify(x,answer){
    assert(answer.startsWith('STATE|'));const f=answer.slice(6).split('|');assert.equal(f.length,11);
    const raw=f.slice(0,10).join('|');assert.equal(f[10],createHmac('sha256',Buffer.alloc(32,0x19)).update('lycheedev.p0.state\0'+raw).digest('hex'));
    assert.deepEqual(f.slice(1,7),[x.binding,'2'.repeat(32),'3'.repeat(32),x.number.toString(16).padStart(8,'0'),x.nonce,x.digest]);
    assert.equal(f[9],'00');assert(performance.now()-x.started<=750,'fixture_observation_expired');
    record(join(this.dir,`authorization-${x.number}.json`),JSON.stringify({request:x,observation:answer,qualifiedElapsed:performance.now()-x.started}));
  }
  commit(x){assert(existsSync(join(this.dir,`authorization-${x.number}.json`)),'authorization_record_required');assert(performance.now()-x.started<=750,'publication_expired');return publish(this.path(x.commit),source('COMMIT',x));}
  async status(){const f=(await this.send('STATUS')).split('|');assert.equal(f[0],'STATUS');return {executions:Number(f[1]),loads:Number(f[2]),callbacks:Number(f[3]),records:f[4]};}
  async stop(){if(!this.error){await this.send('EXIT');this.child.stdin.end();}const end=await this.closed;writeFileSync(join(this.dir,'stdout.txt'),this.output,{flag:'wx'});writeFileSync(join(this.dir,'stderr.txt'),this.errors,{flag:'wx'});assert.equal(end.exit,0,JSON.stringify(end));}
}
const active=[];
async function world(name,body){const w=new World(name);active.push(w);assert((await w.read()).startsWith('READY|Lua51'));try{await body(w);}finally{await w.stop();active.splice(active.indexOf(w),1);}}
try{
  // Exactly the audited FIFO ordering E(A), old STAGE, H(A), with publication
  // between callbacks. The host deliberately does not infer an input drain.
  await world('fifo-before-B-authorization',async w=>{
    const a=request(7,1,'RECEIVED','A');w.prepare(a);w.qualify(a,await w.send('STAGE'));w.commit(a);
    assert((await w.send('COMMIT')).startsWith('ACCEPTED|00000007|')); // E(A)
    const b=request(8,3);w.prepare(b);assert((await w.send('STAGE')).startsWith('STATE|')); // old STAGE
    assert.equal(await w.send('COMMIT'),'ERR|empty_slot'); // H(A), B not authorized
    assert.equal((await w.status()).executions,1);facts.fifoOldCommitCannotExecuteUnauthorizedB=true;
    assert.throws(()=>w.commit(b),/authorization_record_required/);
  });
  await world('fifo-after-B-authorization',async w=>{
    const a=request(7,1,'RECEIVED','A');w.prepare(a);w.qualify(a,await w.send('STAGE'));w.commit(a);await w.send('COMMIT');
    const b=request(8,3);w.prepare(b);const observation=await w.send('STAGE');w.qualify(b,observation);w.commit(b);
    assert((await w.send('COMMIT')).startsWith('ACCEPTED|00000008|')); // old key is only a file reader
    assert.equal((await w.status()).executions,2);facts.oldKeyMayExecuteOnlyAfterExactBAuthorization=true;
    assert.equal(await w.send('COMMIT'),'ERR|no_pending');assert.equal((await w.status()).executions,2);
  });
  await world('early-empty-commit-and-recovery',async w=>{
    const a=request(1,1);w.prepare(a);const receipt=await w.send('STAGE');
    assert.equal(await w.send('COMMIT'),'ERR|empty_slot');w.qualify(a,receipt);w.commit(a);
    assert.equal(await w.send('COMMIT'),'ERR|no_pending');assert.equal((await w.status()).executions,0);
    const b=request(2,3);w.prepare(b);w.qualify(b,await w.send('STAGE'));w.commit(b);await w.send('COMMIT');
    assert.equal((await w.status()).executions,1);facts.emptyCommitConsumesAttemptAndNeedsNewPair=true;
  });
  // Every identity component is present in real on-disk COMMIT bytes. Generate
  // a fresh digest for mutated fields, so this tests exact private matching.
  for(const field of ['binding','number','nonce','stage','commit','op','payload'])await world('mismatch-'+field,async w=>{
    const a=request(1,1);w.prepare(a);w.qualify(a,await w.send('STAGE'));
    const b={...a};if(field==='binding')b.binding='4'.repeat(32);if(field==='number')b.number=2;if(field==='nonce')b.nonce='a'.repeat(64);if(field==='stage'){b.stage=3;b.commit=4;}if(field==='commit')b.commit=4;if(field==='op')b.op='RECEIVED';if(field==='payload')b.payload='4142';
    b.raw=[b.binding,b.number.toString(16).padStart(8,'0'),b.nonce,String(b.stage).padStart(3,'0'),String(b.commit).padStart(3,'0'),b.op,b.payload].join('|');b.digest=sha(b.raw);
    publish(w.path(a.commit),source('COMMIT',b));assert((await w.send('COMMIT')).startsWith('ERR|'));assert.equal((await w.status()).executions,0);
  });facts.exactIdentityMutationClassesRejected=7;
  await world('private-gate-and-immutable-publish',async w=>{
    const a=request(1,1);w.prepare(a);w.qualify(a,await w.send('STAGE'));assert.equal(w.commit(a),'published');assert.equal(w.commit(a),'already_published');
    const b=request(2,1);assert.throws(()=>publish(w.path(2),source('COMMIT',b)),/immutable_collision/);
    await w.send('COMBAT_ON');assert.equal(await w.send('COMMIT'),'ERR|private_blocked');assert.equal((await w.status()).loads,1);
    await w.send('COMBAT_OFF');await w.send('COMMIT');assert.equal((await w.status()).executions,1);
    // Inject hostile same-number/different-content file independently of the
    // host journal, whose create_new also rejects that conflicting intent.
    publish(w.path(3),source('STAGE',request(1,3)));assert.equal(await w.send('STAGE'),'ERR|stage_sequence');facts.privateBlockedDoesNotConsumeCommit=true;facts.sameNumberDifferentRequestRejected=true;
  });
  for(const cut of ['prepared','observed','authorized','published'])await world('cut-'+cut,async w=>{
    const a=request(1,1);w.prepare(a);
    if(cut==='prepared'){assert.equal(await w.send('COMMIT'),'ERR|no_pending');}
    else{const receipt=await w.send('STAGE');if(cut!=='observed')w.qualify(a,receipt);if(cut==='published')w.commit(a);
      if(cut==='published'){assert((await w.send('COMMIT')).startsWith('ACCEPTED|'));assert.equal((await w.status()).executions,1);}
      else{assert.equal(await w.send('COMMIT'),'ERR|empty_slot');assert.equal((await w.status()).executions,0);}}
  });facts.publicationCutClasses=4;
  const after=manifest();assert.deepEqual(after,before);writeFileSync(join(evidence,'inputs-after.json'),JSON.stringify(after,null,2)+'\n',{flag:'wx'});
  const result={schema:'lycheedev.protocol-prototype.exact-commit.v1',state:'completed_scoped_lua_checks',facts,traces,node:process.version,luaSHA256:before.at(-1).sha256,scope:'Node host fixture, real Lua51 callbacks and disk slot files; once-loader model; fixed trusted binding',rustHost:'not_run',finalProtocolPassed:false,unproven:['Windows/WoW input and empty-wrapper semantics','cold binding and private authentication isolation','cross-pause expiry and B1 semantics','full process crash/directory durability','500-slot allocator and lifecycle','BODY cleanup/cancellation']};
  record(join(evidence,'result.json'),JSON.stringify(result,null,2)+'\n');console.log(JSON.stringify({state:result.state,evidence,facts}));
}catch(error){for(const w of active){w.child.kill();await w.closed;writeFileSync(join(w.dir,'failure-stdout.txt'),w.output);writeFileSync(join(w.dir,'failure-stderr.txt'),w.errors);}record(join(evidence,'result.json'),JSON.stringify({state:'failed',error:error.stack,facts,traces,finalProtocolPassed:false},null,2)+'\n');console.error(JSON.stringify({state:'failed',evidence,error:error.message}));process.exitCode=1;}
