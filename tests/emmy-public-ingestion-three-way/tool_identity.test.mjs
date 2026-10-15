import test from 'node:test';
import assert from 'node:assert/strict';
import * as fs from 'node:fs';
import {tmpdir} from 'node:os';
import {join,dirname,resolve} from 'node:path';
import {createHash} from 'node:crypto';
import {hostedPythonAlias,toolFileIdentities} from './tool_identity.mjs';

function fixture(t) {
  const root=fs.mkdtempSync(join(fs.realpathSync(tmpdir()),'emmy-tool-identity-'));
  t.after(()=>fs.rmSync(root,{recursive:true,force:true}));
  const python=join(root,'Python','3.12.10','x64','python.exe'),alias=join(dirname(python),'python3.exe');
  fs.mkdirSync(dirname(python),{recursive:true});fs.writeFileSync(python,'fixture executable bytes, never executed');
  return {root,python,alias,policy:{path:alias,target:python},paths:[python,alias]};
}
const digest=bytes=>createHash('sha256').update(bytes).digest('hex');
function duringFirstRead(action) {
  let called=false;
  return {...fs,readSync(...args){const n=fs.readSync(...args);if(n&&!called){called=true;action();}return n;}};
}

test('only the fixed hosted Python x64 root and already-supported releases are eligible',()=>{
  const python='C:\\hostedtoolcache\\windows\\Python\\3.12.10\\x64\\python.exe';
  assert.deepEqual(hostedPythonAlias(python),{path:python.replace('python.exe','python3.exe'),target:python});
  for(const path of [python.replace('C:','D:'),python.replace('hostedtoolcache','checkout'),
    python.replace('Python\\','Python-evil\\'),python.replace('3.12.10','3.11.9'),
    python.replace('3.12.10','3.15.0'),python.replace('x64','x86'),
    python.replace('x64','x64-freethreaded'),python.replace('python.exe','python3.exe'),
    '\\\\server\\share\\Python\\3.12.10\\x64\\python.exe']) {
    assert.throws(()=>hostedPythonAlias(path),/fixed_hosted_python_root/);
  }
});

test('ordinary tool files are hashed and every original path retains its mapping',t=>{
  const f=fixture(t);fs.writeFileSync(f.alias,'ordinary sibling');
  const result=toolFileIdentities(f.paths,f.policy);
  assert.equal(Object.keys(result.files).length,2);assert.equal(Object.keys(result.mappings).length,2);
  assert.equal(result.files[f.python].sha256,digest(fs.readFileSync(f.python)));
  assert.equal(result.mappings[f.alias].target,f.alias);assert.equal(result.mappings[f.alias].link,null);
  assert.deepEqual(toolFileIdentities(f.paths,f.policy),result);
});

test('the official single-leaf alias hashes its checked target without collapsing the alias record',t=>{
  const f=fixture(t);fs.symlinkSync(f.python,f.alias,'file');
  const result=toolFileIdentities(f.paths,f.policy);
  assert.equal(Object.keys(result.files).length,1);assert.equal(Object.keys(result.mappings).length,2);
  assert.equal(result.mappings[f.alias].target,f.python);assert.equal(result.mappings[f.alias].link,f.python);
  assert.equal(result.files[f.python].sha256,digest(fs.readFileSync(f.python)));
  assert.deepEqual(toolFileIdentities(f.paths,f.policy),result);
});

test('an unrelated file alias remains rejected even inside the selected Python root',t=>{
  const f=fixture(t),other=join(dirname(f.python),'pythonw.exe');fs.symlinkSync(f.python,other,'file');
  assert.throws(()=>toolFileIdentities([f.python,other],f.policy),/unapproved_tool_alias/);
});

test('PowerShell and source-looking aliases receive no exception',t=>{
  const f=fixture(t);
  for(const name of ['Microsoft.PowerShell.Commands.Utility.dll','source.rs','Cargo.lock']) {
    const other=join(f.root,name);fs.symlinkSync(f.python,other,'file');
    assert.throws(()=>toolFileIdentities([f.python,other],f.policy),/unapproved_tool_alias/);
  }
});

