// Coordinator-only entry; blind agents get an argv call tool, not this shell.
import assert from 'node:assert/strict';
import {existsSync} from 'node:fs';
import {dirname, join, resolve} from 'node:path';
import {StringDecoder} from 'node:string_decoder';
import {setTimeout as delay} from 'node:timers/promises';
import {contained} from './rust-disk.mjs';
import {LIMITS} from '../tests/rust-skill-usage/trial-policy.mjs';
import {publishRequest, readBounded, regular} from '../tests/rust-skill-usage/trial-protocol.mjs';
export function activeSession(path) {
  path=resolve(path);regular(dirname(path),true);
  const session=JSON.parse(readBounded(path,8192));assert.equal(session.schema,'lycheedev.skill-trial-session.v1');
  assert.equal(session.receipt,join(dirname(path),'receipt.json'),'trial.session_receipt');
  const owner=JSON.parse(readBounded(session.receipt,16<<20));
  assert.equal(owner.schema,'lycheedev.disk-owner.v1');assert.equal(owner.state,'active','trial.owner_terminal');assert.equal(owner.owner,'skill-data-usage');assert.equal(owner.id,session.ownerId);
  assert.equal(session.inbox,join(owner.directory,'inbox'),'trial.session_inbox');assert.equal(session.inputs,join(owner.directory,'inputs.json'));
  contained(owner.directory,session.inbox);regular(session.inbox,true);
  assert(Number.isFinite(session.deadline)&&Date.now()<session.deadline,'trial.session_deadline');return session;
}
async function submit(path,request) {
  const session=activeSession(path);publishRequest(session.inbox,request);
  const output=join(dirname(resolve(path)),'response-'+String(request.seq).padStart(3,'0')+'.json');
  while(!existsSync(output)){activeSession(path);await delay(LIMITS.pollMillis);}
  const result=JSON.parse(readBounded(output,12<<20));assert.equal(result.seq,request.seq);return result;
}
export async function relay(path,input=process.stdin,output=value=>console.log(JSON.stringify(value)),send=submit) {
  let sequence=1,pending='',total=0,terminal=false;const decoder=new StringDecoder('utf8');
  const consume=async line=>{
    assert(Buffer.byteLength(line)<=LIMITS.requestBytes,'trial.request_bytes');const value=JSON.parse(line);
    assert(value&&typeof value==='object'&&!Array.isArray(value)&&!Object.hasOwn(value,'seq'),'trial.relay_request');
    const request={...value,seq:sequence++},result=await send(path,request);output(result);terminal=['finish','cancel'].includes(request.op);
  };
  try{
    for await(const chunk of input){
      total+=Buffer.byteLength(chunk);assert(total<=LIMITS.inputBytes,'trial.total_input_bytes');pending+=typeof chunk==='string'?chunk:decoder.write(chunk);
      let newline;while((newline=pending.indexOf('\n'))>=0){const line=pending.slice(0,newline);pending=pending.slice(newline+1);if(line.trim()){assert(!terminal,'trial.after_terminal');await consume(line);}}
      assert(Buffer.byteLength(pending)<=LIMITS.requestBytes,'trial.request_bytes');if(terminal)return;
    }
    pending+=decoder.end();if(pending.trim())await consume(pending);
  }finally{
    // EOF, parse failure, stream cancellation all
    // request cancellation. If publication fails, the owner's idle/total limit
    // remains the fail-safe. No uncertain call is retried.
    if(!terminal){try{output(await send(path,{seq:sequence,op:'cancel'}));}catch(error){output({state:'cancel_unconfirmed',reason:error.message});}}
  }
}
export async function main(argv) {
  const [mode,...args]=argv;
  if(mode==='start'){const {runOwnedTrial}=await import('../tests/rust-skill-usage/trial-owned.mjs');runOwnedTrial(resolve('.'),args);return;}
  if(mode==='relay'){assert.equal(args.length,1);await relay(args[0]);return;}
  if(mode==='call'){assert.equal(args.length,3);assert(Buffer.byteLength(args[2])<=LIMITS.requestBytes);console.log(JSON.stringify(await submit(args[0],{seq:Number(args[1]),op:'call',argv:JSON.parse(args[2])})));return;}
  if(mode==='restore-key'){assert.equal(args.length,3);console.log(JSON.stringify(await submit(args[0],{seq:Number(args[1]),op:mode,reason:args[2]})));return;}
  assert(['finish','cancel'].includes(mode)&&args.length===2,'usage: start | call | finish | cancel | restore-key | relay');
  console.log(JSON.stringify(await submit(args[0],{seq:Number(args[1]),op:mode})));
}
if(process.argv[1]&&resolve(process.argv[1])===resolve(import.meta.filename))await main(process.argv.slice(2));
