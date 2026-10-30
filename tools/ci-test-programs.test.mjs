// Test-program record custody: a downstream validate stage must reject records
// from another workspace/commit/source tree and never run an unsealed program.
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { after, test } from 'node:test';
import { RECORD_SCHEMA, buildTestPrograms, readTestRecord, runTestPrograms, packageName, isFormalBridge } from './ci-test-programs.mjs';
import { sourceDigest } from './rust-build.mjs';

const root = resolve(import.meta.dirname, '..');
const base = join(root, '.tmp', `ci-test-programs-test-${process.pid}`);
const recordPath = join(base, 'test-programs.json');
mkdirSync(base, { recursive: true });
after(() => rmSync(base, { recursive: true, force: true }));

function gitHead() {
  const result = spawnSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8', windowsHide: true, shell: false });
  assert.equal(result.status, 0);
  return result.stdout.trim();
}
function record(overrides = {}) {
  return {
    schema: RECORD_SCHEMA, root, commit: gitHead(), sourceTree: sourceDigest(root),
    cargoTargetDir: join(base, 'target'), programs: [], support: [], excluded: [], ...overrides,
  };
}
function write(value) { writeFileSync(recordPath, JSON.stringify(value, null, 2)); return recordPath; }

function buildFixture(name) {
  const directory=join(base,name);
  for(const child of ['crates/fixture','release','.cargo'])mkdirSync(join(directory,child),{recursive:true});
  for(const child of ['Cargo.toml','Cargo.lock','.cargo/config.toml','crates/fixture/Cargo.toml'])writeFileSync(join(directory,child),'fixture');
  return directory;
}

test('test-program builds forward default and explicit jobs to Cargo and the sealed command',()=>{
  for(const jobs of [undefined,2]){
    const directory=buildFixture(`jobs-${jobs??'default'}`),output=join(directory,'artifact');let calls=0,received;
    const built=buildTestPrograms({root:directory,output,...(jobs===undefined?{}:{jobs}),execute(command,args,options){
      calls++;assert.equal(command,'cargo');received=args;
      const target=join(options.env.CARGO_TARGET_DIR,'debug/deps');mkdirSync(target,{recursive:true});
      const executable=join(target,'fixture.exe');writeFileSync(executable,'mock libtest program');
      const messages=[{reason:'compiler-artifact',executable,package_id:'path+file:///fixture#fixture@0.1.0',target:{kind:['lib'],name:'fixture'},profile:{test:true}},{reason:'build-finished',success:true}];
      return {status:0,handlesReaped:true,stdout:Buffer.from(messages.map(value=>JSON.stringify(value)).join('\n')),stderr:Buffer.alloc(0)};
    }});
    const expected=['test','--locked','--offline','--workspace','--all-targets','--no-run','--message-format=json-render-diagnostics','-j',String(jobs??1)];
    assert.equal(calls,1);assert.deepEqual(received,expected);assert.deepEqual(built.command,['cargo',...expected]);
    assert.deepEqual(JSON.parse(readFileSync(join(output,'test-programs.json'))).command,built.command);
    assert.equal(built.programs.length,1);
  }
});

test('invalid build jobs refuse admission before creating output or launching Cargo',()=>{
  const directory=buildFixture('invalid-jobs'),output=join(directory,'artifact');let calls=0;
  for(const jobs of [0,5,2.5,'bad',null,true])assert.throws(()=>buildTestPrograms({root:directory,output,jobs,execute(){calls++;throw new Error('compiler must not start');}}),/build.invalid_jobs/);
  assert.equal(calls,0);assert.equal(existsSync(output),false);assert.equal(existsSync(join(directory,'.tmp')),false);
});