test('same-byte target escapes, including a sibling lookalike, fail before hashing',t=>{
  const f=fixture(t);
  for(const path of [join(f.root,'outside.exe'),join(dirname(f.python),'python-evil.exe')]) {
    fs.copyFileSync(f.python,path);fs.symlinkSync(path,f.alias,'file');
    assert.throws(()=>toolFileIdentities(f.paths,f.policy),/tool_alias_escape/);fs.unlinkSync(f.alias);
  }
});

test('an indirect alias chain is not the one reviewed mapping',t=>{
  const f=fixture(t),middle=join(dirname(f.python),'middle.exe');
  fs.symlinkSync(f.python,middle,'file');fs.symlinkSync(middle,f.alias,'file');
  assert.throws(()=>toolFileIdentities(f.paths,f.policy),/tool_alias_escape/);
});

test('a linked selected target, broken link and link loop all fail closed',t=>{
  const f=fixture(t),other=join(f.root,'real.exe');fs.renameSync(f.python,other);
  fs.symlinkSync(other,f.python,'file');fs.symlinkSync(f.python,f.alias,'file');
  assert.throws(()=>toolFileIdentities(f.paths,f.policy),/tool_reparse|unapproved_tool_alias/);
  fs.unlinkSync(f.python);fs.unlinkSync(f.alias);fs.symlinkSync(f.python,f.alias,'file');
  assert.throws(()=>toolFileIdentities(f.paths,f.policy));
  fs.symlinkSync(f.alias,f.python,'file');assert.throws(()=>toolFileIdentities(f.paths,f.policy));
});

test('directory symlinks and Windows junctions remain rejected for every tool',t=>{
  const f=fixture(t),linked=join(f.root,'linked');
  fs.symlinkSync(dirname(f.python),linked,process.platform==='win32'?'junction':'dir');
  assert.throws(()=>toolFileIdentities([f.python,join(linked,'python.exe')],f.policy),/tool_reparse/);
});

test('a directory target is never inventoried as a file',t=>{
  const f=fixture(t);fs.mkdirSync(f.alias);
  assert.throws(()=>toolFileIdentities(f.paths,f.policy),/tool_not_file/);
});

test('the alias target must be independently present in the selected tool inventory',t=>{
  const f=fixture(t);fs.symlinkSync(f.python,f.alias,'file');
  assert.throws(()=>toolFileIdentities([f.alias],f.policy),/tool_file_count_or_python/);
});

test('bounded reads reject oversized and concurrently growing targets',t=>{
  const f=fixture(t);fs.symlinkSync(f.python,f.alias,'file');
  assert.throws(()=>toolFileIdentities(f.paths,f.policy,{maxBytes:1}),/tool_file_budget/);
  const bytes=fs.statSync(f.python).size;
  assert.throws(()=>toolFileIdentities(f.paths,f.policy,{maxBytes:bytes,
    io:duringFirstRead(()=>fs.appendFileSync(f.python,'growth'))}),/tool_file_grew/);
});

test('target modification during hashing cannot produce an accepted identity',t=>{
  const f=fixture(t);fs.symlinkSync(f.python,f.alias,'file');
  assert.throws(()=>toolFileIdentities(f.paths,f.policy,{io:duringFirstRead(()=>{
    fs.writeFileSync(f.python,'X'.repeat(fs.statSync(f.python).size));
    const later=new Date(Date.now()+10000);fs.utimesSync(f.python,later,later);
  })}),/tool_handle_drift|tool_target_drift/);
});

