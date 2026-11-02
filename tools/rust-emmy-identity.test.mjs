import test from 'node:test';
import assert from 'node:assert/strict';
import fs,{mkdirSync,mkdtempSync,readFileSync,rmSync,symlinkSync,writeFileSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join,posix,resolve,win32} from 'node:path';
import {syncBuiltinESMExports} from 'node:module';
import {emmyInventory,emmyOwnedFiles,emmyRecipeFiles,emmyRecipePath,emmyRelativePath,emmySourcePath,emmySupportFiles,emmyTreeDigest,inspectEmmy,verifyEmmyWiring} from './rust-emmy-identity.mjs';
import {emmyPathsOverlap,emmyStageParentComponents,stageEmmySources} from './rust-source-dependencies.mjs';
import {licensedDependencies} from './rust-license-inventory.mjs';
import {hashFile,runCargo,sourceDigest} from './rust-build.mjs';

const repository=resolve(import.meta.dirname,'..');
const license=`MIT License

Copyright (c) 2024-2026 CppCXY

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
`;
// The pinned native engine is the only Source semantic engine: both the Source
// and the adapter edges are mandatory dependencies without an optional switch.
const wiring='[dependencies]\n'+['emmylua_code_analysis','emmylua_parser'].map(name=>`${name} = { path = "../../${emmySourcePath}/crates/${name}", version = "=0.30.0" }`).join('\n');
const adapterWiring=wiring;
function fixture(){
  const root=mkdtempSync(join(tmpdir(),'lycheedev-emmy-custody-'));
  try{
  const put=(path,value)=>{mkdirSync(join(root,path,'..'),{recursive:true});writeFileSync(join(root,path),value);};
  for(const name of [...emmySupportFiles,...emmyOwnedFiles])put(name,readFileSync(join(repository,name)));
  for(const name of emmyRecipeFiles)put(`${emmyRecipePath}/${name}`,readFileSync(join(repository,emmyRecipePath,name)));
  put(`${emmyRecipePath}/patches/test.patch`,'reviewed fixture patch\n');
  for(const name of ['Cargo.toml','Cargo.lock','.cargo/config.toml','release/fixture.json'])put(name,'fixture');
  put('crates/lycheedev-source/Cargo.toml',wiring);
  put('crates/lycheedev-emmy-adapter/Cargo.toml',adapterWiring);
  put(`${emmySourcePath}/LICENSE`,license);
  put(`${emmySourcePath}/Cargo.toml`,'fixture workspace');
  for(const name of ['emmylua_code_analysis','emmylua_parser','emmylua_diagnostic_macro'])put(`${emmySourcePath}/crates/${name}/Cargo.toml`,'fixture crate');
  const identity=JSON.parse(readFileSync(join(root,emmyRecipePath,'identity.json')));
  identity.identity='public-base-binding-context-reconstruction-v1';
  identity.patches=[{path:'patches/test.patch',sha256:hashFile(join(root,emmyRecipePath,'patches/test.patch'))}];
  const reseal=()=>{const actual=emmyInventory(join(root,emmySourcePath));identity.files=actual.files.length;identity.bytes=actual.totalBytes;identity.patchedTreeSHA256=emmyTreeDigest(actual.files);put(`${emmyRecipePath}/files.json`,JSON.stringify(actual.files));put(`${emmyRecipePath}/identity.json`,JSON.stringify(identity));};
  reseal();return{root,put,identity,reseal,cleanup:()=>rmSync(root,{recursive:true,force:true})};
  }catch(error){rmSync(root,{recursive:true,force:true});throw error;}
}
function withFixture(fn){const fixtureData=fixture();try{return fn(fixtureData);}finally{fixtureData.cleanup();}}

