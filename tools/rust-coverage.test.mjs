import test from 'node:test';
import assert from 'node:assert/strict';
import {existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, truncateSync, writeFileSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {basename, dirname, join} from 'node:path';
import {collectRustCoverage, coverageArguments, coverageEnvironment, coverageSelection, selectCoverageArtifact, summarizeCoverage, verifyCoverageTests} from './rust-coverage.mjs';
import {hashFile, runCargo} from './rust-build.mjs';

const TARGET = 'x86_64-pc-windows-msvc';
const selection = coverageSelection({packageName:'lycheedev-core', lib:true});
const jsonl = rows => Buffer.from(rows.map(row => JSON.stringify(row)).join('\n')+'\n');
const summary = {functions:{count:5,covered:3},lines:{count:10,covered:8},regions:{count:12,covered:7}};
const output = (names = ['tests::first', 'tests::second'], ignored = [], filtered = 0) => Buffer.from(
  `running ${names.length+ignored.length} tests\n${names.map(name => `test ${name} ... ok\n`).join('')}${ignored.map(name => `test ${name} ... ignored, needs fixture\n`).join('')}`
  + `\ntest result: ok. ${names.length} passed; 0 failed; ${ignored.length} ignored; 0 measured; ${filtered} filtered out; finished in 0.01s\n`);
function fixture() {
  const root = mkdtempSync(join(tmpdir(),'lycheedev-coverage-')), toolchain = join(root,'toolchain');
  for (const directory of ['release','.cargo','crates/lycheedev-core/src',`toolchain/lib/rustlib/${TARGET}/bin`,'toolchain/bin']) mkdirSync(join(root,directory),{recursive:true});
  for (const file of ['Cargo.toml','Cargo.lock','crates/lycheedev-core/Cargo.toml']) writeFileSync(join(root,file),'fixture');
  writeFileSync(join(root,'.cargo/config.toml'),'[build]\ntarget-dir = "target"\n\n[target.x86_64-pc-windows-msvc]\nrustflags = ["-C", "target-feature=+crt-static"]\n');
  const source = join(root,'crates/lycheedev-core/src/lib.rs'); writeFileSync(source,'fn fixture() {}\n');
  for (const path of ['bin/rustc.exe','bin/cargo.exe',`lib/rustlib/${TARGET}/bin/llvm-profdata.exe`,`lib/rustlib/${TARGET}/bin/llvm-cov.exe`]) writeFileSync(join(toolchain,path),`tool:${path}`);
  return {root, toolchain, source, cleanup:() => rmSync(root,{recursive:true,force:true})};
}
function exportBytes(f, files = [{filename:f.source,summary}]) {
  return Buffer.from(JSON.stringify({type:'llvm.coverage.json.export',version:'2.0.1',data:[{files}]}));
}
function mock(f, hooks = {}) {
  const calls = [], options = {packageName:'lycheedev-core',lib:true,toolchain:f.toolchain};
  let artifact;
  function execute(command,args,owner) {
    const name = basename(command);
    const step = name === 'rustc.exe' ? (args.includes('sysroot') ? 'sysroot' : 'rustc')
      : name === 'cargo.exe' || name === 'cargo' ? (args.includes('--version') ? (command === 'cargo' ? 'cargo-launch-version' : 'cargo-version') : 'cargo')
      : name === 'llvm-profdata.exe' ? (args.includes('--version') ? 'profdata-version' : 'merge')
      : name === 'llvm-cov.exe' ? (args.includes('--version') ? 'cov-version' : 'export') : 'tests';
    calls.push({step,command,args,owner});
    assert(owner.timeout > 0 && owner.timeout <= 600000);
    assert.equal(owner.env.RUSTUP_TOOLCHAIN,f.toolchain);
    assert.equal(owner.env.RUSTUP_AUTO_INSTALL,'0');
    assert.equal(existsSync(join(f.root,'.tmp/rust-disk/channels/ordinary.lease')),true,`lease held at ${step}`);
    assert.equal(owner.cargoBuild,step === 'cargo');
    let stdout = Buffer.alloc(0);
    if (step === 'rustc') stdout = Buffer.from(`rustc 1.99.0 (aaaaaaa 2026-09-01)\ncommit-hash: ${'a'.repeat(40)}\nrelease: 1.99.0\nhost: ${TARGET}\nLLVM version: 22.1.0\n`.replaceAll('\n','\r\n'));
    if (step === 'sysroot') stdout = Buffer.from(f.toolchain+'\r\n');
    if (step === 'cargo-version' || step === 'cargo-launch-version') stdout = Buffer.from('cargo 1.99.0 (bbbbbbb 2026-09-01)\r\n');
    if (step === 'profdata-version' || step === 'cov-version') stdout = Buffer.from('LLVM (http://llvm.org/):\r\n  LLVM version 22.1.0-rust-1.99.0-stable\r\n  Optimized build.\r\n');
    if (step === 'cargo') {
      assert.deepEqual(args,['test','--locked','--offline','-j','1','--target',TARGET,'-p','lycheedev-core',
        ...(options.lib ? ['--lib'] : ['--test',options.testName]),'--no-run','--message-format=json']);
      assert.equal(owner.env.RUSTC,join(f.toolchain,'bin/rustc.exe'));
      assert.equal(owner.env.CARGO_ENCODED_RUSTFLAGS,'-C\x1ftarget-feature=+crt-static\x1f-C\x1finstrument-coverage');
      assert.equal(owner.env.CARGO_INCREMENTAL,'0'); assert.equal(owner.env.RUSTC_WRAPPER,'');
      assert.equal(owner.env.CARGO_BUILD_BUILD_DIR,owner.env.CARGO_TARGET_DIR); assert.equal(owner.cwd,f.root);
      const executable = join(owner.env.CARGO_TARGET_DIR,TARGET,'debug/deps/fixture-aabbcc.exe'); mkdirSync(dirname(executable),{recursive:true}); writeFileSync(executable,'instrumented fixture');
      artifact = {reason:'compiler-artifact',manifest_path:join(f.root,'crates/lycheedev-core/Cargo.toml'),
        package_id:'path+file:///fixture#lycheedev-core@3.5.0',target:{kind:[options.lib ? 'lib' : 'test'],name:options.testName || 'lycheedev_core',src_path:f.source},
        profile:{test:true},filenames:[executable],executable};
      stdout = jsonl([artifact,{reason:'build-finished',success:true}]);
    }
    if (step === 'tests') {
      assert.equal(owner.cwd,join(f.root,'crates/lycheedev-core'));
      assert.equal(command,artifact.executable); assert.deepEqual(args.slice(0,3),['--test-threads=1','--format=pretty','--color=never']);
      const path = owner.env.LLVM_PROFILE_FILE.replace('%m','123_0').replace('%p','99'); writeFileSync(path,'raw counts');
      stdout = options.exact ? output([options.exact],[],5) : output(['tests::first','tests::second'],['tests::needs_fixture']);
    }
    if (step === 'merge') {
      assert.deepEqual(args.slice(0,5),['merge','-sparse','--failure-mode=any','-j','1']);
      const raw = args.slice(5,-2); assert.equal(raw.length,1); assert.equal(readFileSync(raw[0],'utf8'),'raw counts');
      writeFileSync(args.at(-1),'merged counts');
    }
    if (step === 'export') {
      assert(args.includes('--summary-only')); assert(args.includes('--format=text')); assert(args.includes('--object'));
      assert.equal(readFileSync(args.at(-1),'utf8'),'instrumented fixture'); stdout = exportBytes(f);
    }
    const result = {status:0,handlesReaped:true,stdout,stderr:Buffer.alloc(0)};
    hooks[step]?.({result,owner,command,args,artifact,f,calls});
    return result;
  }
  return {execute,calls,options};
}
function failed(f, runner, expected, extra = {}) {
  let error;
  try { collectRustCoverage(f.root,runner.options,{execute:runner.execute,...extra}); } catch (failure) { error = failure; }
  assert(error); assert.match(error.message,expected);
  return {error, report:JSON.parse(readFileSync(error.coverageReport)), receipt:JSON.parse(readFileSync(error.coverageEvidence))};
}
// Model only the existing owner's exact launcher-hash decision and native-zero
// orphan failure. This does not execute PowerShell or verify a real signature.
function cargoAdmissionModel(f) {
  const launcher=join(f.root,'reviewed-launcher/cargo.exe'); mkdirSync(dirname(launcher)); writeFileSync(launcher,'reviewed rustup proxy');
  const reviewedSHA256=hashFile(launcher), admissions=[];
  function admit({command,result}) {
    const path=command==='cargo' ? launcher : command, enabled=hashFile(path)===reviewedSHA256;
    const proof={helperAdmission:enabled ? {state:'enabled',cargo:{path,sha256:hashFile(path)}}
      : {state:'disabled',reason:'owned.unreviewed_cargo_identity'},forced:true,knownAuxiliaryCleanup:enabled};
    admissions.push(proof); result.proof=proof;
    if(!enabled) result.error=new Error('owned.forced_tree_termination');
  }
  return {launcher,reviewedSHA256,admissions,admit};
}

test('selection and CLI accept exactly one owned package target with an optional exact filter',()=>{
  assert.deepEqual(coverageArguments(['--package','lycheedev-core','--lib','--toolchain','C:\\Rust']),{packageName:'lycheedev-core',lib:true,toolchain:'C:\\Rust'});
  assert.deepEqual(coverageSelection({packageName:'lycheedev-data',testName:'csv_stream',exact:'case::one'}),{packageName:'lycheedev-data',kind:'test',testName:'csv_stream',exact:'case::one'});
  for (const options of [{},{packageName:'other',lib:true},{packageName:'lycheedev-core'},
    {packageName:'lycheedev-core',lib:true,testName:'integration'},{packageName:'lycheedev-core',lib:true,exact:'--ignored'},
    {packageName:'lycheedev-core',testName:'../outside'}]) assert.throws(()=>coverageSelection(options),/coverage\./);
  for (const args of [['--workspace'],['--lib','--lib'],['--package','a','--package','b'],['--timeout','900000'],['--toolchain']]) assert.throws(()=>coverageArguments(args),/coverage\./);
});
test('ambient flag, compiler, runner, test and output overrides are refused',()=>{
  for (const key of ['RUSTFLAGS','RUSTC','CARGO_ENCODED_RUSTFLAGS','CARGO_BUILD_BUILD_DIR','CARGO_BUILD_TARGET_DIR','CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS','CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUNNER','CARGO_PROFILE_TEST_DEBUG','RUST_TEST_THREADS','LLVM_PROFILE_FILE','RUSTC_WRAPPER'])
    assert.throws(()=>coverageEnvironment({[key]:'override'}),/uncontrolled_environment/);
  assert.deepEqual(coverageEnvironment({PATH:'fixture',RUSTUP_TOOLCHAIN:'1.99.0'}),{PATH:'fixture',RUSTUP_TOOLCHAIN:'1.99.0'});
});
test('selected pipeline holds ordinary lease, retains exact raw/report/artifact bytes and retires scratch',()=>{
  const f=fixture(); try {
    const runner=mock(f), result=collectRustCoverage(f.root,runner.options,{execute:runner.execute});
    assert.deepEqual(runner.calls.map(row=>row.step),['rustc','sysroot','cargo-version','cargo-launch-version','profdata-version','cov-version','cargo','tests','merge','export']);
    const report=JSON.parse(readFileSync(result.report)), owner=JSON.parse(readFileSync(result.coverageEvidence)), build=JSON.parse(readFileSync(result.buildEvidence));
    assert.equal(report.complete,true); assert.equal(report.tests.passed,2); assert.equal(report.tests.ignored,1);
    assert.equal(report.coverage.files[0].regions.uncovered,5); assert.match(report.coverage.denominator,/No workspace percentage/);
    assert.equal(hashFile(report.artifact.preservedBinary),report.artifact.sha256);
    assert.equal(hashFile(report.profiles[0].path),report.profiles[0].sha256);
    assert.equal(owner.state,'retired'); assert.equal(build.state,'retired'); assert.equal(existsSync(owner.directory),false);
    assert.equal(existsSync(join(f.root,'.tmp/rust-disk/channels/ordinary.lease')),false);
    for (const row of owner.sealedLogs) assert.equal(hashFile(row.path),row.sha256);
    assert.equal(report.build.inputHashes.sourceTree.length,64);
  } finally {f.cleanup();}
});
test('coverage uses the reviewed Cargo launch contract with the exact selected toolchain despite an ambient override',()=>{
  const f=fixture(), prior={RUSTUP_TOOLCHAIN:process.env.RUSTUP_TOOLCHAIN,RUSTUP_AUTO_INSTALL:process.env.RUSTUP_AUTO_INSTALL};
  try {
    process.env.RUSTUP_TOOLCHAIN='misleading-other-toolchain'; process.env.RUSTUP_AUTO_INSTALL='1';
    const model=cargoAdmissionModel(f), selectedCargo=join(f.toolchain,'bin/cargo.exe');
    assert.notEqual(hashFile(selectedCargo),model.reviewedSHA256);
    const runner=mock(f,{cargo:model.admit}), result=collectRustCoverage(f.root,runner.options,{execute:runner.execute});
    const report=JSON.parse(readFileSync(result.report));
    assert.equal(report.complete,true); assert.equal(report.tools.cargo.path,selectedCargo);
    assert.equal(report.tools.cargo.sha256,hashFile(selectedCargo));
    assert.equal(report.compiler.cargoLaunchVerbose,report.compiler.cargoVerbose);
    assert.equal(runner.calls.find(row=>row.step==='cargo-version').command,selectedCargo);
    assert.equal(runner.calls.find(row=>row.step==='cargo-launch-version').command,'cargo');
    assert.equal(runner.calls.find(row=>row.step==='cargo').command,'cargo');
    assert.equal(model.admissions.length,1); assert.equal(model.admissions[0].helperAdmission.state,'enabled');
    assert.equal(model.admissions[0].helperAdmission.cargo.sha256,model.reviewedSHA256);
    for(const {owner} of runner.calls) {
      assert.equal(owner.env.RUSTUP_TOOLCHAIN,f.toolchain); assert.equal(owner.env.RUSTUP_AUTO_INSTALL,'0');
      assert.equal(owner.env.CARGO_ENCODED_RUSTFLAGS,'-C\x1ftarget-feature=+crt-static\x1f-C\x1finstrument-coverage');
    }
  } finally {
    for(const [key,value] of Object.entries(prior)) { if(value===undefined) delete process.env[key]; else process.env[key]=value; }
    f.cleanup();
  }
});
for(const changedAfterPreflight of [false,true]) test(`${changedAfterPreflight ? 'changed' : 'wrong'} Cargo launcher retains the native-zero orphan failure`,()=>{
  const f=fixture(); try {
    const model=cargoAdmissionModel(f), replace=()=>writeFileSync(model.launcher,'unreviewed replacement proxy');
    if(!changedAfterPreflight) replace();
    const runner=mock(f,{...(changedAfterPreflight ? {'cargo-launch-version':replace} : {}),cargo:model.admit});
    const {report,receipt}=failed(f,runner,/cargo_failed/);
    assert.equal(report.complete,false); assert.equal(receipt.handlesReaped,true); assert.equal(receipt.state,'retired');
    assert.equal(runner.calls.at(-1).step,'cargo'); assert.equal(report.steps.at(-1).status,0);
    assert.equal(report.steps.at(-1).error,'owned.forced_tree_termination');
    assert.equal(model.admissions[0].helperAdmission.reason,'owned.unreviewed_cargo_identity');
    assert.equal(model.admissions[0].knownAuxiliaryCleanup,false);
    assert.equal(existsSync(join(f.root,'.tmp/rust-disk/channels/ordinary.lease')),false);
  } finally {f.cleanup();}
});
test('integration selection and exact filter are explicit and successive runs never reuse profile output',()=>{
  const f=fixture(); try {
    const locations=[];
    for(let i=0;i<2;i++) {
      const runner=mock(f); Object.assign(runner.options,{lib:false,testName:'integration',exact:'case::one'});
      const result=collectRustCoverage(f.root,runner.options,{execute:runner.execute}), report=JSON.parse(readFileSync(result.report));
      locations.push(report.profilePattern); assert.equal(report.tests.passed,1); assert.equal(report.tests.filtered,5);
      assert.deepEqual(runner.calls.find(row=>row.step==='tests').args.slice(-2),['case::one','--exact']);
    }
    assert.notEqual(locations[0],locations[1]);
  } finally {f.cleanup();}
});
test('missing LLVM component fails before owner admission or any process and never installs tools',()=>{
  const f=fixture(); try {
    rmSync(join(f.toolchain,`lib/rustlib/${TARGET}/bin/llvm-cov.exe`));
    assert.throws(()=>collectRustCoverage(f.root,{packageName:'lycheedev-core',lib:true,toolchain:f.toolchain},{execute(){assert.fail('must not run');}}),/tool_missing/);
    assert.equal(existsSync(join(f.root,'.tmp')),false);
  } finally {f.cleanup();}
});
test('missing selected Cargo fails before any launch and cannot use rustup Cargo fallback',()=>{
  const f=fixture(); try {
    rmSync(join(f.toolchain,'bin/cargo.exe'));
    assert.throws(()=>collectRustCoverage(f.root,{packageName:'lycheedev-core',lib:true,toolchain:f.toolchain},{execute(){assert.fail('must not run');}}),/tool_missing/);
    assert.equal(existsSync(join(f.root,'.tmp')),false);
  } finally {f.cleanup();}
});
test('changed Cargo target configuration is refused rather than silently losing flags',()=>{
  const f=fixture(); try {
    writeFileSync(join(f.root,'.cargo/config.toml'),'[build]\nrustflags = ["-C", "new-required-flag"]');
    const runner=mock(f); assert.throws(()=>collectRustCoverage(f.root,runner.options,{execute:runner.execute}),/project_rustflags_changed/);
    assert.equal(runner.calls.length,0);
  } finally {f.cleanup();}
});
for (const [name,step,alter,expected] of [
  ['wrong Rust release','rustc',x=>x.result.stdout=Buffer.from(x.result.stdout.toString().replace('release: 1.99.0','release: 1.98.0')),/compiler_identity/],
  ['wrong Cargo release','cargo-version',x=>x.result.stdout=Buffer.from('cargo 1.98.0 (bbbbbbb 2026-09-01)\n'),/cargo_identity/],
  ['wrong launch Cargo identity','cargo-launch-version',x=>x.result.stdout=Buffer.from('cargo 1.99.0 (ccccccc 2026-09-01)\n'),/cargo_launch_identity/],
  ['mismatched LLVM patch','cov-version',x=>x.result.stdout=Buffer.from('LLVM version 22.1.1\n'),/llvm_version_mismatch/],
  ['mismatched LLVM Rust suffix','cov-version',x=>x.result.stdout=Buffer.from('LLVM version 22.1.0-rust-1.98.0-stable\n'),/llvm_version_mismatch/],
  ['wrong sysroot','sysroot',x=>x.result.stdout=Buffer.from(x.f.root),/compiler_sysroot/],
  ['changed tool bytes','cargo-version',x=>writeFileSync(join(x.f.toolchain,'bin/rustc.exe'),'replaced'),/file_changed/],
  ['changed selected Cargo bytes','cargo-launch-version',x=>writeFileSync(join(x.f.toolchain,'bin/cargo.exe'),'replaced'),/file_changed/],
  ['Cargo failed','cargo',x=>{x.result.status=101;},/cargo_failed/],
  ['test failed','tests',x=>{x.result.status=101;},/tests_failed/],
  ['empty test selection','tests',x=>{x.result.stdout=output([],[],20);},/no_tests_executed/],
  ['missing raw profile','tests',x=>rmSync(x.owner.env.LLVM_PROFILE_FILE.replace('%m','123_0').replace('%p','99')),/profiles_missing/],
  ['merge failed','merge',x=>{x.result.status=1;},/merge_failed/],
  ['export warning','export',x=>{x.result.stderr=Buffer.from('warning: mismatched profile');},/export_diagnostics/],
  ['corrupt LLVM JSON','export',x=>{x.result.stdout=Buffer.from('{');},/JSON/],
  ['source mutated during test','tests',x=>writeFileSync(x.f.source,'changed source'),/source_changed_during_compile/],
  ['executable mutated during test','tests',x=>writeFileSync(x.artifact.executable,'changed executable'),/file_changed/],
]) test(`${name} does not become coverage success and preserves evidence`,()=>{
  const f=fixture(); try {
    const runner=mock(f,{[step]:alter}), {report,receipt}=failed(f,runner,expected);
    assert.equal(report.complete,false); assert.equal(receipt.state,'retired'); assert.equal(receipt.handlesReaped,true);
    assert.equal(existsSync(receipt.directory),false); assert.equal(existsSync(join(f.root,'.tmp/rust-disk/channels/ordinary.lease')),false);
    assert.equal(JSON.parse(readFileSync(report.buildEvidence)).state,'retired');
  } finally {f.cleanup();}
});
test('downstream test failure keeps Cargo exit zero as a separate recorded fact',()=>{
  const f=fixture(); try {
    const runner=mock(f,{tests({result}){result.status=101;}}), {report}=failed(f,runner,/tests_failed/);
    assert.equal(report.steps.find(row=>row.name==='cargo').status,0); assert.equal(report.steps.find(row=>row.name==='tests').status,101);
    const build=JSON.parse(readFileSync(report.buildEvidence));
    const command=JSON.parse(readFileSync(build.sealedLogs.find(row=>row.path.endsWith('command.json')).path));
    assert.equal(command.exit,0); assert.match(command.error,/tests_failed/);
  } finally {f.cleanup();}
});
test('unreaped test retains both owners and target lease; no later child is started',()=>{
  const f=fixture(); try {
    const runner=mock(f,{tests({result}){result.handlesReaped=false;}}), {report,receipt}=failed(f,runner,/tree_reap_unverified/);
    assert.equal(receipt.state,'interrupted'); assert.equal(receipt.handlesReaped,false); assert.equal(existsSync(receipt.directory),true);
    const build=JSON.parse(readFileSync(report.buildEvidence)); assert.equal(build.state,'interrupted'); assert.equal(existsSync(build.directory),true);
    assert.equal(runner.calls.at(-1).step,'tests'); assert.equal(report.rawArchived,false);
    assert.throws(()=>runCargo(f.root,['check'],{execute(){assert.fail('must not execute');}}),/EEXIST/);
  } finally {f.cleanup();}
});
test('all children share the original 600-second deadline rather than receiving a fresh allowance',()=>{
  const f=fixture(); try {
    let time=0; const runner=mock(f,{rustc(){time=600001;}});
    const {report}=failed(f,runner,/deadline/,{now:()=>time});
    assert.equal(runner.calls.length,1); assert.equal(report.steps[0].timeoutMs,600000);
  } finally {f.cleanup();}
});
test('an executor throw retains both ownership units instead of assuming no process was started',()=>{
  const f=fixture(); try {
    const runner=mock(f,{tests(){throw new Error('fixture executor threw after starting');}});
    const {report,receipt}=failed(f,runner,/executor threw/);
    assert.equal(receipt.state,'interrupted'); assert.equal(existsSync(receipt.directory),true);
    assert.equal(JSON.parse(readFileSync(report.buildEvidence)).handlesReaped,false);
    assert.equal(runner.calls.at(-1).step,'tests');
  } finally {f.cleanup();}
});
test('oversized profile retains its owned scratch instead of silently omitting or deleting bytes',()=>{
  const f=fixture(); try {
    const runner=mock(f,{tests({owner}){truncateSync(owner.env.LLVM_PROFILE_FILE.replace('%m','123_0').replace('%p','99'),(16<<20)+1);}});
    const {report,receipt}=failed(f,runner,/file_budget/);
    assert.equal(report.rawArchived,false); assert.equal(receipt.state,'failed'); assert.equal(existsSync(receipt.directory),true);
    assert.equal(existsSync(join(f.root,'.tmp/rust-disk/channels/ordinary.lease')),false);
  } finally {f.cleanup();}
});
test('Cargo artifact cardinality, completion, manifest, target and output boundaries are enforced',()=>{
  const f=fixture(); try {
    const target=join(f.root,'target'); mkdirSync(target); const executable=join(target,'selected.exe'); writeFileSync(executable,'binary');
    const good={reason:'compiler-artifact',manifest_path:join(f.root,'crates/lycheedev-core/Cargo.toml'),target:{kind:['lib'],src_path:f.source},profile:{test:true},executable,filenames:[executable]};
    const done={reason:'build-finished',success:true};
    assert.equal(selectCoverageArtifact(jsonl([good,done]),f.root,target,selection).path,executable);
    for(const rows of [[good,good,done],[good],[good,done,done],[good,{...done,success:false}],
      [{...good,manifest_path:join(f.root,'other/Cargo.toml')},done],[{...good,profile:{test:false}},done],
      [{...good,executable:f.source,filenames:[f.source]},done],[{...good,filenames:[]},done]])
      assert.throws(()=>selectCoverageArtifact(jsonl(rows),f.root,target,selection));
  } finally {f.cleanup();}
});
test('selected executable file links are rejected when file-link creation is available',t=>{
  const f=fixture(); try {
    const target=join(f.root,'target'); mkdirSync(target); const executable=join(target,'selected.exe'); writeFileSync(executable,'binary');
    const link=join(target,'linked.exe');
    try { symlinkSync(executable,link,'file'); }
    catch(error) { if(process.platform==='win32' && ['EPERM','EACCES','ENOTSUP'].includes(error.code)) return t.skip(`file-link privilege unavailable: ${error.code}`); throw error; }
    const row={reason:'compiler-artifact',manifest_path:join(f.root,'crates/lycheedev-core/Cargo.toml'),target:{kind:['lib'],src_path:f.source},profile:{test:true},executable:link,filenames:[link]};
    assert.throws(()=>selectCoverageArtifact(jsonl([row,{reason:'build-finished',success:true}]),f.root,target,selection),/reparse/);
  } finally {f.cleanup();}
});
test('test summaries require named rows, unique completion and a nonempty exact selection',()=>{
  assert.equal(verifyCoverageTests(output(['one'],['ignored']),selection).ignored,1);
  for(const bytes of [output([]),Buffer.concat([output(),output()]),output(['same','same']),Buffer.from('test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0s\n')])
    assert.throws(()=>verifyCoverageTests(bytes,selection),/coverage\./);
  assert.throws(()=>verifyCoverageTests(output(['wrong']),{...selection,exact:'wanted'}),/test_selection/);
  assert.throws(()=>verifyCoverageTests(output(['one'],[],1),selection),/test_selection/);
});
test('should-panic pretty annotations preserve the canonical exact test name and ignored name',()=>{
  const result=verifyCoverageTests(output(['module::panics - should panic'],[],4),{...selection,exact:'module::panics'});
  assert.deepEqual(result.names,['module::panics']);
  assert.deepEqual(verifyCoverageTests(output(['ordinary'],['module::ignored - should panic']),selection).ignoredNames,['module::ignored']);
});
test('summary scope excludes external files, rejects duplicate/impossible counts and never fabricates a workspace denominator',()=>{
  const f=fixture(); try {
    const external=join(f.root,'vendor/dependency.rs');
    const report=summarizeCoverage(exportBytes(f,[{filename:f.source,summary},{filename:external,summary}]),f.root);
    assert.equal(report.files.length,1); assert.deepEqual(report.excluded,[external]); assert.equal(Object.hasOwn(report,'percent'),false);
    for(const files of [[{filename:external,summary}],[{filename:f.source,summary},{filename:f.source,summary}],
      [{filename:f.source,summary:{...summary,lines:{count:1,covered:2}}}]]) assert.throws(()=>summarizeCoverage(exportBytes(f,files),f.root),/coverage\./);
  } finally {f.cleanup();}
});
