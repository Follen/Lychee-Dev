import test from 'node:test';
import assert from 'node:assert/strict';
import {existsSync, linkSync, lstatSync, mkdirSync, mkdtempSync, readFileSync, readSync, readdirSync, rmSync, symlinkSync, truncateSync, writeFileSync, writeSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {basename, dirname, join} from 'node:path';
import {archiveCoverageFile, collectRustCoverage, coverageArguments, coverageEnvironment, coverageSelection, selectCoverageArtifact, selectCoverageArtifacts, summarizeCoverage, verifyCoverageArchive, verifyCoverageTests} from './rust-coverage.mjs';
import {hashFile, runCargo} from './rust-build.mjs';
import {usage} from './rust-disk.mjs';

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
// Synthetic summary-only output, constructed from LLVM 23.1.1's
// renderRoot/renderFile/renderSummary. No user-machine export or raw profile
// is copied here. Counts and filenames are deliberately test-only values.
// https://github.com/llvm/llvm-project/blob/llvmorg-23.1.1/llvm/tools/llvm-cov/CoverageExporterJson.cpp
function llvm23Export(f, externalCount = 0) {
  const fullSummary = {
    lines:{count:10,covered:8,percent:80}, functions:{count:5,covered:3,percent:60},
    instantiations:{count:5,covered:3,percent:60}, regions:{count:12,covered:7,notcovered:5,percent:100*7/12},
    branches:{count:0,covered:0,notcovered:0,percent:0}, mcdc:{count:0,covered:0,notcovered:0,percent:0},
  };
  const files = [f.source,...Array.from({length:externalCount},(_,i)=>join(f.root,`vendor/dependency-${i}.rs`))]
    .sort().map(filename=>({filename,summary:structuredClone(fullSummary)}));
  const totals = structuredClone(fullSummary);
  for(const metric of Object.values(totals)) for(const key of ['count','covered','notcovered'])
    if(Object.hasOwn(metric,key)) metric[key] *= files.length;
  return {data:[{files,totals}],type:'llvm.coverage.json.export',version:'3.1.0'};
}
function mock(f, hooks = {}) {
  const calls = [], options = {packageName:'lycheedev-core',lib:true,toolchain:f.toolchain};
  let artifact, companion;
  function execute(command,args,owner) {
    const name = basename(command);
    const step = name === 'rustc.exe' ? (args.includes('sysroot') ? 'sysroot' : 'rustc')
      : name === 'cargo.exe' || name === 'cargo' ? (args.includes('--version') ? (command === 'cargo' ? 'cargo-launch-version' : 'cargo-version') : 'cargo')
      : name === 'llvm-profdata.exe' ? (args.includes('--version') ? 'profdata-version' : 'merge')
      : name === 'llvm-cov.exe' ? (args.includes('--version') ? 'cov-version'
        : args.filter(arg=>arg==='--object').length===2 ? 'export-combined'
          : companion && args.at(-1)===companion.executable ? 'export-companion' : 'export') : 'tests';
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
      if(options.companionBin) {
        const executable=join(owner.env.CARGO_TARGET_DIR,TARGET,`debug/${options.companionBin}.exe`); writeFileSync(executable,'instrumented companion');
        companion={reason:'compiler-artifact',manifest_path:artifact.manifest_path,package_id:artifact.package_id,
          target:{kind:['bin'],crate_types:['bin'],name:options.companionBin,src_path:f.mainSource},
          profile:{test:false,opt_level:'0',debuginfo:1,debug_assertions:true,overflow_checks:true},features:['fixture'],fresh:true,
          filenames:[executable],executable};
      }
      stdout = jsonl([artifact,...(companion ? [companion] : []),{reason:'build-finished',success:true}]);
    }
    if (step === 'tests') {
      assert.equal(owner.cwd,join(f.root,'crates/lycheedev-core'));
      assert.equal(command,artifact.executable); assert.deepEqual(args.slice(0,3),['--test-threads=1','--format=pretty','--color=never']);
      const path = owner.env.LLVM_PROFILE_FILE.replace('%m','123_0').replace('%p','99'); writeFileSync(path,'raw counts');
      if(companion) for(let i=0;i<4;i++) writeFileSync(owner.env.LLVM_PROFILE_FILE.replace('%m','456_0').replace('%p',String(100+i)),'companion counts');
      stdout = options.exact ? output([options.exact],[],5) : output(['tests::first','tests::second'],['tests::needs_fixture']);
    }
    if (step === 'merge') {
      assert.deepEqual(args.slice(0,5),['merge','-sparse','--failure-mode=any','-j','1']);
      const raw = args.slice(5,-2); assert.equal(raw.length,companion ? 5 : 1); assert.match(basename(raw[0]),/^test-.*\.profraw$/);
      if(f.rawBytes) { assert.equal(lstatSync(raw[0]).size,f.rawBytes); assert.equal(hashFile(raw[0]),f.rawSHA256); }
      else assert.equal(readFileSync(raw[0],'utf8'),'raw counts');
      writeFileSync(args.at(-1),'merged counts');
    }
    if (step.startsWith('export')) {
      assert(args.includes('--summary-only')); assert(args.includes('--format=text')); assert(args.includes('--object'));
      const objects=args.flatMap((arg,index)=>arg==='--object' ? [args[index+1]] : []);
      assert.deepEqual(objects,step==='export-combined' ? [artifact.executable,companion.executable]
        : [step==='export-companion' ? companion.executable : artifact.executable]);
      if(step==='export') {
        if(f.binaryBytes) assert.equal(lstatSync(args.at(-1)).size,f.binaryBytes);
        else assert.equal(readFileSync(args.at(-1),'utf8'),'instrumented fixture');
      }
      if(companion) {
        assert(args.includes('--debuginfod=false'));
        const lookup=args.find(arg=>arg.startsWith('--debug-file-directory='))?.slice('--debug-file-directory='.length);
        assert(lookup); assert.equal(existsSync(lookup),true);
        const shared=step==='export-combined' ? {...summary,lines:{count:10,covered:9}} : summary;
        stdout=exportBytes(f,[{filename:f.source,summary:shared},...(step==='export' ? [] : [{filename:f.mainSource,summary}])]);
      } else stdout = exportBytes(f);
    }
    const result = {status:0,handlesReaped:true,stdout,stderr:Buffer.alloc(0)};
    hooks[step]?.({result,owner,command,args,artifact,companion,f,calls});
    return result;
  }
  return {execute,calls,options};
}
function companionMock(f, hooks = {}) {
  f.mainSource=join(f.root,'crates/lycheedev-core/src/main.rs'); writeFileSync(f.mainSource,'fn main() {}\n');
  writeFileSync(join(f.root,'crates/lycheedev-core/Cargo.toml'),'[package]\nname = "lycheedev-core"\n[[bin]]\nname = "fixture_cli"\npath = "src/main.rs"\n');
  const runner=mock(f,hooks);
  Object.assign(runner.options,{lib:false,testName:'integration',exact:'case::one',companionBin:'fixture_cli'});
  return runner;
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
    assert.equal(verifyCoverageArchive(report.artifact.archive).sha256,report.artifact.sha256);
    assert.equal(verifyCoverageArchive(report.profiles[0]).sha256,report.profiles[0].sha256);
    assert.equal(report.schema,'lycheedev.rust-coverage.v2');
    assert.equal(existsSync(report.profiles[0].path),false);
    assert.equal(build.sealedLogs.some(row=>row.original.endsWith('part-000.bin')),false);
    const custody=JSON.parse(readFileSync(build.sealedLogs.find(row=>row.original.endsWith('custody.json')).path));
    assert.deepEqual(custody.profiles,report.profiles); assert.deepEqual(custody.artifact,report.artifact.archive);
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
test('raw profile beyond 128 MiB retains its owned scratch instead of omitting or deleting bytes',()=>{
  const f=fixture(); try {
    const runner=mock(f,{tests({owner}){truncateSync(owner.env.LLVM_PROFILE_FILE.replace('%m','123_0').replace('%p','99'),(128<<20)+1);}});
    const {report,receipt}=failed(f,runner,/file_budget/);
    assert.equal(report.rawArchived,false); assert.equal(receipt.state,'failed'); assert.equal(existsSync(receipt.directory),true);
    assert.equal(existsSync(join(f.root,'.tmp/rust-disk/channels/ordinary.lease')),false);
  } finally {f.cleanup();}
});
test('real-sized 32916376-byte profile and large binary/merged data seal as bounded parts and merge complete originals',()=>{
  const f=fixture(); try {
    f.rawBytes=32916376; f.binaryBytes=(16<<20)+1;
    const runner=mock(f,{
      cargo({artifact}) { truncateSync(artifact.executable,f.binaryBytes); },
      tests({owner}) { const path=owner.env.LLVM_PROFILE_FILE.replace('%m','123_0').replace('%p','99'); truncateSync(path,f.rawBytes); f.rawSHA256=hashFile(path); },
      merge({args}) { truncateSync(args.at(-1),(16<<20)+3); },
    });
    const result=collectRustCoverage(f.root,runner.options,{execute:runner.execute}), report=JSON.parse(readFileSync(result.report));
    assert.equal(report.complete,true); assert.equal(report.rawArchived,true); assert.equal(report.mergedArchived,true);
    for(const row of [report.artifact.archive,...report.profiles,report.merged]) {
      assert.equal(row.parts.length,2); assert.deepEqual(verifyCoverageArchive(row),{bytes:row.bytes,sha256:row.sha256});
      assert(row.parts.every(part=>part.bytes<=(16<<20)));
    }
    assert.equal(report.profiles[0].sha256,f.rawSHA256); assert.equal(report.profiles[0].bytes,32916376);
    assert.equal(existsSync(report.profiles[0].path),false); assert.equal(existsSync(report.merged.path),false);
    assert.equal(existsSync(report.artifact.path),true);
    for(const path of [result.coverageEvidence,result.buildEvidence]) {
      const owner=JSON.parse(readFileSync(path)); assert.equal(owner.state,'retired');
      for(const log of owner.sealedLogs) { assert(lstatSync(log.path).size<=(16<<20)); assert.equal(hashFile(log.path),log.sha256); }
    }
    const build=JSON.parse(readFileSync(result.buildEvidence));
    assert(build.sealedLogs.some(row=>row.original.endsWith('custody.json')));
    assert(!build.sealedLogs.some(row=>row.original.endsWith('.bin')&&row.original.includes('.parts')));
  } finally {f.cleanup();}
});
test('multiple individually bounded raw profiles still obey the original 128 MiB total',()=>{
  const f=fixture(); try {
    const runner=mock(f,{tests({owner}) {
      const first=owner.env.LLVM_PROFILE_FILE.replace('%m','123_0').replace('%p','99'); truncateSync(first,64<<20);
      const second=join(dirname(first),'test-124_0-100.profraw'); writeFileSync(second,'x'); truncateSync(second,(64<<20)+1);
    }});
    const {report,receipt}=failed(f,runner,/profile_total_budget/);
    assert.equal(report.rawArchived,false); assert.equal(receipt.state,'failed'); assert(existsSync(receipt.directory));
    assert.equal(runner.calls.at(-1).step,'tests');
  } finally {f.cleanup();}
});
test('raw profile count remains capped at 128 independently of byte size',()=>{
  const f=fixture(); try {
    const runner=mock(f,{tests({owner}) { const directory=dirname(owner.env.LLVM_PROFILE_FILE);
      for(let i=0;i<128;i++) writeFileSync(join(directory,`test-${i}-1.profraw`),'x');
    }});
    const {report,receipt}=failed(f,runner,/profile_count_budget/);
    assert.equal(report.rawArchived,false); assert(existsSync(receipt.directory)); assert.equal(runner.calls.at(-1).step,'tests');
  } finally {f.cleanup();}
});
test('merged output exceeding raw-plus-merged scratch allowance is rejected after production and retained',()=>{
  const f=fixture(); try {
    const runner=mock(f,{merge({args}) { truncateSync(args.at(-1),128<<20); }});
    const {report,receipt}=failed(f,runner,/scratch_total_budget/);
    assert.equal(report.rawArchived,true); assert.equal(report.mergedArchived,false); assert(existsSync(receipt.directory));
    assert.equal(lstatSync(join(receipt.directory,'coverage.profdata')).size,128<<20); assert.equal(runner.calls.at(-1).step,'merge');
  } finally {f.cleanup();}
});
test('archive reservation refuses cumulative binary and raw evidence before another large copy',()=>{
  const f=fixture(); try {
    f.binaryBytes=128<<20;
    const runner=mock(f,{cargo({artifact}) { truncateSync(artifact.executable,f.binaryBytes); },
      tests({owner}) { truncateSync(owner.env.LLVM_PROFILE_FILE.replace('%m','123_0').replace('%p','99'),120<<20); }});
    const {report,receipt}=failed(f,runner,/evidence_total_budget/);
    assert.equal(report.rawArchived,false); assert(existsSync(receipt.directory)); assert.equal(runner.calls.at(-1).step,'tests');
    assert.equal(verifyCoverageArchive(report.artifact.archive).bytes,128<<20);
    assert(report.observedEvidenceBytesBeforeReport<(256<<20));
  } finally {f.cleanup();}
});
test('producer evidence overflow keeps the original test failure and a bounded failure report',()=>{
  const f=fixture(); try {
    const runner=mock(f,{tests({owner,result}) {
      result.status=101; result.logs=[];
      for(let i=0;i<9;i++) { const path=join(owner.evidence,`fixture-large-${i}.bin`); writeFileSync(path,'x'); truncateSync(path,15<<20); result.logs.push(path); }
    }});
    const {report,receipt}=failed(f,runner,/tests_failed/);
    assert.match(report.error,/tests_failed/); assert.match(report.evidenceBudgetError,/evidence_total_budget/);
    assert.equal(report.complete,false); assert(existsSync(receipt.directory)); assert.equal(receipt.state,'failed');
  } finally {f.cleanup();}
});
test('Cargo output copies and final receipt growth cannot turn an over-budget archive into success',()=>{
  const f=fixture(); try {
    f.binaryBytes=128<<20; f.rawBytes=33498098;
    const runner=mock(f,{cargo({artifact,result}) {
      truncateSync(artifact.executable,f.binaryBytes);
      const prefix='{"reason":"compiler-message","message":"', suffix='"}\n';
      const padding=(16<<20)-result.stdout.length-Buffer.byteLength(prefix+suffix);
      result.stdout=Buffer.concat([result.stdout,Buffer.from(prefix+'x'.repeat(padding)+suffix)]);
      result.stderr=Buffer.alloc(16<<20,0x78);
    },tests({owner}) {
      const path=owner.env.LLVM_PROFILE_FILE.replace('%m','123_0').replace('%p','99');
      truncateSync(path,f.rawBytes); f.rawSHA256=hashFile(path);
    }});
    const {error,report,receipt}=failed(f,runner,/evidence_total_budget/);
    assert.equal(report.complete,false); assert.equal(report.rawArchived,false); assert(existsSync(receipt.directory));
    assert(usage(join(f.root,'.tmp/rust-disk/evidence')).bytes<=(256<<20));
    assert(report.reservedCoverageReceiptBytes>=lstatSync(error.coverageEvidence).size);
  } finally {f.cleanup();}
});
test('large raw data is archived even when the test fails and merge never starts',()=>{
  const f=fixture(); try {
    const runner=mock(f,{tests({owner,result}) { truncateSync(owner.env.LLVM_PROFILE_FILE.replace('%m','123_0').replace('%p','99'),32916376); result.status=101; }});
    const {report,receipt}=failed(f,runner,/tests_failed/);
    assert.equal(report.rawArchived,true); assert.equal(verifyCoverageArchive(report.profiles[0]).bytes,32916376);
    assert.equal(receipt.state,'retired'); assert.equal(runner.calls.at(-1).step,'tests');
  } finally {f.cleanup();}
});
test('failed merge that changes a previously archived raw input retains the changed scratch bytes',()=>{
  const f=fixture(); try {
    const runner=mock(f,{merge({args,result}) { writeFileSync(args[5],'changed raw'); result.status=1; }});
    const {report,receipt}=failed(f,runner,/merge_failed/);
    assert.equal(report.rawArchived,false); assert.match(report.profileArchiveError,/file_changed/); assert(existsSync(receipt.directory));
    assert.equal(readFileSync(report.profiles[0].path,'utf8'),'changed raw');
  } finally {f.cleanup();}
});
test('a raw profile appearing after archival prevents cleanup of unarchived bytes',()=>{
  const f=fixture(); try {
    const runner=mock(f,{merge({args,result}) { writeFileSync(join(dirname(args[5]),'test-999-100.profraw'),'late raw'); result.status=1; }});
    const {report,receipt}=failed(f,runner,/merge_failed/);
    assert.equal(report.rawArchived,false); assert.match(report.profileArchiveError,/profile_set_changed/); assert(existsSync(receipt.directory));
    assert.equal(readFileSync(join(dirname(report.profiles[0].path),'test-999-100.profraw'),'utf8'),'late raw');
  } finally {f.cleanup();}
});
test('failed export cannot discard a changed previously archived merged profile',()=>{
  const f=fixture(); try {
    const runner=mock(f,{export({args,result}) { writeFileSync(args.find(arg=>arg.startsWith('--instr-profile=')).split('=').slice(1).join('='),'changed merged'); result.status=1; }});
    const {report,receipt}=failed(f,runner,/export_failed/);
    assert.equal(report.mergedArchived,false); assert.match(report.mergedArchiveError,/file_changed/); assert(existsSync(receipt.directory));
    assert.equal(readFileSync(report.merged.path,'utf8'),'changed merged');
  } finally {f.cleanup();}
});
test('oversize report metadata becomes a bounded failure without masking it with seal log limits',()=>{
  const f=fixture(); try {
    const runner=mock(f,{rustc({result}) { result.stdout=Buffer.concat([result.stdout,Buffer.alloc(9<<20,0x78)]); }});
    const {error,report,receipt}=failed(f,runner,/evidence_total_budget/);
    assert.equal(report.complete,false); assert(report.omittedFromOversizeFailureReport.includes('compiler'));
    assert(lstatSync(error.coverageReport).size<(16<<20)); assert.equal(receipt.state,'failed'); assert(existsSync(receipt.directory));
    assert(receipt.sealedLogs.some(row=>lstatSync(row.path).size>(9<<20)));
  } finally {f.cleanup();}
});
test('archival finalization and custody index do not reset or escape the original deadline',()=>{
  const f=fixture(); try {
    let exported=false, tailChecks=0;
    const runner=mock(f,{export() { exported=true; }});
    const {report}=failed(f,runner,/deadline/,{now:()=>exported&&++tailChecks>=3 ? 600001 : 0});
    assert.equal(runner.calls.at(-1).step,'export'); assert.equal(report.complete,false); assert.equal(report.totalProcessBudgetMs,600000);
  } finally {f.cleanup();}
});
test('archive boundaries retain exact whole identity at 16 MiB minus one, exact and plus one',()=>{
  const f=fixture(); try {
    for(const bytes of [(16<<20)-1,16<<20,(16<<20)+1]) {
      const path=join(f.root,`profile-${bytes}`); writeFileSync(path,'data'); truncateSync(path,bytes);
      const source={path,bytes,sha256:hashFile(path)}, logs=[];
      const archived=archiveCoverageFile(source,join(f.root,`parts-${bytes}`),logs);
      assert.equal(archived.parts.length,Math.ceil(bytes/(16<<20))); assert.deepEqual(verifyCoverageArchive(archived),{bytes,sha256:source.sha256});
      assert(logs.every(file=>lstatSync(file).size<=(16<<20)));
    }
  } finally {f.cleanup();}
});
for(const mode of ['truncate-source','change-source','corrupt-copy','truncate-copy','zero-write']) test(`${mode} during streaming archive cannot authenticate or clean source`,()=>{
  const f=fixture(); try {
    const path=join(f.root,'source.profraw'); writeFileSync(path,Buffer.alloc(70000,1));
    const source={path,bytes:70000,sha256:hashFile(path)}, logs=[]; let once=false;
    const io={read(fd,buffer,offset,length,position) {
      const n=readSync(fd,buffer,offset,length,position);
      if(!once&&n) { once=true; if(mode==='truncate-source') truncateSync(path,69999); if(mode==='change-source') writeFileSync(path,Buffer.alloc(70000,2)); }
      if(!n&&mode==='truncate-copy') truncateSync(logs[0],1); return n;
    },write(fd,buffer,offset,length) {
      if(mode==='zero-write') return 0;
      const n=writeSync(fd,buffer,offset,length); if(mode==='corrupt-copy') writeSync(fd,Buffer.from([99]),0,1,0); return n;
    }};
    assert.throws(()=>archiveCoverageFile(source,join(f.root,'parts'),logs,io),/coverage\.(?:archive_|file_changed)/);
    assert(existsSync(path)); assert(logs.length>0); assert(logs.every(file=>lstatSync(file).size<=(16<<20)));
  } finally {f.cleanup();}
});
test('streaming archive completes partial writes and validates ordered part metadata and bytes',()=>{
  const f=fixture(); try {
    const path=join(f.root,'source.profraw'); writeFileSync(path,'fixture profile');
    const source={path,bytes:lstatSync(path).size,sha256:hashFile(path)}, logs=[];
    const row=archiveCoverageFile(source,join(f.root,'parts'),logs,{write:(fd,buffer,offset,length)=>writeSync(fd,buffer,offset,Math.min(length,3))});
    assert.deepEqual(verifyCoverageArchive(row),{bytes:source.bytes,sha256:source.sha256});
    for(const change of [x=>x.parts[0].offset++,x=>x.parts.pop(),x=>x.parts[0].path=path,x=>x.sha256='0'.repeat(64),x=>x.parts[0].sha256='0'.repeat(64)]) {
      const changed=structuredClone(row); change(changed); assert.throws(()=>verifyCoverageArchive(changed),/coverage\.archive_/);
    }
    writeFileSync(logs[0],'fixture Profile'); assert.throws(()=>verifyCoverageArchive(row),/archive_part_hash/);
  } finally {f.cleanup();}
});
test('partial profile archive failure seals partial parts and retains the raw owner',()=>{
  const f=fixture(); try {
    const runner=mock(f), archiveFile=(row,directory,logs)=>archiveCoverageFile(row,directory,logs,
      row.path.endsWith('.profraw') ? {write(){throw new Error('fixture write failure');}} : {});
    const {report,receipt}=failed(f,runner,/fixture write failure/,{archiveFile});
    assert.equal(report.rawArchived,false); assert.equal(receipt.state,'failed'); assert(existsSync(receipt.directory));
    assert(receipt.sealedLogs.some(log=>log.path.includes('.profraw.parts')&&lstatSync(log.path).size===0));
    assert.equal(runner.calls.at(-1).step,'tests');
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
test('reviewed LLVM JSON 2.0.1 and synthetic 3.1.0 summaries have the same owned-file meaning',()=>{
  const f=fixture(); try {
    const exported=llvm23Export(f), report=summarizeCoverage(Buffer.from(JSON.stringify(exported)),f.root);
    assert.deepEqual(report,summarizeCoverage(exportBytes(f),f.root));
    assert.deepEqual(Object.keys(exported.data[0]).sort(),['files','totals']);
    assert.deepEqual(Object.keys(exported.data[0].files[0]).sort(),['filename','summary']);
    assert.deepEqual(Object.keys(exported.data[0].totals).sort(),['branches','functions','instantiations','lines','mcdc','regions']);
    assert.deepEqual(report.files[0],{path:'crates/lycheedev-core/src/lib.rs',
      functions:{count:5,covered:3,uncovered:2},lines:{count:10,covered:8,uncovered:2},regions:{count:12,covered:7,uncovered:5}});
    assert.equal(Object.hasOwn(report,'totals'),false); assert.equal(Object.hasOwn(report.files[0],'branches'),false);
  } finally {f.cleanup();}
});
test('LLVM 23.1.1 pipeline accepts a synthetic 526-file 3.1.0 export without including dependency totals',()=>{
  const f=fixture(); try {
    const exported=llvm23Export(f,525), bytes=Buffer.from(JSON.stringify(exported));
    const version=({result})=>{result.stdout=Buffer.from(result.stdout.toString().replaceAll('22.1.0','23.1.1'));};
    const runner=mock(f,{rustc:version,'profdata-version':version,'cov-version':version,export({result}){result.stdout=bytes;}});
    const result=collectRustCoverage(f.root,runner.options,{execute:runner.execute});
    const report=JSON.parse(readFileSync(result.report)), receipt=JSON.parse(readFileSync(result.coverageEvidence));
    assert.equal(report.complete,true); assert.equal(report.compiler.llvm,'23.1.1');
    assert.equal(report.steps.length,10); assert(report.steps.every(step=>step.status===0));
    assert.equal(report.coverage.files.length,1); assert.equal(report.coverage.excluded.length,525);
    assert.equal(report.coverage.files[0].lines.count,10); assert.equal(exported.data[0].totals.lines.count,5260);
    assert.match(report.coverage.denominator,/No workspace percentage/); assert.equal(Object.hasOwn(report.coverage,'totals'),false);
    const exportLog=receipt.sealedLogs.find(row=>row.original===report.steps.at(-1).stdout);
    assert(exportLog); assert.deepEqual(readFileSync(exportLog.path),bytes);
    assert.equal(receipt.state,'retired'); assert.equal(existsSync(receipt.directory),false);
  } finally {f.cleanup();}
});
test('LLVM export rejects unknown or malformed versions and incorrect type identifiers',()=>{
  const f=fixture(); try {
    for(const version of [undefined,null,3,{},[],['3.1.0'],'','2','2.','2.not-a-version','2.0.0','2.0.2',
      '3','3.0.1','3.1.0\n','3.1.0-extra','3.1.1','3.2.0','4.0.0']) {
      const exported=llvm23Export(f); exported.version=version;
      assert.throws(()=>summarizeCoverage(Buffer.from(JSON.stringify(exported)),f.root),/coverage\.export_schema/,`version ${JSON.stringify(version)}`);
    }
    for(const type of [undefined,null,1,{},[],['llvm.coverage.json.export'],'LLVM.coverage.json.export','llvm.coverage.json','']) {
      const exported=llvm23Export(f); exported.type=type;
      assert.throws(()=>summarizeCoverage(Buffer.from(JSON.stringify(exported)),f.root),/coverage\.export_schema/);
    }
  } finally {f.cleanup();}
});
test('LLVM export validates root, single data object, files and consumed summary shapes',()=>{
  const f=fixture(); try {
    const encode=value=>Buffer.from(JSON.stringify(value));
    for(const root of [null,[],true,0,'export',{}])
      assert.throws(()=>summarizeCoverage(encode(root),f.root),/coverage\.export_schema/);
    for(const data of [undefined,null,{},[],[null],[[]],[{}],[{files:null}],[{files:{}}],[{files:[]},{files:[]}]]) {
      const exported=llvm23Export(f); exported.data=data;
      assert.throws(()=>summarizeCoverage(encode(exported),f.root),/coverage\.export_schema/);
    }
    for(const file of [null,[],false,3,'file']) {
      const exported=llvm23Export(f); exported.data[0].files=[file];
      assert.throws(()=>summarizeCoverage(encode(exported),f.root),/coverage\.export_schema/);
    }
    for(const filename of [undefined,null,1,{},[],['/file.rs'],'relative.rs','']) {
      const exported=llvm23Export(f); exported.data[0].files[0].filename=filename;
      assert.throws(()=>summarizeCoverage(encode(exported),f.root),/coverage\.export_filename/);
    }
    for(const badSummary of [undefined,null,[],false,5,'summary',{}]) {
      const exported=llvm23Export(f); exported.data[0].files[0].summary=badSummary;
      assert.throws(()=>summarizeCoverage(encode(exported),f.root),/coverage\.export_metric/);
    }
    for(const metric of ['functions','lines','regions']) for(const row of [undefined,null,[],false,3,'metric',{},
      {count:'1',covered:0},{count:1,covered:'0'},{count:-1,covered:0},{count:1,covered:-1},
      {count:1,covered:2},{count:0.5,covered:0},{count:2,covered:0.5},{count:2**53,covered:0},{count:1,covered:Infinity}]) {
      const exported=llvm23Export(f); exported.data[0].files[0].summary[metric]=row;
      assert.throws(()=>summarizeCoverage(encode(exported),f.root),/coverage\.export_metric/);
    }
  } finally {f.cleanup();}
});
test('unsupported LLVM export schema fails after native-zero export and preserves the exact failure output',()=>{
  const f=fixture(); try {
    const exported=llvm23Export(f); exported.version='3.1.1'; const bytes=Buffer.from(JSON.stringify(exported));
    const runner=mock(f,{export({result}){result.stdout=bytes;}}), {report,receipt}=failed(f,runner,/coverage\.export_schema/);
    assert.equal(report.complete,false); assert.equal(report.coverage,undefined);
    assert.equal(runner.calls.at(-1).step,'export'); assert.equal(report.steps.at(-1).status,0);
    const exportLog=receipt.sealedLogs.find(row=>row.original===report.steps.at(-1).stdout);
    assert(exportLog); assert.deepEqual(readFileSync(exportLog.path),bytes);
    assert.equal(receipt.state,'retired'); assert.equal(report.rawArchived,true);
  } finally {f.cleanup();}
});

test('companion option is explicit, integration-only and keeps the original selection shape by default',()=>{
  const args=['--package','lycheedev-cli','--test','source_inspect_public','--exact','case::one','--companion-bin','lycheedev'];
  assert.deepEqual(coverageArguments(args),{packageName:'lycheedev-cli',testName:'source_inspect_public',exact:'case::one',companionBin:'lycheedev'});
  assert.deepEqual(coverageSelection(coverageArguments(args)),{packageName:'lycheedev-cli',kind:'test',testName:'source_inspect_public',exact:'case::one',companionBin:'lycheedev'});
  assert.equal(Object.hasOwn(selection,'companionBin'),false);
  for(const value of ['',null,1,'../lycheedev','lycheedev.exe','--bin','a'.repeat(129)])
    assert.throws(()=>coverageSelection({packageName:'lycheedev-cli',testName:'integration',companionBin:value}),/invalid_companion_bin/);
  assert.throws(()=>coverageSelection({packageName:'lycheedev-cli',lib:true,companionBin:'lycheedev'}),/companion_integration_required/);
  for(const suffix of [['--companion-bin'],['--companion-bin','a','--companion-bin','b']])
    assert.throws(()=>coverageArguments(['--package','lycheedev-cli','--test','integration',...suffix]),/invalid_option/);
});

test('opt-in pipeline authenticates two same-build roles, witnesses the companion and uses LLVM union counts',()=>{
  const f=fixture(); try {
    const runner=companionMock(f), result=collectRustCoverage(f.root,runner.options,{execute:runner.execute});
    const report=JSON.parse(readFileSync(result.report)), coverage=JSON.parse(readFileSync(result.coverageEvidence)), build=JSON.parse(readFileSync(result.buildEvidence));
    assert.equal(report.schema,'lycheedev.rust-coverage.v3'); assert.equal(report.complete,true);
    assert.deepEqual(runner.calls.map(row=>row.step),['rustc','sysroot','cargo-version','cargo-launch-version','profdata-version','cov-version','cargo','tests','merge','export','export-companion','export-combined']);
    assert.equal(report.artifact.role,'test'); assert.equal(report.companionArtifact.role,'companion');
    assert.notEqual(report.artifact.path,report.companionArtifact.path);
    assert.equal(report.artifact.cargo.packageId,report.companionArtifact.cargo.packageId);
    assert.equal(report.companionArtifact.cargo.fresh,true); assert.equal(report.companionArtifact.cargo.profile.test,false);
    assert.equal(report.companionArtifact.cargo.profile.opt_level,'0'); assert.deepEqual(report.companionArtifact.cargo.features,['fixture']);
    assert.equal(report.companionArtifact.cargo.target.src_path,f.mainSource);
    assert.equal(report.profiles.length,5); assert.equal(report.tests.passed,1); assert.equal(report.tests.ignored,0); assert.equal(report.tests.filtered,5);
    assert.equal(report.coverage.files.find(file=>file.path.endsWith('lib.rs')).lines.covered,9);
    assert.equal(report.testCoverage.files[0].lines.covered,8); assert.equal(report.companionCoverage.files[0].lines.covered,8);
    assert.equal(report.companionWitness.source,'crates/lycheedev-core/src/main.rs'); assert.equal(report.companionWitness.absentFromTestMap,true);
    assert.equal(report.companionWitness.companionCoveredLines,8); assert.match(report.companionWitness.scope,/not complete capture/);
    assert.match(report.coverage.denominator,/never sums/); assert.equal(Object.hasOwn(report.coverage,'totals'),false);
    assert.equal(report.totalProcessBudgetMs,600000); assert.equal(report.budgets.rawBytes,128<<20); assert.equal(report.budgets.scratchBytes,128<<20);
    assert.equal(report.budgets.evidenceBytes,256<<20); assert.equal(report.budgets.evidenceMetadataReserveBytes,16<<20);
    assert.equal(basename(report.profilePattern),'test-%m-%p.profraw');
    const custody=JSON.parse(readFileSync(build.sealedLogs.find(row=>row.original.endsWith('custody.json')).path));
    assert.equal(custody.schema,'lycheedev.coverage-custody.v2'); assert.equal(Object.hasOwn(custody,'artifact'),false);
    assert.deepEqual(custody.objects.map(row=>row.role),['test','companion']);
    assert.deepEqual(custody.objects.map(row=>row.archive),[report.artifact.archive,report.companionArtifact.archive]);
    for(const object of [report.artifact,report.companionArtifact]) {
      assert.equal(verifyCoverageArchive(object.archive).sha256,object.sha256); assert.equal(hashFile(object.path),object.sha256);
    }
    for(const log of coverage.sealedLogs) assert.equal(hashFile(log.path),log.sha256);
    assert.equal(build.sealedLogs.some(row=>row.original.endsWith('part-000.bin')),false);
    assert.equal(coverage.state,'retired'); assert.equal(build.state,'retired'); assert.equal(existsSync(coverage.directory),false);
    assert.equal(existsSync(join(f.root,'.tmp/rust-disk/channels/ordinary.lease')),false);
  } finally {f.cleanup();}
});

for(const [name,change,expected] of [
  ['absent',rows=>rows.splice(1,1),/companion_cardinality/],
  ['duplicate',rows=>rows.splice(1,0,structuredClone(rows[1])),/companion_cardinality/],
  ['foreign manifest',rows=>rows[1].manifest_path+='-other',/companion_cardinality/],
  ['foreign package',rows=>rows[1].package_id+='-other',/companion_package_identity/],
  ['missing test package',rows=>delete rows[0].package_id,/artifact_package_identity/],
  ['missing companion package',rows=>delete rows[1].package_id,/companion_package_identity/],
  ['wrong name',rows=>rows[1].target.name='other',/companion_cardinality/],
  ['wrong kind',rows=>rows[1].target.kind=['test'],/companion_cardinality/],
  ['mixed kind',rows=>rows[1].target.kind=['bin','test'],/companion_cardinality/],
  ['harness profile',rows=>rows[1].profile.test=true,/companion_cardinality/],
  ['missing profile',rows=>delete rows[1].profile.test,/companion_cardinality/],
  ['null executable',rows=>rows[1].executable=null,/companion_cardinality/],
  ['relative executable',rows=>rows[1].executable='other.exe',/companion_path/],
  ['undeclared executable',rows=>rows[1].filenames=[],/companion_executable/],
  ['string filenames',rows=>rows[1].filenames=rows[1].executable,/companion_executable/],
  ['same object',rows=>{rows[1].executable=rows[0].executable;rows[1].filenames=[rows[0].executable];},/companion_same_object/],
  ['wrong source',rows=>rows[1].target.src_path=rows[0].target.src_path,/companion_source/],
  ['outside executable',rows=>{rows[1].executable=rows[1].target.src_path;rows[1].filenames=[rows[1].executable];},/path_outside_owned_root/],
  ['Cargo failure message',rows=>rows.at(-1).success=false,/cargo_completion/],
]) test(`companion selection rejects ${name} before execution without guessing a nearby binary`,()=>{
  const f=fixture(); try {
    const runner=companionMock(f,{cargo({result}) { const rows=result.stdout.toString().trim().split('\n').map(JSON.parse); change(rows); result.stdout=jsonl(rows); }});
    const {report,receipt}=failed(f,runner,expected);
    assert.equal(report.complete,false); assert.equal(runner.calls.at(-1).step,'cargo');
    assert.equal(receipt.state,'retired'); assert.equal(existsSync(receipt.directory),false);
    assert.equal(report.companionArtifact,undefined); assert.equal(report.coverage,undefined);
  } finally {f.cleanup();}
});

test('same-stream selection retains Cargo fields and ignores an unrelated package binary',()=>{
  const f=fixture(); try {
    const runner=companionMock(f,{cargo({result,owner,companion}) {
      const rows=result.stdout.toString().trim().split('\n').map(JSON.parse);
      rows.unshift({...structuredClone(companion),manifest_path:join(f.root,'unrelated/Cargo.toml'),package_id:'unrelated'});
      const selection=coverageSelection(runner.options), selected=selectCoverageArtifacts(jsonl(rows),f.root,owner.env.CARGO_TARGET_DIR,selection);
      assert.deepEqual(selected.companion.row,companion); assert.equal(selected.companion.row.fresh,true);
      result.stdout=jsonl(rows);
    }});
    const result=collectRustCoverage(f.root,runner.options,{execute:runner.execute}); assert.equal(JSON.parse(readFileSync(result.report)).complete,true);
  } finally {f.cleanup();}
});

for(const objectRole of ['test','companion']) for(const step of ['tests','merge','export','export-companion','export-combined'])
  test(`${objectRole} original mutation at ${step} cannot become companion coverage success`,()=>{
    const f=fixture(); try {
      const runner=companionMock(f,{[step]({artifact,companion}) {writeFileSync((objectRole==='test' ? artifact : companion).executable,'changed object');}});
      const {report,receipt}=failed(f,runner,/file_changed/);
      assert.equal(report.complete,false); assert.match(report.objectVerificationError,/file_changed/);
      assert.equal(runner.calls.at(-1).step,step); assert.equal(receipt.state,'retired');
      const object=objectRole==='test' ? report.artifact : report.companionArtifact;
      assert.notEqual(hashFile(object.path),object.sha256); assert.equal(verifyCoverageArchive(object.archive).sha256,object.sha256);
    } finally {f.cleanup();}
  });

for(const objectRole of ['test','companion']) for(const corruption of ['original','part','whole-hash'])
  test(`${objectRole} ${corruption} corruption before execution is rejected`,()=>{
    const f=fixture(); try {
      const runner=companionMock(f), archived=[];
      const archiveFile=(row,directory,logs)=>{
        const archive=archiveCoverageFile(row,directory,logs); archived.push(archive);
        if(archived.length===2) {
          const victim=archived[objectRole==='test' ? 0 : 1];
          if(corruption==='original') writeFileSync(victim.path,'changed');
          else if(corruption==='part') writeFileSync(victim.parts[0].path,Buffer.alloc(victim.parts[0].bytes,0x78));
          else victim.sha256='0'.repeat(64);
        }
        return archive;
      };
      const {report,receipt}=failed(f,runner,/file_changed|archive_part_hash|archive_whole_hash/,{archiveFile});
      assert.equal(report.complete,false); assert.equal(runner.calls.at(-1).step,'cargo'); assert.equal(receipt.state,'retired');
      assert(report.objectVerificationError);
    } finally {f.cleanup();}
  });

for(const step of ['tests','export','export-companion','export-combined']) test(`companion archive corruption after ${step} is retained as failure`,()=>{
  const f=fixture(); try {
    const runner=companionMock(f,{[step]({owner}) {
      const part=join(owner.evidence,'instrumented-companion.exe.parts/part-000.bin'); writeFileSync(part,Buffer.alloc(lstatSync(part).size,0x78));
    }});
    const {report,receipt}=failed(f,runner,/archive_part_hash/);
    assert.equal(report.complete,false); assert.match(report.objectVerificationError,/archive_part_hash/);
    assert.equal(runner.calls.at(-1).step,step); assert.equal(receipt.state,'retired');
    assert(receipt.sealedLogs.some(row=>row.original.includes('instrumented-companion.exe.parts')));
  } finally {f.cleanup();}
});

for(const [name,step,change,expected] of [
  ['test unexpectedly maps companion','export',(x,f)=>x.data[0].files.push({filename:f.mainSource,summary}),/companion_witness_in_test/],
  ['companion source absent','export-companion',x=>x.data[0].files.pop(),/companion_witness_missing/],
  ['companion source zero','export-companion',x=>{x.data[0].files.at(-1).summary={functions:{count:5,covered:0},lines:{count:10,covered:0},regions:{count:12,covered:0}};},/companion_witness_missing/],
  ['combined loses source','export-combined',x=>x.data[0].files.pop(),/combined_file_union/],
  ['combined duplicate filename','export-combined',x=>x.data[0].files.push(x.data[0].files[0]),/duplicate_export_file/],
  ['combined multiple data entries','export-combined',x=>x.data.push(x.data[0]),/export_schema/],
  ['combined unknown schema','export-combined',x=>x.version='4.0.0',/export_schema/],
  ['combined impossible counts','export-combined',x=>x.data[0].files[0].summary.lines.covered=11,/export_metric/],
]) test(`${name} fails the explicit companion proof without a percentage`,()=>{
  const f=fixture(); try {
    const runner=companionMock(f,{[step]({result}) {const x=JSON.parse(result.stdout);change(x,f);result.stdout=Buffer.from(JSON.stringify(x));}});
    const {report,receipt}=failed(f,runner,expected);
    assert.equal(report.complete,false); assert.equal(report.coverage,undefined); assert.equal(receipt.state,'retired');
    assert.equal(report.rawArchived,true); assert.equal(report.mergedArchived,true);
  } finally {f.cleanup();}
});

for(const step of ['cargo','tests','merge','export','export-companion','export-combined']) test(`companion ${step} failure preserves raw custody and does not start a later child`,()=>{
  const f=fixture(); try {
    const runner=companionMock(f,{[step]({result}){result.status=1;}}), {report,receipt}=failed(f,runner,new RegExp(`${step}_failed`));
    assert.equal(report.complete,false); assert.equal(runner.calls.at(-1).step,step); assert.equal(receipt.state,'retired');
    assert.equal(report.rawArchived,true); assert.equal(report.profiles.length,step==='cargo' ? 0 : 5);
    assert.equal(existsSync(receipt.directory),false); assert.equal(existsSync(join(f.root,'.tmp/rust-disk/channels/ordinary.lease')),false);
  } finally {f.cleanup();}
});

test('unreaped companion test retains both owners and skips archive reauthentication',()=>{
  const f=fixture(); try {
    const runner=companionMock(f,{tests({result}){result.handlesReaped=false;}}), {report,receipt}=failed(f,runner,/tree_reap_unverified/);
    assert.equal(report.rawArchived,false); assert.equal(receipt.state,'interrupted'); assert.equal(existsSync(receipt.directory),true);
    assert.equal(existsSync(join(f.root,'.tmp/rust-disk/channels/ordinary.lease')),true); assert.equal(runner.calls.at(-1).step,'tests');
    assert.equal(JSON.parse(readFileSync(report.buildEvidence)).state,'interrupted');
  } finally {f.cleanup();}
});

for(const step of ['tests','export-companion']) test(`external object lookup contamination at ${step} prevents further export`,()=>{
  const f=fixture(); try {
    const runner=companionMock(f,{[step]({owner}) {writeFileSync(join(dirname(owner.env.LLVM_PROFILE_FILE),'../empty-object-lookup/foreign.exe'),'foreign');}});
    const {report,receipt}=failed(f,runner,/external_object_lookup/);
    assert.equal(report.complete,false); assert.equal(report.coverage,undefined); assert.equal(report.externalObjectDiscovery.debuginfod,false);
    assert.equal(runner.calls.filter(row=>row.step.startsWith('export')).length,step==='tests' ? 0 : 2);
    assert.equal(receipt.state,'failed'); assert.equal(existsSync(receipt.directory),true);
    assert.equal(readFileSync(join(report.externalObjectDiscovery.debugFileDirectory,'foreign.exe'),'utf8'),'foreign');
  } finally {f.cleanup();}
});

test('companion exports consume the original remaining process budget',()=>{
  const f=fixture(); try {
    let time=0;
    const runner=companionMock(f,{'export-companion'(){time=600001;}}), {report}=failed(f,runner,/deadline/,{now:()=>time});
    assert.equal(report.totalProcessBudgetMs,600000); assert.equal(runner.calls.at(-1).step,'export-companion'); assert.equal(report.coverage,undefined);
  } finally {f.cleanup();}
});

test('two individually bounded objects must fit cumulative evidence admission before either archive',()=>{
  const f=fixture(); try {
    const runner=companionMock(f,{cargo({artifact,companion}) {truncateSync(artifact.executable,128<<20);truncateSync(companion.executable,128<<20);}});
    let copies=0;
    const {report,receipt}=failed(f,runner,/evidence_total_budget/,{archiveFile(...args){copies++;return archiveCoverageFile(...args);}});
    assert.equal(copies,0); assert.equal(report.artifact,undefined); assert.equal(report.companionArtifact,undefined);
    assert.equal(runner.calls.at(-1).step,'cargo'); assert.equal(report.complete,false); assert.equal(receipt.state,'retired');
  } finally {f.cleanup();}
});

test('two object bytes plus exports and build copies remain subject to the original evidence reserve',()=>{
  const f=fixture(); try {
    f.binaryBytes=112<<20;
    const runner=companionMock(f,{cargo({artifact,companion}) {truncateSync(artifact.executable,f.binaryBytes);truncateSync(companion.executable,112<<20);},
      'export-companion'({result}) {result.stdout=Buffer.alloc(9<<20,0x78);}});
    const {report}=failed(f,runner,/evidence_total_budget/);
    assert.equal(report.complete,false); assert.equal(runner.calls.at(-1).step,'export-companion');
    assert.equal(report.artifact.bytes,112<<20); assert.equal(report.companionArtifact.bytes,112<<20);
    assert.equal(report.budgets.evidenceBytes,256<<20);
  } finally {f.cleanup();}
});

test('distinct companion pathname cannot alias the selected executable through a hard link',()=>{
  const f=fixture(); try {
    const runner=companionMock(f,{cargo({artifact,companion}) {rmSync(companion.executable);linkSync(artifact.executable,companion.executable);}});
    const {report}=failed(f,runner,/companion_same_object/); assert.equal(report.complete,false); assert.equal(runner.calls.at(-1).step,'cargo');
  } finally {f.cleanup();}
});

for(const delta of [0,1]) test(`combined object admission at evidence reserve boundary ${delta ? 'plus one' : 'exact'} is byte-exact`,()=>{
  const f=fixture(); try {
    let archiveCalls=0;
    const runner=companionMock(f,{cargo({artifact,companion,owner,result}) {
      const previousCopies=readdirSync(owner.evidence).filter(name=>name.endsWith('.bin')).reduce((sum,name)=>sum+lstatSync(join(owner.evidence,name)).size,0);
      const currentOutput=result.stdout.length+result.stderr.length;
      const budget=(256<<20)-(16<<20)-usage(dirname(owner.evidence)).bytes-previousCopies-3*currentOutput;
      f.binaryBytes=120<<20; truncateSync(artifact.executable,f.binaryBytes); truncateSync(companion.executable,budget-f.binaryBytes+delta);
    }});
    const {report}=failed(f,runner,delta ? /evidence_total_budget/ : /fixture exact object admission reached/,
      {archiveFile() {archiveCalls++;throw new Error('fixture exact object admission reached');}});
    assert.equal(archiveCalls,delta ? 0 : 1); assert.equal(runner.calls.at(-1).step,'cargo'); assert.equal(report.complete,false);
  } finally {f.cleanup();}
});

for(const step of ['export-companion','export-combined']) test(`${step} diagnostics are retained and reject acceptance despite native zero`,()=>{
  const f=fixture(); try {
    const runner=companionMock(f,{[step]({result}) {result.stderr=Buffer.from('warning: incomplete coverage');}});
    const {report,receipt}=failed(f,runner,/export_diagnostics/);
    assert.equal(report.complete,false); assert.equal(report.steps.at(-1).status,0); assert.equal(runner.calls.at(-1).step,step);
    const log=receipt.sealedLogs.find(row=>row.original===report.steps.at(-1).stderr); assert.equal(readFileSync(log.path,'utf8'),'warning: incomplete coverage');
  } finally {f.cleanup();}
});

test('combined owned file absent from both authenticated object maps is rejected',()=>{
  const f=fixture(); try {
    const extra=join(f.root,'crates/lycheedev-core/src/unregistered.rs'); writeFileSync(extra,'fn unregistered() {}\n');
    const runner=companionMock(f,{'export-combined'({result}) {
      const data=JSON.parse(result.stdout);data.data[0].files.push({filename:extra,summary});result.stdout=Buffer.from(JSON.stringify(data));
    }});
    const {report}=failed(f,runner,/combined_file_union/);assert.equal(report.complete,false);assert.equal(report.coverage,undefined);
  } finally {f.cleanup();}
});

test('late raw mutation in the combined export retains unarchived scratch without losing the primary native failure',()=>{
  const f=fixture(); try {
    const runner=companionMock(f,{'export-combined'({owner,result}) {
      const path=owner.env.LLVM_PROFILE_FILE.replace('%m','456_0').replace('%p','100');writeFileSync(path,'changed child counts');result.status=1;
    }});
    const {report,receipt}=failed(f,runner,/export-combined_failed/);
    assert.equal(report.rawArchived,false);assert.match(report.profileArchiveError,/file_changed/);assert.equal(existsSync(receipt.directory),true);
    assert.equal(report.complete,false);assert.equal(receipt.state,'failed');
  } finally {f.cleanup();}
});

test('compound object mutation and lookup contamination preserve the first error and all foreign scratch',()=>{
  const f=fixture(); try {
    const runner=companionMock(f,{tests({artifact,owner}) {
      writeFileSync(artifact.executable,'changed object');
      writeFileSync(join(dirname(owner.env.LLVM_PROFILE_FILE),'../empty-object-lookup/foreign.exe'),'foreign');
    }});
    const {report,receipt}=failed(f,runner,/file_changed/);
    assert.equal(report.complete,false);assert.match(report.error,/file_changed/);assert.match(report.objectVerificationError,/file_changed/);
    assert.match(report.objectLookupError,/external_object_lookup/);assert.equal(receipt.state,'failed');assert.equal(existsSync(receipt.directory),true);
    assert.equal(readFileSync(join(report.externalObjectDiscovery.debugFileDirectory,'foreign.exe'),'utf8'),'foreign');
    assert.equal(report.rawArchived,true);assert.equal(runner.calls.at(-1).step,'tests');
  } finally {f.cleanup();}
});
