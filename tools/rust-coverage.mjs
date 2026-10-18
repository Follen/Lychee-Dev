// Selected Windows test-binary coverage, not a workspace acceptance percentage.
// Rust's supported instrument-coverage workflow and Cargo JSON artifact contract:
// https://doc.rust-lang.org/rustc/instrument-coverage.html
// https://doc.rust-lang.org/cargo/reference/external-tools.html
// https://llvm.org/docs/CommandGuide/llvm-cov.html
// https://llvm.org/docs/CommandGuide/llvm-profdata.html
import assert from 'node:assert/strict';
import {copyFileSync, existsSync, lstatSync, mkdirSync, readFileSync, readdirSync, realpathSync, writeFileSync} from 'node:fs';
import {basename, dirname, isAbsolute, join, relative, resolve, sep} from 'node:path';
import {performance} from 'node:perf_hooks';
import {beginScratch, contained, sha256} from './rust-disk.mjs';
import {hashFile, runCargo} from './rust-build.mjs';
import {ownedSpawnSync} from './rust-owned-process.mjs';

const TARGET = 'x86_64-pc-windows-msvc', RUST = '1.99.0';
const FILE_LIMIT = 16 << 20, RAW_LIMIT = 128 << 20;
const FLAGS = ['-C', 'target-feature=+crt-static', '-C', 'instrument-coverage'];
const SUPPORT = ['rust-coverage.mjs', 'rust-build.mjs', 'rust-disk.mjs', 'rust-owned-process.mjs', 'rust-owned-process.ps1', 'rust-build-auxiliary.json'];
const save = (path, value) => writeFileSync(path, JSON.stringify(value, null, 2) + '\n', {flag:'wx'});
const check = (value, code) => assert(value, `coverage.${code}`);
const supportIdentity = () => Object.fromEntries(SUPPORT.map(name => [name, hashFile(join(import.meta.dirname, name))]));

function text(bytes) {
  check(Buffer.isBuffer(bytes) && bytes.length <= FILE_LIMIT, 'output_budget');
  return new TextDecoder('utf-8', {fatal:true}).decode(bytes).replaceAll('\r\n', '\n');
}
function regular(path, limit = FILE_LIMIT) {
  const info = lstatSync(path);
  check(info.isFile() && !info.isSymbolicLink() && info.size > 0 && info.size <= limit, 'file_budget');
  return {path, bytes:info.size, sha256:hashFile(path, limit)};
}
function unchanged(row) { check(regular(row.path, Math.max(FILE_LIMIT, row.bytes)).sha256 === row.sha256, 'file_changed'); }

export function coverageSelection({packageName, testName, lib = false, exact} = {}) {
  check(typeof packageName === 'string' && /^lycheedev-[a-z0-9-]+$/.test(packageName), 'package_required');
  check(typeof lib === 'boolean' && (lib !== (testName !== undefined)), 'one_target_required');
  if (testName !== undefined) check(typeof testName === 'string' && /^[A-Za-z0-9_][A-Za-z0-9_-]{0,127}$/.test(testName), 'invalid_test_target');
  if (exact !== undefined) check(typeof exact === 'string' && /^[A-Za-z0-9_][A-Za-z0-9_:]{0,511}$/.test(exact), 'invalid_exact_filter');
  return {packageName, kind:lib ? 'lib' : 'test', ...(testName ? {testName} : {}), ...(exact ? {exact} : {})};
}

export function coverageEnvironment(environment) {
  for (const key of Object.keys(environment)) {
    if (/^(?:RUSTFLAGS|RUSTDOCFLAGS|RUSTC|RUSTC_WRAPPER|RUSTC_WORKSPACE_WRAPPER|RUSTC_BOOTSTRAP|LLVM_PROFILE_FILE|CARGO_ENCODED_RUSTFLAGS|CARGO_ENCODED_RUSTDOCFLAGS|CARGO_TARGET_DIR|CARGO_BUILD_(?:TARGET|BUILD_DIR|TARGET_DIR|RUSTC|RUSTFLAGS|RUSTC_WRAPPER|RUSTC_WORKSPACE_WRAPPER)|CARGO_TARGET_.*_(?:RUSTFLAGS|RUNNER)|CARGO_PROFILE_.*|RUST_TEST_.*|LLVM_PROFILE_.*)$/i.test(key))
      check(!environment[key], `uncontrolled_environment:${key}`);
  }
  return {...environment};
}

