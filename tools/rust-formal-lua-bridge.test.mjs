// Controller-only parsing tests: never compile Rust, launch Lua or run a test EXE.
import assert from 'node:assert/strict';
import test from 'node:test';
import {resolve, join} from 'node:path';
import {readFileSync, mkdtempSync, mkdirSync, rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {fileURLToPath} from 'node:url';
import {SUPPORT, bridgePaths, selectTestExecutable, verifyTestOutput} from './rust-formal-lua-bridge.mjs';
import {BASE, ORACLE, MANIFEST_SHA256, RELEASE_FIELDS_SHA256, SUPPORT as ORACLE_SUPPORT, authenticate, hash} from '../tests/rust-parity/formal-lua-3.2.3/assertions.mjs';
const root = fileURLToPath(new URL('..', import.meta.url));
const target = resolve('target');
const artifact = {reason:'compiler-artifact', target:{name:'formal_lua_bridge',kind:['test']},
  profile:{test:true}, executable:join(target,'debug/deps/formal_lua_bridge-a1b2c3.exe')};
const complete = {reason:'build-finished',success:true};
const jsonl = rows => rows.map(row=>JSON.stringify(row)).join('\n')+'\n';
const cases = ['normal','after_bind','after_prepare','after_commit','after_confirm','after_release','reload_bind','compile_error','cleanup_error'].map(name=>`formal_lua_recovery_${name}`);
const output = (names=cases, summary='9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;') => Buffer.from(
  `running ${names.length} tests\n${names.map(name=>`test ${name} ... ok`).join('\n')}\n\ntest result: ok. ${summary} finished in 0.20s\n`);

test('formal and prototype entries retain owned user environments and existing bounded execution recipes',()=>{
  const formal=readFileSync(join(root,'tools/rust-formal-lua.mjs'),'utf8');
  assert.match(formal,/cleanLuaEnvironment\(ownedUserEnvironment\(job.directory, env\)\)/);
  assert.match(formal,/cleanLuaEnvironment\(ownedUserEnvironment\(job.directory\)\)/);
  const bridge=readFileSync(join(root,'tools/rust-formal-lua-bridge.mjs'),'utf8');
  assert.match(bridge,/oneShot: false, isolateUser: true/);
  assert.match(bridge,/\.\.\.ownedUserEnvironment\(job.directory\), LYCHEEDEV_FORMAL_LUA_BRIDGE_ADMISSION: admissionPath/);
  assert.match(bridge,/timeout: 255000/);
  for(const path of ['protocol/native-v1-prototype/run-rust.mjs','protocol/native-exact-commit-prototype/run-rust.mjs']){
    const source=readFileSync(join(root,path),'utf8');
    assert.match(source,/oneShot:false,isolateUser:true/);
    assert.match(source,/--locked/);assert.match(source,/--offline/);
  }
  for(const [path,key] of [['protocol/native-lifecycle-prototype/run-rust.mjs','LYCHEEDEV_LIFECYCLE_PROTOTYPE_ROOT'],
    ['protocol/native-lifecycle-prototype/run-negative-original.mjs','LYCHEEDEV_LIFECYCLE_NEGATIVE_ROOT'],
    ['protocol/native-lifecycle-revision/run-rust.mjs','LYCHEEDEV_LIFECYCLE_REVISION_ROOT']]){
    const source=readFileSync(join(root,path),'utf8');
    assert.match(source,new RegExp(`\\.\\.\\.ownedUserEnvironment\\(evidence\\),${key}:root`));
    assert.match(source,/timeout:300000/);assert(!source.includes('--one-shot'));
    assert.match(source,/sourceTreeBefore=sourceDigest\(root\)/);assert.match(source,/actual-input-snapshot/);
  }
});

test('selects exactly the current named integration artifact',()=>{
  const selected=selectTestExecutable(jsonl([{reason:'compiler-artifact',target:{name:'dependency'}},artifact,complete]),target);
  assert.equal(selected.executable,artifact.executable); assert.deepEqual(selected.row,artifact);
});
for(const [name, rows] of [
  ['missing',[complete]], ['duplicate',[artifact,artifact,complete]],
  ['wrong target',[{...artifact,target:{name:'native_e2e',kind:['test']}},complete]],
  ['ordinary binary',[{...artifact,target:{name:'formal_lua_bridge',kind:['bin']}},complete]],
  ['non-test profile',[{...artifact,profile:{test:false}},complete]],
  ['missing completion',[artifact]], ['failed completion',[artifact,{...complete,success:false}]],
  ['duplicate completion',[artifact,complete,complete]],
  ['foreign path',[{...artifact,executable:resolve('foreign/formal_lua_bridge-a1b2c3.exe')},complete]],
  ['wrong exe basename',[{...artifact,executable:join(target,'debug/deps/native_e2e-a1.exe')},complete]],
]) test(`rejects ${name}`,()=>assert.throws(()=>selectTestExecutable(jsonl(rows),target)));
test('requires structured Cargo JSON',()=>assert.throws(()=>selectTestExecutable('not JSON\n',target)));
test('accepts all nine exact zero-skip case results',()=>{
  assert.equal(verifyTestOutput(output()).passed,9);
  assert.equal(verifyTestOutput(Buffer.from(output().toString().replaceAll('\n','\r\n'))).passed,9);
});
for(const [name, bytes] of [
  ['zero tests',output([], '0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;')],
  ['missing case',output(cases.slice(1))], ['duplicate case',output([...cases.slice(1),cases[1]])],
  ['foreign test',output([...cases.slice(1),'native_e2e'])],
  ['ignored',output(cases,'9 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out;')],
  ['filtered',output(cases,'9 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out;')],
  ['failure',output(cases,'9 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out;')],
  ['double summary',Buffer.concat([output(),Buffer.from('test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0s\n')])],
]) test(`rejects ${name} result`,()=>assert.throws(()=>verifyTestOutput(bytes)));

test('Node admission and Rust bridge authenticate the same current 113-file manifest and exact release fields',()=>{
  const source=readFileSync(join(root,'crates/lycheedev-live/tests/support/formal_lua_fixture.rs'),'utf8');
  const constant=name=>source.match(new RegExp(`pub const ${name}: &str =\\s*"([a-f0-9]+)";`))?.[1];
  assert.equal(constant('ORACLE'),ORACLE); assert.equal(constant('MANIFEST_SHA256'),MANIFEST_SHA256);
  assert.equal(constant('RELEASE_FIELDS_SHA256'),RELEASE_FIELDS_SHA256);
  const includes=[...source.matchAll(/include_bytes!\("\.\.\/\.\.\/\.\.\/\.\.\/([^"\n]+)"\)/g)].map(match=>match[1]);
  assert.deepEqual(includes,[`${BASE}/release-fields.json`,`${BASE}/manifest.json`]);
  assert.equal(hash(readFileSync(join(root,includes[0]))),RELEASE_FIELDS_SHA256);
  assert.equal(hash(readFileSync(join(root,includes[1]))),MANIFEST_SHA256);
  const fixed=authenticate(root); assert.equal(fixed.fixtures.length+fixed.addon.length,113);
  assert.deepEqual(JSON.parse(readFileSync(join(root,includes[0]))),fixed.retainedReleaseFields);
  assert.match(source,/value\["fixture"\]\["oracle"\], ORACLE/);
  assert.match(source,/report\["oracle"\], ORACLE/);
  assert.match(source,/serde_json::from_slice\(RELEASE_FIELDS\)/);
  assert(!source.includes('formal-lua-3.2.2')); assert(!source.includes('split_once'));
  assert(ORACLE_SUPPORT.every(path=>SUPPORT.includes(path)));
  assert.equal(new Set(SUPPORT).size,SUPPORT.length);
  const controller=readFileSync(join(root,'tools/rust-formal-lua-bridge.mjs'),'utf8');
  assert.match(controller,/fixture: \{root: fixture, oracle: ORACLE, manifestSHA256: MANIFEST_SHA256, releaseFieldsSHA256: RELEASE_FIELDS_SHA256\}/);
  assert.match(controller,/materialize\(root, fixture, inputs\)/);
});
test('all nine actual Rust bridge test definitions retain the checked scenario set',()=>{
  const source=readFileSync(join(root,'crates/lycheedev-live/tests/formal_lua_bridge.rs'),'utf8');
  const names=[...source.matchAll(/^case!\((formal_lua_recovery_\w+),/gm)].map(match=>match[1]);
  assert.deepEqual(names.sort(),[...cases].sort());
});


test('bridge retry paths preserve defaults and require sealed records plus fresh contained output',()=>{
  const repository=mkdtempSync(join(tmpdir(),'formal-bridge-paths-'));
  try {
    const defaults=bridgePaths([],repository);
    assert.equal(defaults.runtimePath,join(repository,'.tmp/ci-formal-lua-runtime/build-report.json'));
    assert.equal(defaults.candidatePath,join(repository,'.tmp/ci-cli-artifact/artifact.json'));
    assert.equal(defaults.artifactDirectory,join(repository,'.tmp/ci-formal-lua-bridge-test'));
    const selected=bridgePaths(['.tmp/retry-runtime/build-report.json','.tmp/retry-cli/artifact.json','.tmp/retry-bridge'],repository);
    assert.equal(selected.artifactPath,join(repository,'.tmp/retry-bridge/artifact.json'));
    mkdirSync(selected.artifactDirectory,{recursive:true});
    assert.throws(()=>bridgePaths(['.tmp/retry-runtime/build-report.json','.tmp/retry-cli/artifact.json','.tmp/retry-bridge'],repository),/directory_exists/);
    for(const args of [['one'],['one','two'],['one','two','three','four'],['lua.exe','artifact.json','new'],['build-report.json','cli.exe','new'],['../build-report.json','artifact.json','new'],['build-report.json','../artifact.json','new'],['build-report.json','artifact.json','..'],['--runtime','artifact.json','new']])assert.throws(()=>bridgePaths(args,repository));
    const source=readFileSync(new URL('./rust-formal-lua-bridge.mjs',import.meta.url),'utf8');
    assert.match(source,/verifyInterpreter\(runtimePath\)/);assert.match(source,/verifyCandidateArtifact\(root, candidatePath\)/);
    assert.match(source,/verifyTestArtifact\(test, support, inputs, paths\)/);
  } finally {rmSync(repository,{recursive:true,force:true});}
});
