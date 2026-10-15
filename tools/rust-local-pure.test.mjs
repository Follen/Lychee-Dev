import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdirSync,readFileSync,writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {beginScratch,sha256} from './rust-disk.mjs';
import {localOwnedSpawn} from './rust-local-owned-process.mjs';
import {sourcePaths,bindingContractPaths,harnessPaths,harnessSourcePath,snapshotInputs,verifyTestResults,checkedAdmissionRepository} from './rust-local-pure.mjs';
test('minimal harness compiles actual unmodified production module/test paths',()=>{
  assert.equal(sourcePaths.length,8);
  const source=readFileSync('tests/rust-local-pure/src/lib.rs','utf8');
  assert.match(source,/pub mod raw_json;/);
  assert.match(source,/pub mod original_lines;/);
  assert.match(source,/pub mod research_projection;/);
  assert.equal(harnessSourcePath('crates/lycheedev-source/src/native/research_projection.rs'),'research_projection.rs');
  assert.match(source,/pub mod stderr_observation;/);
  assert.equal(harnessSourcePath('crates/lycheedev-source/src/process/stderr_observation.rs'),'stderr_observation.rs');
  assert.equal(harnessSourcePath('crates/lycheedev-source/src/native/original_lines.rs'),'original_lines.rs');
  assert.equal(harnessSourcePath('crates/lycheedev-core/src/mailbox/code_bytes.rs'),'mailbox/code_bytes.rs');
  assert.throws(()=>harnessSourcePath('../original_lines.rs'));
  assert.equal(new Set(sourcePaths.map(harnessSourcePath)).size,sourcePaths.length);
  assert.equal(source.includes('fn '),false);
  assert.equal(Object.keys(snapshotInputs(process.cwd(),[...sourcePaths,...harnessPaths])).length,11);
});
test('harness has isolated pinned official deps and a real optimized symbols profile',()=>{
  const manifest=readFileSync('tests/rust-local-pure/Cargo.toml','utf8');
  assert.match(manifest,/\[workspace\]/);assert.match(manifest,/serde = \{ version = "=1\.0\.229"/);
  assert.match(manifest,/serde_json = \{ version = "=1\.0\.151"/);
  assert.match(manifest,/\[profile.symbols\]\ninherits = "release"\ndebug = 2\nstrip = "none"\nincremental = false/);
});
test('zero, filtered, missing and duplicate assertions never count as passing',()=>{
  const good='test raw_json::tests::example ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;';
  assert.equal(verifyTestResults(good,['example']).passed,1);
  for(const text of [good.replace('1 passed','0 passed'),good.replace('0 filtered','1 filtered'),good.replace('example','different'),good+'\n'+good])assert.throws(()=>verifyTestResults(text,['example']));
  assert.throws(()=>verifyTestResults(good,[]));
});
test('cache custody rejects source mutation, extra files and wrong official archive hash',async()=>{
  const root=process.cwd(),checker=join(root,'tests/rust-local-pure/check_cache.py');
  const job=beginScratch(root,'local-cache-verifier-fixture',{command:['python3','cache custody negative fixtures'],inputs:{checker:sha256(readFileSync(checker))}});
  const script=join(job.directory,'fixture.py');
  writeFileSync(script,`import gzip, hashlib, io, pathlib, sys, tarfile\nsys.path.insert(0, ${JSON.stringify(join(root,'tests/rust-local-pure'))})\nfrom check_cache import verify\nbase=pathlib.Path.cwd()\nregistry='index.crates.io-1949cf8c6b5b557f'\nsource=base/'registry/src'/registry/'fixture-1.0.0'\nsource.mkdir(parents=True)\n(source/'lib.rs').write_bytes(b'unchanged')\ncache=base/'registry/cache'/registry\ncache.mkdir(parents=True)\nraw=io.BytesIO()\nwith tarfile.open(fileobj=raw,mode='w') as tar:\n item=tarfile.TarInfo('fixture-1.0.0/lib.rs');item.size=9;tar.addfile(item,io.BytesIO(b'unchanged'))\narchive=gzip.compress(raw.getvalue())\n(cache/'fixture-1.0.0.crate').write_bytes(archive)\nlock=base/'Cargo.lock'\ntext='version = 4\\n[[package]]\\nname = "fixture"\\nversion = "1.0.0"\\nsource = "registry+https://github.com/rust-lang/crates.io-index"\\nchecksum = "'+hashlib.sha256(archive).hexdigest()+'"\\n'\nlock.write_text(text)\nassert len(verify(lock,base))==1\ndef rejects(code):\n try: verify(lock,base)\n except ValueError as e: assert code in str(e),str(e)\n else: raise AssertionError('accepted '+code)\n(source/'lib.rs').write_bytes(b'changed');rejects('cache.source_bytes')\n(source/'lib.rs').write_bytes(b'unchanged')\n(source/'extra.rs').write_bytes(b'extra');rejects('cache.source_inventory')\nlock.write_text(text.replace(hashlib.sha256(archive).hexdigest(),'0'*64));rejects('cache.archive_hash')\nprint('valid plus 3 rejection cases passed')\n`);
  let result;
  try {
    result=await localOwnedSpawn('/usr/bin/python3',[script],{cwd:job.directory,evidence:job.evidence,writable:[job.directory],env:{...process.env,PYTHONDONTWRITEBYTECODE:'1'},timeout:5000});
    job.seal({state:result.handlesReaped?(result.status===0&&!result.error?'succeeded':'failed'):'interrupted',handlesReaped:result.handlesReaped,logs:result.logs});
    assert.equal(result.error,undefined);assert.equal(result.status,0,result.stderr.toString());assert.equal(result.handlesReaped,true);
  }catch(error){if(!result)job.seal({state:'interrupted',handlesReaped:false});throw error;}
});

test('owned integration target uses the actual path dependency and original test file',()=>{
  assert.deepEqual(bindingContractPaths,[
    'crates/lycheedev-binding-contracts/Cargo.toml',
    'crates/lycheedev-binding-contracts/src/lib.rs',
    'crates/lycheedev-binding-contracts/tests/contracts.rs',
  ]);
  const manifest=readFileSync('tests/rust-local-pure/Cargo.toml','utf8');
  assert.match(manifest,/lycheedev-binding-contracts = \{ path = "\.\.\/\.\.\/crates\/lycheedev-binding-contracts" \}/);
  assert.match(manifest,/\[\[test\]\]\nname = "binding_contracts"\npath = "\.\.\/\.\.\/crates\/lycheedev-binding-contracts\/tests\/contracts.rs"/);
  const names=[...readFileSync(bindingContractPaths[2],'utf8').matchAll(/#\[test\]\s*fn (\w+)\(/g)].map(row=>row[1]);
  assert.equal(names.length,5);assert.equal(new Set(names).size,5);
  const stdout=names.map(name=>`test ${name} ... ok`).join('\n')+'\ntest result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;';
  assert.equal(verifyTestResults(stdout,names).passed,5);
  for(const bad of [stdout.replace('5 passed','0 passed'),stdout.replace('0 ignored','1 ignored'),stdout.replace(names[0],'other'),stdout+'\n'+stdout])assert.throws(()=>verifyTestResults(bad,names));
  assert.equal(Object.keys(snapshotInputs(process.cwd(),[...sourcePaths,...bindingContractPaths,...harnessPaths])).length,14);
});
test('shared admission accepts only an exact preserved registered stage',()=>{
  const root=process.cwd();assert.equal(checkedAdmissionRepository(root),root);
  const outer=beginScratch(root,'local-admission-fixture',{retention:'preserve'});
  const job=beginScratch(outer.directory,'local-owned-contract-runner-stage',{retention:'preserve'});
  const stage=join(job.directory,'stage');mkdirSync(stage);
  assert.throws(()=>checkedAdmissionRepository(stage,outer.directory),/local.unregistered_stage/);
  job.seal({state:'succeeded',handlesReaped:true});
  assert.equal(checkedAdmissionRepository(stage,outer.directory),outer.directory);
  assert.throws(()=>checkedAdmissionRepository(job.directory,outer.directory),/local.unregistered_stage/);
  const original=readFileSync(job.receipt,'utf8'),record=JSON.parse(original);
  for(const changed of [{state:'retired'},{owner:'foreign-owner'},{directory:outer.directory},{handlesReaped:false},{retention:'rebuildable'},{id:outer.id}]){
    writeFileSync(job.receipt,JSON.stringify({...record,...changed}));
    assert.throws(()=>checkedAdmissionRepository(stage,outer.directory),/local.unregistered_stage/);
  }
  writeFileSync(job.receipt,original);outer.seal({state:'succeeded',handlesReaped:true});
});
test('owned cache allowance rejects extra local packages, wrong identity and changed crate bytes',async()=>{
  const root=process.cwd(),checker=join(root,'tests/rust-local-pure/check_cache.py');
  const job=beginScratch(root,'local-owned-cache-verifier-fixture',{command:['python3','fixed owned crate custody negative fixtures'],inputs:{checker:sha256(readFileSync(checker))},retention:'preserve'});
  const script=join(job.directory,'fixture.py');
  writeFileSync(script,String.raw`import gzip, hashlib, io, pathlib, sys, tarfile
sys.path.insert(0, ${JSON.stringify(join(root,'tests/rust-local-pure'))})
from check_cache import verify, OWNED_PATHS
base=pathlib.Path.cwd()
registry='index.crates.io-1949cf8c6b5b557f'
source=base/'registry/src'/registry/'fixture-1.0.0'
source.mkdir(parents=True)
(source/'lib.rs').write_bytes(b'unchanged')
cache=base/'registry/cache'/registry
cache.mkdir(parents=True)
raw=io.BytesIO()
with tarfile.open(fileobj=raw,mode='w') as tar:
 item=tarfile.TarInfo('fixture-1.0.0/lib.rs');item.size=9;tar.addfile(item,io.BytesIO(b'unchanged'))
archive=gzip.compress(raw.getvalue())
(cache/'fixture-1.0.0.crate').write_bytes(archive)
lock=base/'tests/rust-local-pure/Cargo.lock';lock.parent.mkdir(parents=True)
text='version = 4\n[[package]]\nname = "fixture"\nversion = "1.0.0"\nsource = "registry+https://github.com/rust-lang/crates.io-index"\nchecksum = "'+hashlib.sha256(archive).hexdigest()+'"\n[[package]]\nname = "lycheedev-local-pure"\nversion = "0.0.0"\n[[package]]\nname = "lycheedev-binding-contracts"\nversion = "3.1.2"\n'
lock.write_text(text)
identity={}
for name in OWNED_PATHS:
 path=base/name;path.parent.mkdir(parents=True,exist_ok=True)
 data=b'[package]\nname="lycheedev-binding-contracts"\nversion="3.1.2"\n' if name.endswith('Cargo.toml') else b'unchanged'
 path.write_bytes(data);identity[name]=hashlib.sha256(data).hexdigest()
assert len(verify(lock,base,identity))==1
def rejects(code, supplied=identity):
 try: verify(lock,base,supplied)
 except ValueError as e: assert code in str(e),str(e)
 else: raise AssertionError('accepted '+code)
rejects('cache.owned_identity',{})
rejects('cache.owned_identity',{**identity,'arbitrary/file':'0'*64})
rejects('cache.owned_identity',{**identity,'crates/lycheedev-binding-contracts/src/lib.rs':'x'*64})
crate=base/'crates/lycheedev-binding-contracts'
(crate/'src/lib.rs').write_bytes(b'changed');rejects('cache.owned_source_bytes')
(crate/'src/lib.rs').write_bytes(b'unchanged')
(crate/'build.rs').write_bytes(b'extra');rejects('cache.owned_source_bytes');(crate/'build.rs').unlink()
(crate/'extra').mkdir();rejects('cache.owned_inventory');(crate/'extra').rmdir()
(crate/'src/lib.rs').unlink();rejects('cache.owned_inventory')
(crate/'src/lib.rs').symlink_to(source/'lib.rs');rejects('cache.owned_inventory');(crate/'src/lib.rs').unlink();(crate/'src/lib.rs').write_bytes(b'unchanged')
lock.write_text(text+'[[package]]\nname="unknown"\nversion="0.0.0"\n');rejects('cache.unexpected_local_package')
lock.write_text(text.replace('version = "3.1.2"','version = "3.1.3"'));rejects('cache.unexpected_local_package')
lock.write_text(text.split('[[package]]\nname = "lycheedev-binding-contracts"')[0]);rejects('cache.owned_lock_inventory')
lock.write_text(text);assert len(verify(lock,base,identity))==1
print('valid plus 11 fixed owned crate rejection cases passed')
`);
  let result;
  try{
    result=await localOwnedSpawn('/usr/bin/python3',[script],{cwd:job.directory,evidence:job.evidence,writable:[job.directory],env:{...process.env,PYTHONDONTWRITEBYTECODE:'1'},timeout:5000});
    job.seal({state:result.handlesReaped?(result.status===0&&!result.error?'succeeded':'failed'):'interrupted',handlesReaped:result.handlesReaped,logs:result.logs});
    assert.equal(result.error,undefined);assert.equal(result.status,0,result.stderr.toString());assert.equal(result.handlesReaped,true);
  }catch(error){if(!result)job.seal({state:'interrupted',handlesReaped:false});throw error;}
});