export function selectCoverageArtifact(bytes, root, target, selection) {
  const rows = text(bytes).split(/\r?\n/).filter(Boolean).map(line => JSON.parse(line));
  check(rows.length <= 65536, 'cargo_message_budget');
  const finished = rows.filter(row => row.reason === 'build-finished');
  check(finished.length === 1 && finished[0].success === true, 'cargo_completion');
  const manifest = join(root, 'crates', selection.packageName, 'Cargo.toml');
  const found = rows.filter(row => row.reason === 'compiler-artifact' && row.manifest_path === manifest
    && row.target?.kind?.length === 1 && row.target.kind[0] === selection.kind
    && (selection.kind === 'lib' || row.target.name === selection.testName)
    && row.profile?.test === true && row.executable);
  check(found.length === 1, 'artifact_cardinality');
  const row = found[0];
  check(isAbsolute(row.executable), 'artifact_path');
  const path = contained(target, row.executable);
  check(path.endsWith('.exe') && row.filenames?.includes(row.executable), 'artifact_executable');
  contained(join(root, 'crates', selection.packageName), row.target.src_path);
  return {row, ...regular(path, 128 << 20)};
}

export function verifyCoverageTests(bytes, selection) {
  const output = text(bytes).replaceAll('\r\n', '\n');
  const summaries = [...output.matchAll(/^test result: (.+)$/gm)];
  check(summaries.length === 1, 'test_completion');
  const counts = /^ok\. (\d+) passed; 0 failed; (\d+) ignored; 0 measured; (\d+) filtered out; finished in /.exec(summaries[0][1]);
  check(counts, 'test_failure');
  const [passed, ignored, filtered] = counts.slice(1).map(Number);
  check([passed, ignored, filtered].every(Number.isSafeInteger) && passed > 0, 'no_tests_executed');
  const canonicalName = name => name.replace(/ - should panic$/, '');
  const names = [...output.matchAll(/^test (.+) \.\.\. ok$/gm)].map(row => canonicalName(row[1]));
  const ignoredNames = [...output.matchAll(/^test (.+) \.\.\. ignored(?:,.*)?$/gm)].map(row => canonicalName(row[1]));
  check(names.length === passed && ignoredNames.length === ignored && new Set([...names, ...ignoredNames]).size === passed + ignored, 'test_rows');
  check(selection.exact ? passed === 1 && ignored === 0 && names[0] === selection.exact : filtered === 0, 'test_selection');
  return {passed, ignored, filtered, names, ignoredNames};
}

export function summarizeCoverage(bytes, root) {
  const exported = JSON.parse(text(bytes));
  check(exported.type === 'llvm.coverage.json.export' && /^2\./.test(exported.version)
    && Array.isArray(exported.data) && exported.data.length === 1 && Array.isArray(exported.data[0].files), 'export_schema');
  const ownedRoot = join(root, 'crates'), files = [], excluded = [], seen = new Set();
  check(exported.data[0].files.length <= 16384, 'export_file_budget');
  for (const file of exported.data[0].files) {
    check(typeof file.filename === 'string' && isAbsolute(file.filename), 'export_filename');
    const path = resolve(file.filename), rel = relative(ownedRoot, path);
    check(!seen.has(path), 'duplicate_export_file'); seen.add(path);
    if (!rel || rel === '..' || rel.startsWith(`..${sep}`) || isAbsolute(rel) || !path.endsWith('.rs')) { excluded.push(file.filename); continue; }
    contained(ownedRoot, path); regular(path);
    const metrics = {};
    for (const metric of ['functions', 'lines', 'regions']) {
      const row = file.summary?.[metric];
      check(row && Number.isSafeInteger(row.count) && row.count >= 0 && Number.isSafeInteger(row.covered)
        && row.covered >= 0 && row.covered <= row.count, 'export_metric');
      metrics[metric] = {count:row.count, covered:row.covered, uncovered:row.count-row.covered};
    }
    files.push({path:relative(root, path).split(sep).join('/'), ...metrics});
  }
  check(files.length > 0, 'no_owned_coverage');
  return {files:files.sort((a,b) => a.path.localeCompare(b.path)), excluded,
    denominator:'Only Rust source files under crates/ represented in this selected executable’s coverage map. Includes mapped test code and macro expansions; excludes registry, vendor, standard-library and generated files outside crates/. Only the selected target/filter test invocation runs; ignored tests are not automatically run, but their mapped code remains in the denominator. Separate descendant object maps, unmapped files, other target maps and doctest maps are excluded; matching descendant profile counts may contribute to the selected map. File summaries only, without individual line/region hit maps. No workspace percentage, cross-platform or branch-coverage claim.'};
}