test('new identity binds all recipe bytes and full generated inventory without historical manifests',()=>withFixture(({root,put})=>{
  const initial=inspectEmmy(root);assert.equal(initial.historicalPrivatePatchRecovered,false);assert.equal(initial.files.length,5);
  const sourceBefore=sourceDigest(root);put(`${emmyRecipePath}/README.md`,'changed recipe documentation');
  assert.notEqual(inspectEmmy(root).recipeSHA256,initial.recipeSHA256);assert.notEqual(sourceDigest(root),sourceBefore);
  put(`${emmySourcePath}/untracked.rs`,'unapproved source');assert.throws(()=>inspectEmmy(root),/generated_inventory/);
}));
test('modified patch bytes, extra patches and generated source mutations fail closed',()=>{
  for(const [path,text,error] of [[`${emmyRecipePath}/patches/test.patch`,'changed',/patch_digest/],[`${emmyRecipePath}/patches/extra.patch`,'unknown',/unknown_patch/],[`${emmySourcePath}/Cargo.toml`,'changed',/generated_inventory/]])withFixture(({root,put})=>{put(path,text);assert.throws(()=>inspectEmmy(root),error);});
});
test('original MIT bytes and pinned public base cannot be silently replaced',()=>{
  withFixture(({root,put,reseal})=>{put(`${emmySourcePath}/LICENSE`,'different license');reseal();assert.throws(()=>inspectEmmy(root),/original_license_bytes/);});
  withFixture(({root,identity,put})=>{identity.base.commit='0'.repeat(40);put(`${emmyRecipePath}/identity.json`,JSON.stringify(identity));assert.throws(()=>inspectEmmy(root),/base_identity/);});
  withFixture(({root,identity,put})=>{identity.historicalPrivatePatchRecovered=true;put(`${emmyRecipePath}/identity.json`,JSON.stringify(identity));assert.throws(()=>inspectEmmy(root),/identity_schema/);});
});
test('inventory rejects ordering, duplicate/case alias, escape and oversized claims before generated tree reads',()=>{
  for(const mutate of [rows=>rows.reverse(),rows=>{rows[1]={...rows[0]};},rows=>{rows[0].path='../escape';},rows=>{rows[0].bytes=65*2**20;}])withFixture(({root,put})=>{
    const rows=JSON.parse(readFileSync(join(root,emmyRecipePath,'files.json')));mutate(rows);put(`${emmyRecipePath}/files.json`,JSON.stringify(rows));assert.throws(()=>inspectEmmy(root),/emmy\.(?:inventory_|path)/);
  });
  for(const path of ['../escape','/absolute','a\\b','a:b','a//b','a/../b','NUL.rs','a.','a\u0000b'])assert.throws(()=>emmyRelativePath(path),/emmy.path/);
});
test('symbolic links in source entries and source ancestors are rejected',()=>{
  withFixture(({root})=>{symlinkSync(root,join(root,emmySourcePath,'link'),'junction');assert.throws(()=>inspectEmmy(root),/symlink/);});
  withFixture(({root})=>{const path=join(root,emmySourcePath);rmSync(path,{recursive:true});symlinkSync(repository,path,'junction');assert.throws(()=>inspectEmmy(root),/symlink/);});
});
test('tree digest uses Python-compatible ASCII JSON for non-ASCII paths',()=>{
  assert.equal(emmyTreeDigest([{path:'é/😀',bytes:0,sha256:'a'}]),'1b497d5da945f5dc0f5bedce813788b80f6cd32009fe0c35da2850cfc771658e');
});
test('corresponding-source staging copies and rechecks complete recipe, source and MIT license',()=>withFixture(({root})=>{
  const destination=join(root,'staged');mkdirSync(destination);
  const report=stageEmmySources(root,destination),staged=inspectEmmy(destination);
  assert.equal(report.patchedTreeSHA256,staged.patchedTreeSHA256);assert.equal(report.recipeSHA256,inspectEmmy(root).recipeSHA256);
  assert.equal(readFileSync(join(destination,emmySourcePath,'LICENSE'),'utf8'),license);
  assert.throws(()=>stageEmmySources(root,destination),/fresh_stage_required/);
}));
test('staging refuses symlink destination ancestors without writing to their targets',()=>withFixture(({root})=>{
  const destination=join(root,'staged'),external=join(root,'protected');mkdirSync(destination);mkdirSync(external);symlinkSync(external,join(destination,'.tmp'),'junction');
  assert.throws(()=>stageEmmySources(root,destination),/symlink/);assert.deepEqual(fs.readdirSync(external),[]);
}));
test('license closure covers transitive local Emmy crates using verified original MIT and new provenance',()=>withFixture(({root})=>{
  const packages=['emmylua_code_analysis','emmylua_parser','emmylua_diagnostic_macro'].map(name=>({id:name,name,version:'0.30.0',source:null,repository:'https://github.com/CppCXY/emmylua-analyzer-rust',license:'MIT',manifest_path:join(root,emmySourcePath,'crates',name,'Cargo.toml')}));
  const records=licensedDependencies(root,{packages,resolve:{nodes:packages.map(pkg=>({id:pkg.id}))}},[]);
  assert.equal(records.length,3);for(const record of records){assert.equal(record.licenseFiles[0].text,license);assert.equal(record.correspondingSource.historicalPrivatePatchRecovered,false);assert.equal('baseManifestSHA256' in record.correspondingSource,false);assert.equal(record.correspondingSource.patchedTreeSHA256,inspectEmmy(root).patchedTreeSHA256);}
}));
test('active Cargo wiring rejects private paths, commented declarations and missing custody',()=>{
  verifyEmmyWiring(wiring,{optional:false});
  for(const text of [wiring.replaceAll(emmySourcePath,'.tmp/rust-source-binding-context-01/upstream'),wiring.replace('emmylua_parser =','# emmylua_parser ='),wiring+'\nemmylua_parser = {}'])assert.throws(()=>verifyEmmyWiring(text,{optional:false}),/dependency_wiring/);
  withFixture(({root})=>{rmSync(join(root,emmyRecipePath,'identity.json'));assert.throws(()=>runCargo(root,['build'],{execute(){assert.fail('must not execute');}}),/ENOENT/);});
});
test('Source and adapter engine edges are both mandatory and enforced',()=>{
  verifyEmmyWiring(wiring,{optional:false});
  verifyEmmyWiring(adapterWiring,{optional:false});
  assert.throws(()=>verifyEmmyWiring(wiring),/dependency_wiring/);
  assert.throws(()=>verifyEmmyWiring(adapterWiring),/dependency_wiring/);
  assert.throws(()=>verifyEmmyWiring(wiring,{optional:'false'}),/dependency_wiring/);
  for(const text of [wiring.replace('=0.30.0','=0.30.1'),wiring.replaceAll(emmySourcePath,'.tmp/unverified'),wiring+'\nemmylua_parser = {}'])
    assert.throws(()=>verifyEmmyWiring(text,{optional:false}),/dependency_wiring/);
  for(const path of ['crates/lycheedev-source/Cargo.toml','crates/lycheedev-emmy-adapter/Cargo.toml'])
    withFixture(({root,put})=>{put(path,wiring.replace('=0.30.0','=0.30.1'));assert.throws(()=>runCargo(root,['check'],{execute(){assert.fail('must not execute');}}),/dependency_wiring/);});
});
test('adapter-only custody still verifies the pinned recipe before execution',()=>withFixture(({root})=>{
  rmSync(join(root,'crates/lycheedev-source/Cargo.toml'));
  rmSync(join(root,emmyRecipePath,'identity.json'));
  assert.throws(()=>runCargo(root,['check'],{execute(){assert.fail('must not execute');}}),/ENOENT/);
}));
test('injected Cargo fixtures seal new identity and reject source or recipe changes after execution',t=>{
  // Model abundant capacity for tiny fixtures only. No real compiler/process is
  // launched, and production admission quotas/ownership code remain unchanged.
  t.mock.method(fs,'statfsSync',()=>({bavail:100n*1024n**3n,bsize:1n}));syncBuiltinESMExports();
  try{
    withFixture(({root})=>{const expected=inspectEmmy(root);const result=runCargo(root,['check'],{execute(_command,_args,options){assert.equal(options.env.LYCHEEDEV_EMMY_CONTEXT_PATCH_SHA256,expected.patchedTreeSHA256);return{status:0,handlesReaped:true,stdout:Buffer.alloc(0),stderr:Buffer.alloc(0)};}});assert.equal(result.inputHashes.emmy.recipeSHA256,expected.recipeSHA256);assert.equal(result.inputHashes.emmyPatch,expected.patchedTreeSHA256);});
    for(const command of ['check','metadata','fetch'])for(const path of [`${emmySourcePath}/Cargo.toml`,`${emmyRecipePath}/README.md`,'crates/lycheedev-emmy-adapter/Cargo.toml'])withFixture(({root,put})=>{
      assert.throws(()=>runCargo(root,[command],{execute(){put(path,'mutation during compile');return{status:0,handlesReaped:true,stdout:Buffer.alloc(0),stderr:Buffer.alloc(0)};}}),/generated_inventory|source_changed_during_compile|emmy_identity_changed_during_cargo|manifests_changed_during_fetch/);
      assert.equal(fs.existsSync(join(root,'.tmp/rust-disk/channels/ordinary.lease')),false);
    });
  }finally{t.mock.restoreAll();syncBuiltinESMExports();}
});