test('test-program artifact preserves producer receipt, fingerprint logs and explicit Cargo timings',()=>{
  const directory=buildFixture('timings-evidence'),output=join(directory,'artifact');let originalEvidence;
  const html=Buffer.from('<html>test compilation timings\r\n</html>'),stderr=Buffer.from('fingerprint: crate is fresh\r\n');
  const record=buildTestPrograms({root:directory,output,timings:true,execute(command,args,options){
    assert.equal(command,'cargo');assert.equal(args.at(-1),'--timings');originalEvidence=options.evidence;
    const target=options.env.CARGO_TARGET_DIR;mkdirSync(join(target,'debug/deps'),{recursive:true});mkdirSync(join(target,'cargo-timings'));
    writeFileSync(join(target,'cargo-timings/cargo-timing.html'),html);writeFileSync(join(target,'cargo-timings/old.html'),'not this build');
    const executable=join(target,'debug/deps/fixture.exe');writeFileSync(executable,'mock libtest executable');
    const messages=[{reason:'compiler-artifact',executable,package_id:'path+file:///fixture#fixture@0.1.0',target:{kind:['lib'],name:'fixture'},profile:{test:true}},{reason:'build-finished',success:true}];
    return {status:0,handlesReaped:true,stdout:Buffer.from(messages.map(row=>JSON.stringify(row)).join('\n')),stderr};
  }});
  assert.equal(record.buildEvidence,join(output,'evidence/receipt.json'));
  const original=readFileSync(join(originalEvidence,'receipt.json'));assert.deepEqual(readFileSync(record.buildEvidence),original);
  assert.equal(record.buildEvidenceSHA256,createHash('sha256').update(original).digest('hex'));
  assert.deepEqual(readFileSync(join(output,'evidence/stderr.bin')),stderr);assert.deepEqual(readFileSync(record.cargoTimings.path),html);
  const receipt=JSON.parse(original);assert.equal(receipt.state,'retired');assert.equal(existsSync(receipt.directory),false);
  assert.equal(existsSync(join(output,'evidence/old.html')),false);assert.equal(record.command.at(-1),'--timings');
  rmSync(originalEvidence,{recursive:true});
  for(const log of receipt.sealedLogs)assert.equal(createHash('sha256').update(readFileSync(join(output,'evidence',log.path.split(/[\\/]/).at(-1)))).digest('hex'),log.sha256);
  assert.deepEqual(JSON.parse(readFileSync(join(output,'test-programs.json'))),record);
});

test('build CLI rejects invalid, missing and repeated jobs without enabling jobs on run',()=>{
  const script=join(root,'tools/ci-test-programs.mjs'),output=join(base,'cli-invalid-output');
  for(const [args,expected] of [
    [['build',output,'--jobs','0'],/build.invalid_jobs/],
    [['build',output,'--jobs','2.5'],/build.invalid_jobs/],
    [['build',output,'--jobs'],/ci-test-programs.option_value_missing/],
    [['build',output,'--jobs','--owner','fixture'],/ci-test-programs.option_value_missing/],
    [['build',output,'--jobs','2','--jobs','3'],/ci-test-programs.duplicate_option/],
    [['run',recordPath,'--jobs','2'],/ci-test-programs.unknown_option/],
    [['run',recordPath,'--timings'],/ci-test-programs.unknown_option/],
    [['build',output,'--timings','--timings'],/ci-test-programs.duplicate_option/],
  ]){
    const result=spawnSync(process.execPath,[script,...args],{cwd:root,encoding:'utf8',windowsHide:true,shell:false});
    assert.equal(result.status,1);assert.match(result.stderr,expected);
  }
  assert.equal(existsSync(output),false);
});

test('a record from this workspace, commit and source tree is accepted', () => {
  const loaded = readTestRecord(root, write(record()));
  assert.equal(loaded.schema, RECORD_SCHEMA);
});

test('unknown record schema is rejected', () => {
  assert.throws(() => readTestRecord(root, write(record({ schema: 'lycheedev.ci-test-programs.v0' }))), /unknown_record/);
});

test('a record from a different workspace, commit or source tree is rejected', () => {
  assert.throws(() => readTestRecord(root, write(record({ root: join(root, '.tmp') }))), /workspace_mismatch/);
  assert.throws(() => readTestRecord(root, write(record({ commit: '0'.repeat(40) }))), /commit_mismatch/);
  assert.throws(() => readTestRecord(root, write(record({ sourceTree: 'f'.repeat(64) }))), /source_tree_mismatch/);
});

test('validate entry refuses an empty program selection instead of silently passing', () => {
  write(record());
  assert.throws(() => runTestPrograms({ recordPath, packages: ['lycheedev-core'] }), /no_selected_programs/);
});

test('Cargo package identity handles actual version-only path fragments and explicit names',()=>{
 assert.equal(packageName({package_id:'path+file:///D:/Code/wow/addons/Lychee%20Dev/crates/lycheedev-windows#3.5.0'}),'lycheedev-windows');
 assert.equal(packageName({package_id:'path+file:///D:/workspace/crates/lycheedev-live#3.5.0'}),'lycheedev-live');
 assert.equal(packageName({package_id:'path+file:///D:/workspace/other-directory#lycheedev-core@3.5.0'}),'lycheedev-core');
 assert.equal(packageName({package_id:'registry+https://github.com/rust-lang/crates.io-index#serde@1.0.228'}),'serde');
});
test('formal target requires explicit runtime before launching any test program',()=>{
 const program={package:'3.5.0',target:'formal_lua_bridge',kinds:['test'],path:'unused.exe',sha256:'a'.repeat(64),bytes:1};
 assert(isFormalBridge(program));assert(!isFormalBridge({...program,target:'other'}));
 assert.throws(()=>runTestPrograms({recordPath:write(record({programs:[program]}))}),/formal_lua_report_required/);
});