// execute/now injection is for Node custody/fault tests only. The CLI has no
// override for the process owner, time budget, profile directory or build flags.
export function collectRustCoverage(root, options, {execute = ownedSpawnSync, now = () => performance.now()} = {}) {
  root = realpathSync(root);
  const selection = coverageSelection(options); coverageEnvironment(process.env);
  check(typeof options.toolchain === 'string' && isAbsolute(options.toolchain), 'toolchain_path_required');
  const toolchain = realpathSync(options.toolchain);
  const paths = {rustc:contained(toolchain, join(toolchain, 'bin/rustc.exe')), cargo:contained(toolchain, join(toolchain, 'bin/cargo.exe')),
    profdata:contained(toolchain, join(toolchain, 'lib/rustlib', TARGET, 'bin/llvm-profdata.exe')),
    cov:contained(toolchain, join(toolchain, 'lib/rustlib', TARGET, 'bin/llvm-cov.exe'))};
  // Fail before admission/compilation if the installed component is missing.
  for (const path of Object.values(paths)) check(existsSync(path), 'tool_missing');
  const tools = Object.fromEntries(Object.entries(paths).map(([key,path]) => [key, regular(path, 256 << 20)]));
  const support = supportIdentity(), manifest = contained(root, join(root, 'crates', selection.packageName, 'Cargo.toml'));
  regular(manifest);
  // Encoded flags take precedence over Cargo target flags. Fail on a changed
  // project configuration rather than silently dropping a new required flag.
  const configuration = text(readFileSync(join(root, '.cargo/config.toml'))).trim();
  check(configuration === '[build]\ntarget-dir = "target"\n\n[target.x86_64-pc-windows-msvc]\nrustflags = ["-C", "target-feature=+crt-static"]', 'project_rustflags_changed');
  const job = beginScratch(root, 'rust-coverage', {command:['rust-coverage', selection], inputs:{selection, tools, support},
    admissionOptions:{requiredBytes:256 << 20, scratchReserveBytes:128 << 20}});
  const rawDirectory = join(job.directory, 'raw'); mkdirSync(rawDirectory);
  const profilePattern = join(rawDirectory, 'test-%m-%p.profraw');
  const reportPath = join(job.evidence, 'coverage.json'), logs = [], steps = [];
  const report = {schema:'lycheedev.rust-coverage.v1', complete:false, selection, target:TARGET, rust:RUST, flags:FLAGS,
    toolchain, tools, support, steps, profilePattern, coverageEvidence:job.receipt, totalProcessBudgetMs:600000};
  let handlesReaped = true, succeeded = false, failure, build, rawArchived = false;
  function archiveRaw() {
    const names = readdirSync(rawDirectory).sort();
    check(names.length <= 128, 'profile_count_budget');
    let total = 0;
    const records = names.map(name => {
      check(/^test-[0-9_]+-[0-9]+\.profraw$/.test(name), 'profile_filename');
      const row = regular(contained(rawDirectory, join(rawDirectory, name)));
      total += row.bytes; check(total <= RAW_LIMIT, 'profile_total_budget'); return row;
    });
    const output = join(job.evidence, 'raw'); mkdirSync(output);
    report.profiles = records.map(row => {
      const path = join(output, basename(row.path)); copyFileSync(row.path, path); unchanged(row);
      const preserved = regular(path); check(preserved.sha256 === row.sha256, 'profile_copy'); logs.push(path); return preserved;
    });
    rawArchived = true;
  }
  const args = ['test', '--locked', '--offline', '-j', '1', '--target', TARGET, '-p', selection.packageName,
    ...(selection.kind === 'lib' ? ['--lib'] : ['--test', selection.testName]), '--no-run', '--message-format=json'];
  try {
    build = runCargo(root, args, {owner:'rust-coverage-build', oneShot:true,
      // Use the ordinary reviewed Cargo launcher while selecting only this
      // existing toolchain. Absolute paths are supported by rustup:
      // https://rust-lang.github.io/rustup/environment-variables.html
      environment:{RUSTUP_TOOLCHAIN:toolchain, RUSTUP_AUTO_INSTALL:'0',
        RUSTC:paths.rustc, RUSTC_WRAPPER:'', RUSTC_WORKSPACE_WRAPPER:'', CARGO_BUILD_BUILD_DIR:join(root, 'target'),
        CARGO_ENCODED_RUSTFLAGS:FLAGS.join('\x1f'), LLVM_PROFILE_FILE:profilePattern},
      execute(cargoCommand, cargoArgs, ownerOptions) {
        const deadline = now() + ownerOptions.timeout;
        check(ownerOptions.env.CARGO_BUILD_BUILD_DIR === ownerOptions.env.CARGO_TARGET_DIR, 'build_directory');
        report.buildEvidence = join(ownerOptions.evidence, 'receipt.json');
        report.buildDirectory = ownerOptions.env.CARGO_TARGET_DIR;
        let cargoOutput = Buffer.alloc(0), cargoError = Buffer.alloc(0), cargoStatus = null, cargoSignal = null, processFailure;
        function run(name, command, argv, {cargoBuild = false, environment = ownerOptions.env, cwd = root} = {}) {
          for (const row of Object.values(tools)) unchanged(row);
          const remaining = Math.floor(deadline - now()); check(remaining > 0, 'deadline');
          handlesReaped = false;
          const result = execute(command, argv, {...ownerOptions, cwd, env:environment, evidence:job.evidence, cargoBuild,
            timeout:Math.min(remaining, 600000), maxBuffer:FILE_LIMIT});
          handlesReaped = result.handlesReaped === true;
          logs.push(...(result.logs || []));
          const stdout = Buffer.from(result.stdout || ''), stderr = Buffer.from(result.stderr || '');
          if (name === 'cargo') { cargoOutput = stdout; cargoError = stderr; cargoStatus = result.status; cargoSignal = result.signal; }
          check(stdout.length <= FILE_LIMIT && stderr.length <= FILE_LIMIT, 'output_budget');
          const prefix = join(job.evidence, `${steps.length}-${name}`), out = `${prefix}.stdout.bin`, err = `${prefix}.stderr.bin`;
          writeFileSync(out, stdout, {flag:'wx'}); writeFileSync(err, stderr, {flag:'wx'}); logs.push(out, err);
          steps.push({name, command, args:argv, cwd, timeoutMs:remaining, status:result.status, signal:result.signal,
            environmentSHA256:sha256(Buffer.from(JSON.stringify(Object.entries(environment).sort(([a],[b]) => a.localeCompare(b))))),
            handlesReaped, error:result.error?.message, stdout:out, stderr:err});
          check(handlesReaped, 'tree_reap_unverified');
          check(!result.error && result.status === 0 && !result.signal, `${name}_failed`);
          check(now() <= deadline, 'deadline');
          for (const row of Object.values(tools)) unchanged(row);
          return {stdout, stderr};
        }
        try {
          const rustVersion = text(run('rustc-version', paths.rustc, ['--version', '--verbose']).stdout);
          check(/^release: 1\.99\.0$/m.test(rustVersion) && /^commit-hash: [a-f0-9]{40}$/m.test(rustVersion)
            && new RegExp(`^host: ${TARGET}$`, 'm').test(rustVersion), 'compiler_identity');
          const llvm = /^LLVM version: ([0-9]+\.[0-9]+\.[0-9]+)$/m.exec(rustVersion)?.[1]; check(llvm, 'compiler_llvm_identity');
          const sysroot = text(run('rustc-sysroot', paths.rustc, ['--print', 'sysroot']).stdout).trim();
          check(realpathSync(sysroot) === toolchain, 'compiler_sysroot');
          const cargoVersion = text(run('cargo-version', paths.cargo, ['--version', '--verbose']).stdout);
          check(/^cargo 1\.99\.0 \(/m.test(cargoVersion), 'cargo_identity');
          const cargoLaunchVersion = text(run('cargo-launch-version', cargoCommand, ['--version', '--verbose']).stdout);
          check(cargoLaunchVersion === cargoVersion, 'cargo_launch_identity');
          const llvmTools = {};
          for (const key of ['profdata', 'cov']) {
            const version = text(run(`${key}-version`, paths[key], ['--version']).stdout);
            check(new RegExp(`^\\s*LLVM version ${llvm.replaceAll('.', '\\.')}(?:-rust-1\\.99\\.0-stable)?\\s*$`, 'm').test(version), 'llvm_version_mismatch');
            llvmTools[key] = version;
          }
          report.compiler = {verbose:rustVersion, cargoVerbose:cargoVersion, cargoLaunchVerbose:cargoLaunchVersion, llvm, llvmTools};
          const compiled = run('cargo', cargoCommand, cargoArgs, {cargoBuild:true});
          cargoOutput = compiled.stdout; cargoError = compiled.stderr;
          const artifact = selectCoverageArtifact(cargoOutput, root, ownerOptions.env.CARGO_TARGET_DIR, selection);
          const binary = join(job.evidence, 'instrumented-test.exe'); copyFileSync(artifact.path, binary); unchanged(artifact);
          check(regular(binary, 128 << 20).sha256 === artifact.sha256, 'artifact_copy');
          report.artifact = {...artifact, preservedBinary:binary};
          check(readdirSync(rawDirectory).length === 0, 'profiles_before_tests');
          const tested = run('tests', artifact.path, ['--test-threads=1', '--format=pretty', '--color=never',
            ...(selection.exact ? [selection.exact, '--exact'] : [])], {cwd:dirname(manifest)});
          report.tests = verifyCoverageTests(tested.stdout, selection);
          unchanged(artifact); unchanged({path:binary, bytes:artifact.bytes, sha256:artifact.sha256});
          archiveRaw(); check(report.profiles.length > 0, 'profiles_missing');
          const merged = join(job.evidence, 'coverage.profdata');
          run('merge', paths.profdata, ['merge', '-sparse', '--failure-mode=any', '-j', '1',
            ...report.profiles.map(row => row.path), '-o', merged]);
          report.merged = regular(merged); logs.push(merged);
          const exported = run('export', paths.cov, ['export', '--format=text', '--summary-only', '-j', '1',
            `--instr-profile=${merged}`, '--object', binary]);
          check(exported.stderr.length === 0, 'export_diagnostics');
          report.coverage = summarizeCoverage(exported.stdout, root);
          for (const row of report.profiles) unchanged(row); unchanged(report.merged); unchanged(artifact);
          unchanged({path:binary, bytes:artifact.bytes, sha256:artifact.sha256});
          assert.deepEqual(supportIdentity(), support, 'coverage.support_changed');
          check(now() <= deadline, 'deadline');
        } catch (error) { processFailure = error; }
        return {status:cargoStatus, signal:cargoSignal, error:processFailure, handlesReaped,
          stdout:cargoOutput, stderr:cargoError, logs:[...logs]};
      },
    });
    report.build = {buildEvidence:build.buildEvidence, target:build.target, inputHashes:build.inputHashes,
      environmentSHA256:build.environmentSHA256}; report.complete = true; succeeded = true;
  } catch (error) { failure = error; report.error = error.message; report.buildEvidence ??= error.buildEvidence; }
  finally {
    if (handlesReaped && !rawArchived) {
      try { archiveRaw(); } catch (error) { report.profileArchiveError = error.message; }
    }
    report.handlesReaped = handlesReaped; report.rawArchived = rawArchived;
    save(reportPath, report); logs.push(reportPath);
    job.seal({state:handlesReaped ? (succeeded ? 'succeeded' : 'failed') : 'interrupted', handlesReaped, logs:[...new Set(logs)]});
    if (handlesReaped && rawArchived) job.cleanup();
  }
  if (failure) throw Object.assign(failure, {coverageEvidence:job.receipt, coverageReport:reportPath});
  return {report:reportPath, coverageEvidence:job.receipt, buildEvidence:build.buildEvidence, selection, coverage:report.coverage};
}

export function coverageArguments(argv) {
  const options = {};
  for (let i = 0; i < argv.length; i++) {
    const key = {'--toolchain':'toolchain', '--package':'packageName', '--test':'testName', '--exact':'exact'}[argv[i]];
    if (argv[i] === '--lib') { check(options.lib === undefined, 'duplicate_option'); options.lib = true; }
    else { check(key && options[key] === undefined && argv[i+1] && !argv[i+1].startsWith('--'), 'invalid_option'); options[key] = argv[++i]; }
  }
  coverageSelection(options); return options;
}
if (process.argv[1] && resolve(process.argv[1]) === resolve(import.meta.filename)) {
  try {
    check(process.platform === 'win32' && process.arch === 'x64', 'windows_x64_required');
    console.log(JSON.stringify(collectRustCoverage(process.cwd(), coverageArguments(process.argv.slice(2))), null, 2));
  } catch (error) { console.error(JSON.stringify({error:error.message, coverageEvidence:error.coverageEvidence, coverageReport:error.coverageReport})); process.exitCode = 1; }
}