test('alias retargeting during its read is rejected even when target bytes match',t=>{
  const f=fixture(t),other=join(dirname(f.python),'other.exe');fs.copyFileSync(f.python,other);fs.symlinkSync(f.python,f.alias,'file');
  // Capture order sorts python.exe before python3.exe. Mutate on the alias open.
  let opens=0;const io={...fs,openSync(...args){const fd=fs.openSync(...args);if(++opens===2){fs.unlinkSync(f.alias);fs.symlinkSync(other,f.alias,'file');}return fd;}};
  assert.throws(()=>toolFileIdentities(f.paths,f.policy,{io}),/tool_alias_escape|tool_mapping_drift/);
});

test('same-target link replacement during hashing is identity drift',t=>{
  const f=fixture(t);fs.symlinkSync(f.python,f.alias,'file');let opens=0;
  const io={...fs,openSync(...args){const fd=fs.openSync(...args);if(++opens===2){
    fs.renameSync(f.alias,join(dirname(f.alias),'retained-old-alias'));fs.symlinkSync(f.python,f.alias,'file');
  }return fd;}};
  assert.throws(()=>toolFileIdentities(f.paths,f.policy,{io}),/tool_mapping_drift/);
});

test('open-handle identity mismatch is rejected and the handle is closed',t=>{
  const f=fixture(t),other=join(f.root,'other.exe');fs.copyFileSync(f.python,other);fs.writeFileSync(f.alias,'sibling');let closed=0;
  const io={...fs,openSync(){return fs.openSync(other,'r');},closeSync(fd){closed++;return fs.closeSync(fd);}};
  assert.throws(()=>toolFileIdentities(f.paths,f.policy,{io}),/tool_open_drift/);assert.equal(closed,1);
});

test('subsequent snapshots detect target content, object and alias-form drift',t=>{
  const f=fixture(t);fs.symlinkSync(f.python,f.alias,'file');const before=toolFileIdentities(f.paths,f.policy);
  fs.unlinkSync(f.alias);fs.symlinkSync('python.exe',f.alias,'file');
  assert.notDeepEqual(toolFileIdentities(f.paths,f.policy),before);
  fs.renameSync(f.python,join(dirname(f.python),'retained-original.exe'));fs.copyFileSync(join(dirname(f.python),'retained-original.exe'),f.python);
  assert.notDeepEqual(toolFileIdentities(f.paths,f.policy).files[f.python].identity,before.files[f.python].identity);
  fs.writeFileSync(f.python,'different bytes');assert.notEqual(toolFileIdentities(f.paths,f.policy).files[f.python].sha256,before.files[f.python].sha256);
});

test('pre-owner failure diagnostics identify the exact requested path',t=>{
  const f=fixture(t),other=join(f.root,'bad.dll');fs.symlinkSync(f.python,other,'file');
  try {toolFileIdentities([f.python,other],f.policy);assert.fail('must reject');}
  catch(error) {assert(error.message.startsWith('windows-contract.tool_identity:'));assert.equal(JSON.parse(error.message.split('windows-contract.tool_identity:')[1]).requested,resolve(other));}
});

test('the Windows entrypoint binds policy after payload authentication and checks snapshots before/after owned work',()=>{
  const run=fs.readFileSync(new URL('./run_windows_three_way.mjs',import.meta.url),'utf8');
  assert(run.indexOf('payloadCheck();')<run.indexOf("await import('./tool_identity.mjs')"));
  assert(run.includes('const pythonAlias=hostedPythonAlias(python);'));
  assert(run.includes('toolFileIdentities([...files],pythonAlias)'));
  assert(run.includes("assert.deepEqual(toolIdentities(),tools,'windows-contract.tool_drift')"));
  assert(run.includes('payloadCheck();assert.deepEqual(toolIdentities(),tools);'));
  // The original strict guards still own source, payload, stage and Cargo config.
  for(const text of ['function noLinks(path)','noLinks(path);const before=lstatSync(path)',
    "if(info.isSymbolicLink())throw Error('windows-contract.reparse')",'noLinks(base);'])assert(run.includes(text));
  assert(!run.includes('toolFileIdentities([...files],pythonAlias,'));
});
