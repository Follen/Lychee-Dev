import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdtempSync,readFileSync} from 'node:fs';
import {join} from 'node:path';
import {ownedSpawnSync} from './rust-owned-process.mjs';

function fixture(t){const directory=mkdtempSync(join(process.cwd(),'.tmp/rust-process-test-'));t.diagnostic(`Preserved process evidence: ${directory}`);return {cwd:process.cwd(),env:process.env,evidence:directory,timeout:15000,orphanMs:200};}
test('Windows Job preserves binary output and exact argv; all handles and tree are reaped',{skip:process.platform!=='win32'},t=>{
  const options=fixture(t),args=['a b','quoted"value','trailing slash \\','中文'];
  const result=ownedSpawnSync(process.execPath,['-e','process.stdout.write(Buffer.from([0,255,128]));process.stderr.write(JSON.stringify(process.argv.slice(1)));',...args],options);
  assert.equal(result.error,undefined);assert.equal(result.status,0);assert.equal(result.handlesReaped,true);
  assert.deepEqual(result.stdout,Buffer.from([0,255,128]));assert.deepEqual(JSON.parse(result.stderr),args);
  assert.equal(result.proof.forced,false);assert.equal(result.proof.treeReaped,true);assert.equal(result.proof.handlesClosed,true);
});
test('parent exit with a real detached child does not count as tree completion',{skip:process.platform!=='win32'},t=>{
  const options=fixture(t),pidFile=join(options.evidence,'owned-child.json');
  const code=`const {spawn}=require('child_process');const child=spawn(process.execPath,['-e','setInterval(()=>{},1000)'],{detached:true,stdio:'ignore'});require('fs').writeFileSync(process.argv[1],JSON.stringify({pid:child.pid}));child.unref();`;
  const result=ownedSpawnSync(process.execPath,['-e',code,pidFile],options);
  const child=JSON.parse(readFileSync(pidFile));assert.ok(child.pid>0);assert.equal(result.status,0);
  assert.match(result.error.message,/forced_tree_termination/);assert.equal(result.proof.parentExitedBeforeChildren,true);assert.equal(result.proof.forced,true);
  assert.equal(result.proof.treeReaped,true);assert.equal(result.proof.activeProcesses,0);assert.equal(result.handlesReaped,true);
  assert.throws(()=>process.kill(child.pid,0),error=>error.code==='ESRCH');
});
test('deadline terminates the exact owned Job and verifies kernel tree exit',{skip:process.platform!=='win32'},t=>{
  const options={...fixture(t),timeout:200};
  const result=ownedSpawnSync(process.execPath,['-e','setInterval(()=>{},1000)'],options);
  assert.ok(result.error);assert.equal(result.proof.deadlineHit,true);assert.equal(result.proof.forced,true);assert.equal(result.handlesReaped,true);assert.equal(result.proof.activeProcesses,0);
});
test('a parent may exit first while its finite child finishes normally within the drain budget',{skip:process.platform!=='win32'},t=>{
  const options={...fixture(t),orphanMs:1500};
  const code=`const child=require('child_process').spawn(process.execPath,['-e','setTimeout(()=>{},200)'],{detached:true,stdio:'ignore'});child.unref();`;
  const result=ownedSpawnSync(process.execPath,['-e',code],options);
  assert.equal(result.error,undefined);assert.equal(result.status,0);assert.equal(result.handlesReaped,true);assert.equal(result.proof.parentExitedBeforeChildren,true);assert.equal(result.proof.forced,false);
});
test('unbounded child output is capped and its owned tree is reaped before return',{skip:process.platform!=='win32'},t=>{
  const options=fixture(t),code=`const bytes=Buffer.alloc(65536,65);for(;;)require('fs').writeSync(1,bytes);`;
  const result=ownedSpawnSync(process.execPath,['-e',code],options);
  assert.match(result.error.message,/output_budget/);assert.equal(result.handlesReaped,true);assert.equal(result.proof.outputLimit,true);assert.ok(result.stdout.length<=16*1024**2);assert.equal(result.proof.outputReadersComplete,true);
});
test('a caller flag cannot grant the Cargo helper exception to another executable',{skip:process.platform!=='win32'},t=>{
  const options={...fixture(t),cargoBuild:true};
  const code=`const child=require('child_process').spawn(process.execPath,['-e','setInterval(()=>{},1000)'],{detached:true,stdio:'ignore'});child.unref();`;
  const result=ownedSpawnSync(process.execPath,['-e',code],options);
  assert.equal(result.proof.helperAdmission.state,'disabled');assert.equal(result.proof.helperAdmission.reason,'owned.not_cargo_build');
  assert.equal(result.proof.knownAuxiliaryCleanup,false);assert.match(result.error.message,/forced_tree_termination/);assert.equal(result.handlesReaped,true);
});

test('FILETIME evidence preserves decimal strings for root and actual owned descendants',{skip:process.platform!=='win32'},t=>{
  const options=fixture(t),code=`const child=require('child_process').spawn(process.execPath,['-e','setInterval(()=>{},1000)'],{detached:true,stdio:'ignore'});child.unref();`;
  const result=ownedSpawnSync(process.execPath,['-e',code],options);
  assert.match(result.error.message,/forced_tree_termination/);assert.equal(result.handlesReaped,true);
  const rows=result.proof.atDrainDeadline;assert.ok(rows.length>0);
  const raw=readFileSync(result.logs.find(path=>path.endsWith('.receipt.json')),'utf8');
  const rawTimes=[...raw.matchAll(/"creationFileTime"\s*:\s*"(\d+)"/g)].map(match=>match[1]);
  const times=[result.proof.creationFileTime,...rows.map(row=>row.creationFileTime)];
  assert.deepEqual(rawTimes.sort(),[...times].sort());
  for(const value of times){assert.equal(typeof value,'string');assert.match(value,/^[1-9]\d+$/);assert.ok(BigInt(value)>BigInt(Number.MAX_SAFE_INTEGER));assert.equal(BigInt(JSON.parse(JSON.stringify(value))).toString(),value);}
  for(const row of rows){assert.equal(row.error,null);assert.equal(row.jobMember,true);assert.ok(BigInt(row.creationFileTime)>=BigInt(result.proof.creationFileTime));}
});