test('source staging preserves every parent component under Windows path semantics',()=>{
  const components=emmyStageParentComponents(`${emmySourcePath}/crates/emmylua_parser/src/lib.rs`);
  assert.deepEqual(components,['.tmp','emmy-reconstruction-v1','upstream','crates','emmylua_parser','src']);
  assert.equal(win32.join('C:\\stage',...components),'C:\\stage\\.tmp\\emmy-reconstruction-v1\\upstream\\crates\\emmylua_parser\\src');
  assert.throws(()=>emmyStageParentComponents('crates\\escape/Cargo.toml'),/emmy.path/);
});

test('empty generated resource directories are rejected before build and staging',()=>withFixture(({root})=>{
  mkdirSync(join(root,emmySourcePath,'crates/emmylua_code_analysis/resources/unapproved-empty'),{recursive:true});
  assert.throws(()=>inspectEmmy(root),/unexpected_directory/);
  const destination=join(root,'stage-empty-dir');mkdirSync(destination);
  assert.throws(()=>stageEmmySources(root,destination),/unexpected_directory/);
  assert.deepEqual(fs.readdirSync(destination),[]);
  assert.throws(()=>runCargo(root,['check'],{execute(){assert.fail('must not compile');}}),/unexpected_directory/);
}));
test('support tooling is copied and bound to both recipe and source identity',()=>withFixture(({root,put})=>{
  const before=inspectEmmy(root),sourceBefore=sourceDigest(root),destination=join(root,'stage-tools');mkdirSync(destination);
  stageEmmySources(root,destination);
  for(const path of emmySupportFiles)assert.deepEqual(readFileSync(join(root,path)),readFileSync(join(destination,path)));
  put('tools/rust-owned-process.ps1','changed wrapper');assert.notEqual(inspectEmmy(root).recipeSHA256,before.recipeSHA256);assert.notEqual(sourceDigest(root),sourceBefore);
}));

