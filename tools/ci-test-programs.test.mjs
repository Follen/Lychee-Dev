// Test-program record custody: a downstream validate stage must reject records
// from another workspace/commit/source tree and never run an unsealed program.
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { after, test } from 'node:test';
import { RECORD_SCHEMA, TEST_SHARDS, aggregateTestReports, buildTestPrograms, readTestRecord, runTestPrograms, packageName, isFormalBridge } from './ci-test-programs.mjs';
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
      assert(options.env.HOME.startsWith(join(directory,'.tmp','rust-scratch') + (process.platform==='win32'?'\\':'/')));
      assert.equal(options.env.USERPROFILE,options.env.HOME);
      assert.equal(options.env.CODEX_HOME,join(options.env.HOME,'.codex'));
      assert.equal(options.env.LYCHEEDEV_HOME,join(options.env.HOME,'..','home'));
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

test('harnessless fixture programs are sealed as support without entering the test plan',()=>{
  const directory=buildFixture('helper-support'), output=join(directory,'artifact');
  writeFileSync(join(directory,'crates/fixture/Cargo.toml'),'[[test]]\nname = "guarded_spawn_fixture"\npath = "tests/guarded_spawn_fixture.rs"\nharness = false\n');
  const built=buildTestPrograms({root:directory,output,execute(_command,_args,options){
    const target=join(options.env.CARGO_TARGET_DIR,'debug/deps');mkdirSync(target,{recursive:true});
    const messages=[['fixture','lib','test harness'],['guarded_spawn_fixture','test','owned child helper']].map(([name,kind,body])=>{
      const executable=join(target,`${name}.exe`);writeFileSync(executable,body);
      return {reason:'compiler-artifact',executable,package_id:'path+file:///fixture#fixture@0.1.0',target:{kind:[kind],name},profile:{test:true}};
    });
    return {status:0,handlesReaped:true,stdout:Buffer.from([...messages,{reason:'build-finished',success:true}].map(value=>JSON.stringify(value)).join('\n')),stderr:Buffer.alloc(0)};
  }});
  assert.deepEqual(built.programs.map(row=>row.target),['fixture']);
  assert.deepEqual(built.support.map(row=>row.target),['guarded_spawn_fixture']);
  assert.deepEqual(built.excluded.map(row=>row.target),['guarded_spawn_fixture']);
  const helper=built.support[0],bytes=readFileSync(join(directory,helper.path));
  assert.equal(bytes.toString(),'owned child helper');
  assert.equal(helper.sha256,createHash('sha256').update(bytes).digest('hex'));
  assert.deepEqual(readFileSync(helper.destination),bytes);
});

