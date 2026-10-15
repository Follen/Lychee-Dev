// Controller-only parsing tests: never compile Rust, launch Lua or run a test EXE.
import assert from 'node:assert/strict';
import test from 'node:test';
import {resolve, join} from 'node:path';
import {selectTestExecutable, verifyTestOutput} from './rust-formal-lua-bridge.mjs';
const target = resolve('target');
const artifact = {reason:'compiler-artifact', target:{name:'formal_lua_bridge',kind:['test']},
  profile:{test:true}, executable:join(target,'debug/deps/formal_lua_bridge-a1b2c3.exe')};
const complete = {reason:'build-finished',success:true};
const jsonl = rows => rows.map(row=>JSON.stringify(row)).join('\n')+'\n';
const cases = ['normal','after_bind','after_prepare','after_commit','after_confirm','after_release','reload_bind','compile_error','cleanup_error'].map(name=>`formal_lua_recovery_${name}`);
const output = (names=cases, summary='9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;') => Buffer.from(
  `running ${names.length} tests\n${names.map(name=>`test ${name} ... ok`).join('\n')}\n\ntest result: ok. ${summary} finished in 0.20s\n`);

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