test('dependency names reject non-NFC spellings and case-aliased directories',()=>{
  assert.throws(()=>emmyRelativePath('e\u0301.rs'),/emmy.path/);
  withFixture(({root,put})=>{put(`${emmySourcePath}/CRATES/other/file.rs`,'case alias');assert.throws(()=>inspectEmmy(root),/path_alias|generated_inventory/);});
});

test('Windows source-stage overlap rejects case aliases, ancestors, descendants and equality',()=>{
  const source='C:\\work\\.tmp\\emmy\\upstream';
  for(const destination of ['C:\\WORK\\.tmp\\emmy\\upstream\\stage','C:\\WORK\\.tmp','C:\\WORK\\.tmp\\EMMY\\UPSTREAM'])assert.equal(emmyPathsOverlap(source,destination,win32),true);
  for(const destination of ['C:\\WORK\\.tmp\\emmy\\upstream-other','D:\\work\\.tmp\\emmy\\upstream'])assert.equal(emmyPathsOverlap(source,destination,win32),false);
});

test('staged recipe module imports and owned-process runtime files have a complete local closure',()=>{
  const included=new Set([...emmyRecipeFiles.map(path=>`${emmyRecipePath}/${path}`),...emmySupportFiles]);
  for(const path of included){
    if(!path.endsWith('.mjs'))continue;
    const text=readFileSync(join(repository,path),'utf8');
    for(const match of text.matchAll(/\bfrom\s*['"]([^'"]+)['"]|\bimport\s*\(\s*['"]([^'"]+)['"]\s*\)|\bimport\s*['"]([^'"]+)['"]/g)){
      const specifier=match[1]||match[2]||match[3];if(specifier.startsWith('node:'))continue;
      assert(specifier.startsWith('.'),`unpackaged module: ${path}: ${specifier}`);
      assert(included.has(posix.normalize(posix.join(posix.dirname(path),specifier))),`missing staged import: ${path}: ${specifier}`);
    }
  }
  assert(included.has('tools/rust-owned-process.ps1'));
  assert.match(readFileSync(join(repository,'tools/rust-owned-process.mjs'),'utf8'),/rust-owned-process\.ps1/);
  assert(included.has('tools/rust-build-auxiliary.json'));
  assert.match(readFileSync(join(repository,'tools/rust-owned-process.ps1'),'utf8'),/rust-build-auxiliary\.json/);
  assert(included.has('release/rust-disk-policy.json'));
});