function formalProgram(){
 const executable=join(base,'never-executed-formal.exe'),bytes=Buffer.from('synthetic test program bytes');writeFileSync(executable,bytes);
 return {package:'lycheedev-live',target:'formal_lua_bridge',kinds:['test'],path:executable,sha256:createHash('sha256').update(bytes).digest('hex'),bytes:bytes.length};
}
test('reaped formal failure writes a failed report without claiming unattempted passes',()=>{
 const reportPath=join(base,'formal-failed-report.json'),program=formalProgram();
 assert.throws(()=>runTestPrograms({recordPath:write(record({programs:[program]})),reportPath,formalLuaReport:'unused-build-report.json',formalBridge(){throw Object.assign(new Error('formal fixture failed'),{handlesReaped:true,bridgeEvidence:'original-owner-receipt'});}}),/tests_failed/);
 const report=JSON.parse(readFileSync(reportPath));assert.equal(report.state,'failed');assert.equal(report.attempted,1);assert.equal(report.passed,0);assert.equal(report.failed,1);assert.equal(report.results[0].handlesReaped,true);assert.equal(report.results[0].bridgeEvidence,'original-owner-receipt');
});
test('reaped formal failure continues to the next existing program boundary without native execution',()=>{
 const program=formalProgram(),next={...program,target:'next_program',path:join(base,'deliberately-missing-next.exe')};
 assert.throws(()=>runTestPrograms({recordPath:write(record({programs:[program,next]})),formalLuaReport:'unused-build-report.json',formalBridge(){throw Object.assign(new Error('formal fixture failed'),{handlesReaped:true});}}),/program_missing/);
});
test('unreaped formal failure stops before the next program and preserves its failed report',()=>{
 const program=formalProgram(),next={...program,target:'next_program',path:join(base,'deliberately-missing-next.exe')},reportPath=join(base,'formal-unreaped-report.json'),failure=Object.assign(new Error('formal tree unverified'),{handlesReaped:false});
 assert.throws(()=>runTestPrograms({recordPath:write(record({programs:[program,next]})),reportPath,formalLuaReport:'unused-build-report.json',formalBridge(){throw failure;}}),error=>error===failure);
 const report=JSON.parse(readFileSync(reportPath));assert.equal(report.state,'failed');assert.equal(report.programs,2);assert.equal(report.attempted,1);assert.equal(report.passed,0);assert.equal(report.failed,1);assert.equal(report.results[0].handlesReaped,false);
});

test('unreaped ordinary failure stops further launches and preserves the original error and evidence',()=>{
 const program={...formalProgram(),target:'ordinary_program'},next={...program,target:'next_program'},reportPath=join(base,'ordinary-unreaped-report.json');
 const failure=new Error('owned.tree_reap_unverified'),proof={rootReaped:true,treeReaped:false,handlesClosed:false,activeProcesses:1},logs=['original-owner-receipt.json'];
 let launches=0;
 assert.throws(()=>runTestPrograms({recordPath:write(record({programs:[program,next]})),reportPath,executor(){launches++;return {status:null,signal:'SIGTERM',error:failure,handlesReaped:false,proof,logs};}}),error=>error===failure);
 assert.equal(launches,1);
 const report=JSON.parse(readFileSync(reportPath));assert.equal(report.state,'failed');assert.equal(report.programs,2);assert.equal(report.attempted,1);assert.equal(report.failed,1);assert.equal(report.passed,0);
 const result=report.results[0];assert.equal(result.handlesReaped,false);assert.equal(result.exit,null);assert.equal(result.signal,'SIGTERM');assert.equal(result.error,failure.message);assert.deepEqual(result.proof,proof);assert.deepEqual(result.logs,logs);
});

test('reaped ordinary business failure still continues and reports the actual later result',()=>{
 const program={...formalProgram(),target:'ordinary_program'},next={...program,target:'next_program'},reportPath=join(base,'ordinary-reaped-report.json');
 let launches=0;
 assert.throws(()=>runTestPrograms({recordPath:write(record({programs:[program,next]})),reportPath,executor(){return {status:launches++===0?101:0,handlesReaped:true};}}),/tests_failed: 1 program/);
 assert.equal(launches,2);
 const report=JSON.parse(readFileSync(reportPath));assert.equal(report.state,'failed');assert.equal(report.attempted,2);assert.equal(report.failed,1);assert.equal(report.passed,1);assert.deepEqual(report.results.map(result=>result.handlesReaped),[true,true]);
});