test('explicit test filters reject zero execution even when libtest exits successfully',()=>{
  const executable=join(base,'filtered.exe'),bytes=Buffer.from('sealed filter fixture');writeFileSync(executable,bytes);
  const program={package:'lycheedev-windows',target:'fixture',kinds:['lib'],path:executable,sha256:createHash('sha256').update(bytes).digest('hex'),bytes:bytes.length};
  const reportPath=join(base,'filtered-report.json');
  for(const stdout of [
    '',
    'running 0 tests\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out;\n',
    'test result: ok. 1 passed; 0 failed; 0 ignored;\ntest result: ok. 0 passed; 0 failed; 1 ignored;\n',
  ]){
    write(record({programs:[program]}));
    assert.throws(()=>runTestPrograms({recordPath,reportPath,arguments:['missing_test','--exact','--ignored'],executor(){return {status:0,handlesReaped:true,stdout:Buffer.from(stdout)};}}),/tests_failed/);
    const report=JSON.parse(readFileSync(reportPath));
    assert.equal(report.state,'failed');assert.equal(report.results[0].exit,0);assert.equal(report.results[0].error,'ci-test-programs.filtered_no_tests_executed');
  }
  write(record({programs:[program]}));
  const success=runTestPrograms({recordPath,arguments:['existing_test','--exact','--ignored'],executor(){return {status:0,handlesReaped:true,stdout:Buffer.from('test result: ok. 1 passed; 0 failed; 0 ignored;\n')};}});
  assert.equal(success.state,'passed');assert.equal(success.results[0].executedTests,1);
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

let shardFixtureId = 0;
function shardFixture() {
  const directory = buildFixture(`shards-${shardFixtureId++}`), recordPath = join(directory, 'test-programs.json');
  const packages = Object.values(TEST_SHARDS).flat();
  const programs = packages.map((package_, index) => {
    const path = join(directory, `program-${index}.exe`), bytes = Buffer.from(`sealed program ${package_}`); writeFileSync(path, bytes);
    return {package: package_, target: `target-${index}`, kinds: ['lib'], path, sha256: createHash('sha256').update(bytes).digest('hex'), bytes: bytes.length};
  });
  const record = {schema: RECORD_SCHEMA, root: directory, commit: gitHead(), sourceTree: sourceDigest(directory), cargoTargetDir: join(directory, 'target'), programs, support: []};
  writeFileSync(recordPath, JSON.stringify(record));
  const reports = Object.keys(TEST_SHARDS).map(shard => {
    const path = join(directory, `${shard}.json`);
    const report = runTestPrograms({root: directory, recordPath, shard, reportPath: path, executor() {return {status: 0, handlesReaped: true};}});
    return {shard, path, report};
  });
  const output = join(directory, 'aggregate.json');
  return {directory, recordPath, record, reports, output, aggregate: (paths = reports.map(report => report.path)) => aggregateTestReports({root: directory, recordPath, reportPaths: paths, reportPath: output})};
}
function updateShard(fixture, index, change) {
  const value = JSON.parse(readFileSync(fixture.reports[index].path)); change(value);
  writeFileSync(fixture.reports[index].path, JSON.stringify(value));
}

test('named shard execution records its exact plan and record identity; pure aggregate proves full coverage', () => {
  const f = shardFixture(), digest = createHash('sha256').update(readFileSync(f.recordPath)).digest('hex');
  for (const group of f.reports) {
    assert.equal(group.report.commit, f.record.commit); assert.equal(group.report.testRecordSHA256, digest);
    assert.equal(group.report.plannedPrograms.length, TEST_SHARDS[group.shard].length);
    assert(group.report.plannedPrograms.every(program => TEST_SHARDS[group.shard].includes(program.package)));
    assert.equal(group.report.handlesReaped, true);
  }
  const summary = f.aggregate();
  assert.equal(summary.schema, 'lycheedev.ci-test-run.v1'); assert.equal(summary.state, 'passed'); assert.equal(summary.handlesReaped, true);
  assert.equal(summary.commit, f.record.commit); assert.equal(summary.sourceTree, f.record.sourceTree); assert.equal(summary.testRecordSHA256, digest);
  assert.deepEqual(summary.coverage, {complete: true, expectedPrograms: f.record.programs.length, observedPrograms: f.record.programs.length, shards: 2});
  assert.equal(summary.programs, f.record.programs.length); assert.equal(summary.attempted, f.record.programs.length); assert.equal(summary.passed, f.record.programs.length); assert.equal(summary.failed, 0);
  assert.deepEqual(new Set(summary.results.map(row => row.executable)), new Set(f.record.programs.map(row => row.path)));
  for (const group of summary.shardReports) {
    assert.equal(group.sha256, createHash('sha256').update(readFileSync(join(f.directory, group.path))).digest('hex'));
  }
  assert.deepEqual(JSON.parse(readFileSync(f.output)), summary);
});

test('aggregate rejects missing and duplicated groups and writes failed incomplete coverage', () => {
  for (const select of [reports => [reports[0].path], reports => [reports[0].path, reports[0].path]]) {
    const f = shardFixture(); assert.throws(() => f.aggregate(select(f.reports)), /aggregate_shard_count|aggregate_duplicate_shard/);
    const summary = JSON.parse(readFileSync(f.output)); assert.equal(summary.state, 'failed'); assert.equal(summary.coverage.complete, false); assert.equal(summary.handlesReaped, false);
  }
});

test('aggregate rejects a different record, commit, source tree or shard identity', () => {
  for (const [field, value, expected] of [
    ['testRecordSHA256', 'f'.repeat(64), /aggregate_record_identity/], ['commit', '0'.repeat(40), /aggregate_record_identity/],
    ['sourceTree', 'f'.repeat(64), /aggregate_record_identity/], ['shard', 'all', /aggregate_unknown_shard/], ['schema', 'other', /aggregate_schema/],
  ]) {
    const f = shardFixture(); updateShard(f, 0, report => {report[field] = value;});
    assert.throws(() => f.aggregate(), expected); assert.equal(JSON.parse(readFileSync(f.output)).coverage.complete, false);
  }
});

test('aggregate checks plan and actual identities instead of trusting successful counts', () => {
  for (const [change, expected] of [
    [report => report.plannedPrograms.pop(), /aggregate_plan_count/],
    [report => {report.plannedPrograms[1] = report.plannedPrograms[0];}, /aggregate_plan_program/],
    [report => {report.plannedPrograms[0].package = 'other';}, /aggregate_plan_identity/],
    [report => report.results.pop(), /aggregate_result_count/],
    [report => {report.results[1] = report.results[0];}, /aggregate_result_program/],
    [report => {report.results[0].executable = 'another.exe';}, /aggregate_result_program/],
    [report => {report.results[0].sha256 = 'f'.repeat(64);}, /aggregate_result_identity/],
    [report => {report.results[0].package = 'other';}, /aggregate_result_identity/],
    [report => {report.results[0].target = 'other';}, /aggregate_result_identity/],
  ]) {
    const f = shardFixture(); updateShard(f, 0, change); assert.throws(() => f.aggregate(), expected);
  }
});

test('aggregate rejects unexecuted, failed or unreaped results and dishonest report totals', () => {
  for (const [change, expected] of [
    [report => {report.results[0].exit = null;}, /aggregate_result_not_passed/],
    [report => {report.results[0].exit = 1;}, /aggregate_result_not_passed/],
    [report => {report.results[0].ok = false;}, /aggregate_result_not_passed/],
    [report => {report.results[0].handlesReaped = false;}, /aggregate_result_not_passed/],
    [report => {delete report.results[0].handlesReaped;}, /aggregate_result_not_passed/],
    [report => {report.results[0].signal = 'SIGTERM';}, /aggregate_result_not_passed/],
    [report => {report.results[0].error = 'failed';}, /aggregate_result_not_passed/],
    [report => {report.results[0].millis = -1;}, /aggregate_result_millis/],
    [report => {report.state = 'failed';}, /aggregate_summary_not_passed/],
    [report => {report.handlesReaped = false;}, /aggregate_summary_not_passed/],
    [report => {report.failed = 1;}, /aggregate_summary_not_passed/],
    [report => {report.attempted = 0;}, /aggregate_summary_not_passed/],
    [report => {report.programs += 1;}, /aggregate_summary_not_passed/],
    [report => {report.passed -= 1;}, /aggregate_summary_not_passed/],
    [report => {report.arguments = ['--list'];}, /aggregate_arguments/],
    [report => {report.packages = ['lycheedev-data'];}, /aggregate_shard_filters/],
  ]) {
    const f = shardFixture(); updateShard(f, 0, change); assert.throws(() => f.aggregate(), expected);
    assert.equal(JSON.parse(readFileSync(f.output)).state, 'failed');
  }
});

test('full-record coverage rejects a newly added package omitted from both fixed shards', () => {
  const f = shardFixture(); f.record.programs.push({...f.record.programs[0], package: 'lycheedev-new', path: join(f.directory, 'new.exe')});
  writeFileSync(f.recordPath, JSON.stringify(f.record));
  const digest = createHash('sha256').update(readFileSync(f.recordPath)).digest('hex');
  for (let i = 0; i < f.reports.length; i++) updateShard(f, i, report => {report.testRecordSHA256 = digest;});
  assert.throws(() => f.aggregate(), /aggregate_coverage_incomplete/);
  assert.deepEqual(JSON.parse(readFileSync(f.output)).coverage, {complete: false, expectedPrograms: f.record.programs.length, observedPrograms: f.record.programs.length - 1, shards: 2});
});

test('named shard refuses filters and test-list arguments before launching anything', () => {
  const f = shardFixture(); let calls = 0;
  for (const options of [{shard: 'unknown'}, {shard: 'data-delivery', packages: ['lycheedev-data']},
    {shard: 'source-runtime', targets: ['one']}, {shard: 'source-runtime', arguments: ['--list']}]) {
    assert.throws(() => runTestPrograms({root: f.directory, recordPath: f.recordPath, ...options, executor() {calls++;}}), /unknown_shard|shard_filters_conflict|shard_arguments/);
  }
  assert.equal(calls, 0);
});

test('default local run keeps shard all and captures an incomplete launch failure in its report', () => {
  const f = shardFixture(), path = join(f.directory, 'default.json'); rmSync(f.record.programs[0].path);
  assert.throws(() => runTestPrograms({root: f.directory, recordPath: f.recordPath, reportPath: path, executor() {assert.fail('missing executable must never launch');}}), /program_missing/);
  const report = JSON.parse(readFileSync(path)); assert.equal(report.shard, 'all'); assert.equal(report.state, 'failed');
  assert.equal(report.plannedPrograms.length, f.record.programs.length); assert.equal(report.attempted, 0); assert.equal(report.passed, 0);
});

test('formal bridge with passed false is a business failure even when its process was reaped', () => {
  const program = formalProgram(), reportPath = join(base, 'bridge-false.json');
  assert.throws(() => runTestPrograms({recordPath: write(record({programs: [program]})), reportPath, formalLuaReport: 'unused', formalBridge() {return {passed: false};}}), /tests_failed/);
  const report = JSON.parse(readFileSync(reportPath)); assert.equal(report.state, 'failed'); assert.equal(report.failed, 1); assert.equal(report.passed, 0);
});

test('aggregate matches program sets regardless of report order and rejects a duplicated record executable', () => {
  const f = shardFixture(); updateShard(f, 0, report => {report.plannedPrograms.reverse(); report.results.reverse();});
  assert.equal(f.aggregate([...f.reports].reverse().map(row => row.path)).coverage.complete, true);
  f.record.programs.push({...f.record.programs[0]}); writeFileSync(f.recordPath, JSON.stringify(f.record));
  assert.throws(() => f.aggregate(), /aggregate_record_program_duplicate/);
  assert.equal(JSON.parse(readFileSync(f.output)).coverage.complete, false);
});

test('aggregate CLI consumes existing reports without running programs and fails closed on invalid flags', () => {
  const f = shardFixture(), script = join(root, 'tools/ci-test-programs.mjs');
  // CLI uses the repository root. Bind the same fixture program identities to
  // that root without running them again or modifying their bytes.
  f.record.root = root; f.record.sourceTree = sourceDigest(root); writeFileSync(f.recordPath, JSON.stringify(f.record));
  const digest = createHash('sha256').update(readFileSync(f.recordPath)).digest('hex');
  for (let i = 0; i < f.reports.length; i++) updateShard(f, i, report => {report.sourceTree = f.record.sourceTree; report.testRecordSHA256 = digest;});
  const args = ['aggregate', f.recordPath, ...f.reports.flatMap(row => ['--shard-report', row.path]), '--report', f.output];
  const result = spawnSync(process.execPath, [script, ...args], {cwd: root, encoding: 'utf8', windowsHide: true, shell: false});
  assert.equal(result.status, 0, result.stderr); assert.equal(JSON.parse(result.stdout).coverage.complete, true);
  for (const [invalid, expected] of [
    [['aggregate', f.recordPath], /usage:/], [['aggregate', f.recordPath, '--shard', 'data-delivery'], /unknown_option/],
    [['aggregate', f.recordPath, '--shard-report'], /option_value_missing/], [['aggregate', f.recordPath, '--report'], /option_value_missing/],
    [['run', f.recordPath, '--shard'], /option_value_missing/], [['run', f.recordPath, '--shard', 'all', '--shard', 'all'], /duplicate_option/],
    [['run', f.recordPath, '--shard', 'data-delivery', '--package'], /option_value_missing/],
  ]) {
    const failed = spawnSync(process.execPath, [script, ...invalid], {cwd: root, encoding: 'utf8', windowsHide: true, shell: false});
    assert.equal(failed.status, 1); assert.match(failed.stderr, expected);
  }
});