test('owned binding contracts are fixed, staged and tamper-evident',()=>withFixture(({root,put})=>{
  const before=inspectEmmy(root),destination=join(root,'stage-owned');mkdirSync(destination);
  stageEmmySources(root,destination);
  for(const path of emmyOwnedFiles)assert.deepEqual(readFileSync(join(root,path)),readFileSync(join(destination,path)));
  assert.deepEqual(readFileSync(join(destination,'Cargo.toml')),readFileSync(join(destination,emmyRecipePath,'standalone-Cargo.toml')));
  put(emmyOwnedFiles[1],'changed owned contract');
  assert.throws(()=>inspectEmmy(root),/owned_contract_identity/);
  assert.throws(()=>stageEmmySources(root,join(root,'refused')),/owned_contract_identity/);
}));

test('unknown owned files cannot hide a build or path dependency',()=>withFixture(({root,put})=>{
  put('crates/lycheedev-binding-contracts/build.rs','fn main() {}');
  assert.throws(()=>inspectEmmy(root),/owned_contract_inventory/);
}));

test('full-source staging preserves product manifest and rejects conflicting owned bytes',()=>{
  withFixture(({root})=>{
    const destination=join(root,'full-stage');mkdirSync(destination);
    writeFileSync(join(destination,'Cargo.toml'),'retained product workspace');
    stageEmmySources(root,destination);
    assert.equal(readFileSync(join(destination,'Cargo.toml'),'utf8'),'retained product workspace');
    for(const path of emmyOwnedFiles)assert.deepEqual(readFileSync(join(root,path)),readFileSync(join(destination,path)));
  });
  withFixture(({root})=>{
    const destination=join(root,'conflict-stage');mkdirSync(join(destination,'crates/lycheedev-binding-contracts/src'),{recursive:true});
    const target=join(destination,'crates/lycheedev-binding-contracts/src/lib.rs');
    writeFileSync(target,'foreign owner bytes');
    assert.throws(()=>stageEmmySources(root,destination),/owned_contract_identity/);
    assert.equal(readFileSync(target,'utf8'),'foreign owner bytes');
  });
});

// A zero-patch identity cannot authorize a changed tree merely by omitting the patch.
test('unmodified recipe rejects patches and non-base generated inventory',()=>{
  withFixture(({root,identity,put})=>{identity.identity='public-upstream-unmodified-v1';put(`${emmyRecipePath}/identity.json`,JSON.stringify(identity));assert.throws(()=>inspectEmmy(root),/unmodified_identity/);});
  withFixture(({root,identity,put})=>{identity.identity='public-upstream-unmodified-v1';identity.patches=[];put(`${emmyRecipePath}/identity.json`,JSON.stringify(identity));assert.throws(()=>inspectEmmy(root),/unmodified_identity/);});
});
