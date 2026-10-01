import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import {execFileSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import {publicationRetry,dependencyChanged} from './shared-publication-retry.mjs';

const target={session:'CON-a',installation:'D:/Game'};
const consumers=new Set(['1/10','2/20']);
const pending=()=>({session:'CON-a',operationState:'prepared',continuation:{session:'CON-a',kind:'wait_external',remainingBudgetMs:1000,blocker:{kind:'slot_reservation',installation:'D:\\Game\\Interface\\AddOns',consumer:'2/20',runtime:'r',nonce:'n',slot:1}}});
const pool=()=>({files:Array.from({length:200},()=>({consumer:'2/20',runtime:'r',nonce:'n',consumed:false}))});

test('slot 200 remains bounded and requires its exact dependency to change',()=>{
  const r=pending();r.continuation.blocker.slot=200;
  const b=publicationRetry(r,target,consumers),p=pool();
  assert.ok(b);
  assert.equal(dependencyChanged(b,p,'old',true),false);
  p.files[199].consumed=true;
  assert.equal(dependencyChanged(b,p,'old',false),true);
  assert.equal(dependencyChanged({kind:'publication_lock'},{files:p.files.slice(0,64)},'old',true),false);
});

test('only exact owned temporary publication blockers qualify',()=>{
  assert.ok(publicationRetry(pending(),target,consumers));
  for(const mutate of [
    r=>r.session='CON-other',r=>r.continuation.session='CON-other',
    r=>r.operationState='execution_unknown',r=>r.continuation.kind='budget_exhausted',
    r=>r.continuation.kind='needs_decision',r=>r.continuation.remainingBudgetMs=0,
    r=>delete r.continuation.remainingBudgetMs,r=>r.continuation.blocker.consumer='3/30',
    r=>r.continuation.blocker.kind='active_driver',r=>r.continuation.blocker.installation='D:/Other',
    r=>r.continuation.blocker.slot=201,
  ]){const r=pending();mutate(r);assert.equal(publicationRetry(r,target,consumers),null);}
});

test('unchanged reservation or unrelated owner progress never unlocks resume',()=>{
  const b=pending().continuation.blocker,p=pool();
  assert.equal(dependencyChanged(b,p,JSON.stringify(p),true),false);
  p.files[1].consumed=true;
  assert.equal(dependencyChanged(b,p,'older',true),false);
  p.files[0].consumed=true;
  assert.equal(dependencyChanged(b,p,'older',false),true);
  p.files[0]={nonce:'new',consumer:'1/10',runtime:'new-runtime'};
  assert.equal(dependencyChanged(b,p,'older',false),true);
});

test('short lease requires observed installation or peer-call progress',()=>{
  const b={kind:'publication_lock'},p=pool(),before=JSON.stringify(p);
  assert.equal(dependencyChanged(b,p,before,false),false);
  assert.equal(dependencyChanged(b,p,before,true),true);
  p.files[0].consumed=true;
  assert.equal(dependencyChanged(b,p,before,false),true);
  assert.equal(dependencyChanged(b,{files:[]},before,true),false);
});

test('post-run audit rejects old 64-slot evidence and accepts current 200-slot evidence',async()=>{
  const root=await fs.mkdtemp(path.join(os.tmpdir(),'lychee-slot-audit-'));
  try {
    for(const slots of [64,200]) {
      const dir=path.join(root,String(slots)),installation=path.join(dir,'game'),project=path.join(dir,'project');
      const parent=path.join(installation,'Interface/AddOns'),logs=path.join(project,'.lycheedev/live/connections');
      await fs.mkdir(logs,{recursive:true});
      await fs.mkdir(path.join(parent,'.lycheedev-window-owners'),{recursive:true});
      await fs.writeFile(path.join(parent,'.lycheedev-slots.json'),JSON.stringify({schema:slots===200?'lycheedev.slots.v3':'lycheedev.slots.v1',files:Array.from({length:slots},()=>({}))}));
      const rows=[
        {kind:'slot_intent',data:{identity:{slots},operation:{request:'wait-resume'},transaction:{envelope:{action:'prepare',nonce:'one'}}}},
        {kind:'automatic_reload_intent',data:{reload:{request:'capacity-one'}}},
        {kind:'closed',data:{closed:true}},
      ];
      await fs.writeFile(path.join(logs,'CON-a.jsonl'),rows.map(r=>JSON.stringify(r)).join('\n'));
      const report={complete:true,targets:[{name:'a',session:'CON-a',project,installation}],steps:Array.from({length:Math.floor((slots-16)/4)+1},(_,i)=>({target:'a',name:`paired-${i}`,exitCode:0}))};
      if(slots===200)report.slotCount=slots;
      const file=path.join(dir,'report.json');await fs.writeFile(file,JSON.stringify(report));
      const audit=fileURLToPath(new URL('./shared-installation-audit.mjs',import.meta.url));
      if(slots===64){assert.throws(()=>execFileSync(process.execPath,[audit,file],{encoding:'utf8',stdio:'pipe'}));continue;}
      const result=JSON.parse(execFileSync(process.execPath,[audit,file],{encoding:'utf8'}));
      assert.equal(result.complete,true);assert.equal(result.slotCount,slots);
      assert.equal(result.targets[0].pairedCommands,47);
    }
  } finally {await fs.rm(root,{recursive:true,force:true});}
});
