import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync,writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {beginScratch,sha256} from './rust-disk.mjs';
import {localOwnedSpawn} from './rust-local-owned-process.mjs';
import {sourcePaths,harnessPaths,harnessSourcePath,snapshotInputs,verifyTestResults} from './rust-local-pure.mjs';
test('minimal harness compiles actual unmodified production module/test paths',()=>{
  assert.equal(sourcePaths.length,7);
  const source=readFileSync('tests/rust-local-pure/src/lib.rs','utf8');
  assert.match(source,/pub mod raw_json;/);
  assert.match(source,/pub mod original_lines;/);
  assert.match(source,/pub mod stderr_observation;/);
  assert.equal(harnessSourcePath('crates/lycheedev-source/src/process/stderr_observation.rs'),'stderr_observation.rs');
  assert.equal(harnessSourcePath('crates/lycheedev-source/src/native/original_lines.rs'),'original_lines.rs');
  assert.equal(harnessSourcePath('crates/lycheedev-core/src/mailbox/code_bytes.rs'),'mailbox/code_bytes.rs');
  assert.throws(()=>harnessSourcePath('../original_lines.rs'));
  assert.equal(new Set(sourcePaths.map(harnessSourcePath)).size,sourcePaths.length);
  assert.equal(source.includes('fn '),false);
  assert.equal(Object.keys(snapshotInputs(process.cwd(),[...sourcePaths,...harnessPaths])).length,10);
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
