// Selected Windows test-binary coverage, not a workspace acceptance percentage.
// Rust's supported instrument-coverage workflow and Cargo JSON artifact contract:
// https://doc.rust-lang.org/rustc/instrument-coverage.html
// https://doc.rust-lang.org/cargo/reference/external-tools.html
// https://llvm.org/docs/CommandGuide/llvm-cov.html
// https://llvm.org/docs/CommandGuide/llvm-profdata.html
import assert from 'node:assert/strict';
import {closeSync, existsSync, fstatSync, fsyncSync, lstatSync, mkdirSync, openSync, readFileSync, readSync, readdirSync, realpathSync, writeFileSync, writeSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {basename, dirname, isAbsolute, join, relative, resolve, sep} from 'node:path';
import {performance} from 'node:perf_hooks';
import {beginScratch, contained, sha256, usage} from './rust-disk.mjs';
import {hashFile, runCargo} from './rust-build.mjs';
import {ownedSpawnSync} from './rust-owned-process.mjs';

const TARGET = 'x86_64-pc-windows-msvc', RUST = '1.99.0';
const FILE_LIMIT = 16 << 20, RAW_LIMIT = 128 << 20, EVIDENCE_LIMIT = 256 << 20;
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
function sameFile(before, after) {
  for (const key of ['dev', 'ino', 'size', 'mtimeMs', 'ctimeMs']) check(before[key] === after[key], 'file_changed');
}
const partName = index => `part-${String(index).padStart(3, '0')}.bin`;

// Same ordered, bounded-part custody as source-index evidence. Only the complete
// source files are LLVM inputs; a byte fragment is never an independent profile.
export function verifyCoverageArchive(archive) {
  check(archive.schema === 'lycheedev.coverage-file-parts.v1' && archive.partBytes === FILE_LIMIT
    && Number.isSafeInteger(archive.bytes) && archive.bytes > 0 && archive.bytes <= RAW_LIMIT
    && /^[a-f0-9]{64}$/.test(archive.sha256) && Array.isArray(archive.parts)
    && archive.parts.length === Math.ceil(archive.bytes / FILE_LIMIT), 'archive_schema');
  const whole = createHash('sha256'), buffer = Buffer.alloc(65536); let total = 0;
  for (const [index, part] of archive.parts.entries()) {
    const path = contained(archive.directory, join(archive.directory, partName(index)));
    check(part.path === path && part.offset === total && part.bytes === Math.min(FILE_LIMIT, archive.bytes - total), 'archive_parts');
    const before = lstatSync(path); check(before.isFile() && !before.isSymbolicLink() && before.size === part.bytes, 'archive_part_size');
    const input = openSync(path, 'r'), one = createHash('sha256'); let count = 0;
    try {
      sameFile(before, fstatSync(input));
      for (;;) {
        const n = readSync(input, buffer, 0, buffer.length, null); if (!n) break;
        count += n; check(count <= part.bytes, 'archive_part_size'); one.update(buffer.subarray(0, n)); whole.update(buffer.subarray(0, n));
      }
      check(count === part.bytes, 'archive_part_size'); sameFile(before, fstatSync(input));
    } finally { closeSync(input); }
    sameFile(before, lstatSync(path)); check(one.digest('hex') === part.sha256, 'archive_part_hash'); total += count;
  }
  check(total === archive.bytes && whole.digest('hex') === archive.sha256, 'archive_whole_hash');
  return {bytes:total, sha256:archive.sha256};
}

// read/write injection is confined to Node copy-fault tests, never CLI options.
export function archiveCoverageFile(source, directory, logs, {read = readSync, write = writeSync} = {}) {
  check(Number.isSafeInteger(source.bytes) && source.bytes > 0 && source.bytes <= RAW_LIMIT, 'file_budget');
  unchanged(source); const before = lstatSync(source.path);
  mkdirSync(directory);
  const archive = {schema:'lycheedev.coverage-file-parts.v1', path:source.path, bytes:source.bytes, sha256:source.sha256,
    directory, partBytes:FILE_LIMIT, parts:[]};
  const whole = createHash('sha256'), buffer = Buffer.alloc(65536), input = openSync(source.path, 'r'); let total = 0;
  try {
    sameFile(before, fstatSync(input));
    while (total < source.bytes) {
      const path = contained(directory, join(directory, partName(archive.parts.length))), length = Math.min(FILE_LIMIT, source.bytes - total);
      const output = openSync(path, 'wx'), one = createHash('sha256'); let count = 0;
      logs.push(path); // Even a failed/partial copy remains bounded sealed evidence.
      try {
        while (count < length) {
          const wanted = Math.min(buffer.length, length - count), n = read(input, buffer, 0, wanted, null);
          check(n > 0 && n <= wanted, 'archive_source_truncated'); const chunk = buffer.subarray(0, n);
          let written = 0;
          while (written < n) { const m = write(output, chunk, written, n - written); check(m > 0 && m <= n - written, 'archive_short_write'); written += m; }
          one.update(chunk); whole.update(chunk); count += n;
        }
        fsyncSync(output);
      } finally { closeSync(output); }
      archive.parts.push({path, offset:total, bytes:count, sha256:one.digest('hex')}); total += count;
    }
    check(read(input, buffer, 0, 1, null) === 0, 'archive_source_grew'); sameFile(before, fstatSync(input));
  } finally { closeSync(input); }
  sameFile(before, lstatSync(source.path)); check(whole.digest('hex') === source.sha256, 'archive_source_changed');
  verifyCoverageArchive(archive); unchanged(source); return archive;
}

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
  const object = value => value !== null && typeof value === 'object' && !Array.isArray(value);
  // Reviewed summary-only contracts, not a promise to accept future 2.x/3.x:
  // llvmorg-20.1.0 (2.0.1) and llvmorg-23.1.1 (3.1.0), renderSummary/File/Root:
  // https://github.com/llvm/llvm-project/blob/llvmorg-23.1.1/llvm/tools/llvm-cov/CoverageExporterJson.cpp
  // The consumed count/covered fields are unchanged; detailed MC/DC records
  // are omitted by --summary-only and do not expand this report's scope.
  check(object(exported) && exported.type === 'llvm.coverage.json.export' && ['2.0.1', '3.1.0'].includes(exported.version)
    && Array.isArray(exported.data) && exported.data.length === 1 && object(exported.data[0])
    && Array.isArray(exported.data[0].files), 'export_schema');
  const ownedRoot = join(root, 'crates'), files = [], excluded = [], seen = new Set();
  check(exported.data[0].files.length <= 16384, 'export_file_budget');
  for (const file of exported.data[0].files) {
    check(object(file), 'export_schema');
    check(typeof file.filename === 'string' && isAbsolute(file.filename), 'export_filename');
    const path = resolve(file.filename), rel = relative(ownedRoot, path);
    check(!seen.has(path), 'duplicate_export_file'); seen.add(path);
    if (!rel || rel === '..' || rel.startsWith(`..${sep}`) || isAbsolute(rel) || !path.endsWith('.rs')) { excluded.push(file.filename); continue; }
    contained(ownedRoot, path); regular(path);
    check(object(file.summary), 'export_metric');
    const metrics = {};
    for (const metric of ['functions', 'lines', 'regions']) {
      const row = file.summary[metric];
      check(object(row) && Number.isSafeInteger(row.count) && row.count >= 0 && Number.isSafeInteger(row.covered)
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
export function collectRustCoverage(root, options, {execute = ownedSpawnSync, now = () => performance.now(), archiveFile = archiveCoverageFile} = {}) {
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
  const reportPath = join(job.evidence, 'coverage.json'), custodyPath = join(job.evidence, 'custody.json');
  const mergedPath = join(job.directory, 'coverage.profdata'), logs = [], buildLogs = [], steps = [];
  const report = {schema:'lycheedev.rust-coverage.v2', complete:false, selection, target:TARGET, rust:RUST, flags:FLAGS,
    toolchain, tools, support, steps, profilePattern, coverageEvidence:job.receipt, totalProcessBudgetMs:600000,
    budgets:{rawBytes:RAW_LIMIT, scratchBytes:RAW_LIMIT, evidenceBytes:EVIDENCE_LIMIT, evidenceMetadataReserveBytes:FILE_LIMIT,
      scope:'Observed raw+merged scratch and both coverage/build evidence directories; not an OS write quota. Binary/raw/merged bytes are preserved once as ordered parts; the build owner seals their custody index, not duplicate payloads.'}};
  let handlesReaped = true, succeeded = false, failure, build, rawArchived = false, rawAttempted = false, mergedArchived = false, mergedAttempted = false;
  let pendingBuildOutputBytes = 0;
  function evidenceBytes() {
    return usage(job.evidence).bytes + (report.buildEvidence ? usage(dirname(report.buildEvidence)).bytes : 0);
  }
  function evidenceBudget(extra = 0) {
    // Until runCargo seals, account for the runtime logs it will copy. Payload
    // parts stay solely with this owner; a small index binds them to the build.
    const copies = build ? 0 : pendingBuildOutputBytes + buildLogs.reduce((sum, path) => sum + lstatSync(path).size, 0);
    check(evidenceBytes() + copies + extra + FILE_LIMIT <= EVIDENCE_LIMIT, 'evidence_total_budget');
  }
  function preserve(row, name) {
    evidenceBudget(row.bytes);
    return archiveFile(row, join(job.evidence, name), logs);
  }
  function archiveRaw() {
    rawAttempted = true;
    const names = readdirSync(rawDirectory).sort();
    check(names.length <= 128, 'profile_count_budget');
    let total = 0;
    const records = names.map(name => {
      check(/^test-[0-9_]+-[0-9]+\.profraw$/.test(name), 'profile_filename');
      const row = regular(contained(rawDirectory, join(rawDirectory, name)), RAW_LIMIT);
      total += row.bytes; check(total <= RAW_LIMIT, 'profile_total_budget'); return row;
    });
    report.profiles = [];
    for (const row of records) report.profiles.push(preserve(row, `${basename(row.path)}.parts`));
    rawArchived = true;
  }
  function archiveMerged() {
    mergedAttempted = true;
    check(usage(job.directory).bytes <= RAW_LIMIT, 'scratch_total_budget');
    report.merged = preserve(regular(mergedPath, RAW_LIMIT), 'coverage.profdata.parts'); mergedArchived = true;
  }
  function finishArchives() {
    if (!handlesReaped) return;
    if (!rawAttempted) { try { archiveRaw(); } catch (error) { report.profileArchiveError = error.message; } }
    if (existsSync(mergedPath) && !mergedAttempted) { try { archiveMerged(); } catch (error) { report.mergedArchiveError = error.message; } }
    // A failed LLVM child can still change an input/output. Never discard its
    // current scratch bytes just because an earlier copy was authenticated.
    if (rawArchived) { try {
      assert.deepEqual(readdirSync(rawDirectory).sort(), report.profiles.map(row => basename(row.path)).sort(), 'coverage.profile_set_changed');
      for (const row of report.profiles) { unchanged(row); verifyCoverageArchive(row); } }
      catch (error) { rawArchived = false; report.profileArchiveError = error.message; } }
    if (mergedArchived) { try { unchanged(report.merged); verifyCoverageArchive(report.merged); }
      catch (error) { mergedArchived = false; report.mergedArchiveError = error.message; } }
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
          evidenceBudget();
          const remaining = Math.floor(deadline - now()); check(remaining > 0, 'deadline');
          handlesReaped = false;
          const result = execute(command, argv, {...ownerOptions, cwd, env:environment, evidence:job.evidence, cargoBuild,
            timeout:Math.min(remaining, 600000), maxBuffer:FILE_LIMIT});
          handlesReaped = result.handlesReaped === true;
          const processLogs = (result.logs || []).map(path => contained(job.evidence, path));
          logs.push(...processLogs); buildLogs.push(...processLogs);
          const stdout = Buffer.from(result.stdout || ''), stderr = Buffer.from(result.stderr || '');
          if (name === 'cargo') {
            cargoOutput = stdout; cargoError = stderr; cargoStatus = result.status; cargoSignal = result.signal;
            pendingBuildOutputBytes = stdout.length + stderr.length;
          }
          check(stdout.length <= FILE_LIMIT && stderr.length <= FILE_LIMIT, 'output_budget');
          const prefix = join(job.evidence, `${steps.length}-${name}`), out = `${prefix}.stdout.bin`, err = `${prefix}.stderr.bin`;
          writeFileSync(out, stdout, {flag:'wx'}); writeFileSync(err, stderr, {flag:'wx'}); logs.push(out, err);
          buildLogs.push(out, err);
          steps.push({name, command, args:argv, cwd, timeoutMs:remaining, status:result.status, signal:result.signal,
            environmentSHA256:sha256(Buffer.from(JSON.stringify(Object.entries(environment).sort(([a],[b]) => a.localeCompare(b))))),
            handlesReaped, error:result.error?.message, stdout:out, stderr:err});
          check(handlesReaped, 'tree_reap_unverified');
          check(!result.error && result.status === 0 && !result.signal, `${name}_failed`);
          evidenceBudget();
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
          report.artifact = {path:artifact.path, bytes:artifact.bytes, sha256:artifact.sha256,
            archive:preserve(artifact, 'instrumented-test.exe.parts')};
          check(readdirSync(rawDirectory).length === 0, 'profiles_before_tests');
          const tested = run('tests', artifact.path, ['--test-threads=1', '--format=pretty', '--color=never',
            ...(selection.exact ? [selection.exact, '--exact'] : [])], {cwd:dirname(manifest)});
          report.tests = verifyCoverageTests(tested.stdout, selection);
          unchanged(artifact); verifyCoverageArchive(report.artifact.archive);
          archiveRaw(); check(report.profiles.length > 0, 'profiles_missing');
          check(usage(job.directory).bytes < RAW_LIMIT, 'scratch_total_budget');
          for (const row of report.profiles) { unchanged(row); verifyCoverageArchive(row); }
          run('merge', paths.profdata, ['merge', '-sparse', '--failure-mode=any', '-j', '1',
            ...report.profiles.map(row => row.path), '-o', mergedPath]);
          archiveMerged();
          const exported = run('export', paths.cov, ['export', '--format=text', '--summary-only', '-j', '1',
            `--instr-profile=${mergedPath}`, '--object', artifact.path]);
          check(exported.stderr.length === 0, 'export_diagnostics');
          report.coverage = summarizeCoverage(exported.stdout, root);
          for (const row of report.profiles) { unchanged(row); verifyCoverageArchive(row); }
          unchanged(report.merged); verifyCoverageArchive(report.merged); unchanged(artifact); verifyCoverageArchive(report.artifact.archive);
          assert.deepEqual(supportIdentity(), support, 'coverage.support_changed');
          check(now() <= deadline, 'deadline');
        } catch (error) { processFailure = error; }
        finishArchives();
        if (!processFailure && (!rawArchived || !mergedArchived)) processFailure = new Error(report.profileArchiveError || report.mergedArchiveError || 'coverage.archive_incomplete');
        // Bind archived payloads to this build without copying every large part
        // into a second evidence directory. The coverage owner seals each part.
        const custody = {schema:'lycheedev.coverage-custody.v1', artifact:report.artifact?.archive,
          profiles:report.profiles, merged:report.merged, rawArchived, mergedArchived,
          error:processFailure?.message, profileArchiveError:report.profileArchiveError, mergedArchiveError:report.mergedArchiveError};
        try {
          const bytes = Buffer.from(JSON.stringify(custody, null, 2) + '\n'); check(bytes.length <= FILE_LIMIT / 2, 'custody_budget');
          evidenceBudget(bytes.length * 2); writeFileSync(custodyPath, bytes, {flag:'wx'}); logs.push(custodyPath); buildLogs.push(custodyPath);
          check(now() <= deadline, 'deadline');
        } catch (error) { processFailure ??= error; report.custodyError = error.message; }
        return {status:cargoStatus, signal:cargoSignal, error:processFailure, handlesReaped,
          stdout:cargoOutput, stderr:cargoError, logs:[...new Set(buildLogs)]};
      },
    });
    report.build = {buildEvidence:build.buildEvidence, target:build.target, inputHashes:build.inputHashes,
      environmentSHA256:build.environmentSHA256}; report.complete = true; succeeded = true;
  } catch (error) { failure = error; report.error = error.message; report.buildEvidence ??= error.buildEvidence; }
  finally {
    finishArchives();
    if (succeeded && (!rawArchived || !mergedArchived)) {
      failure = new Error(report.profileArchiveError || report.mergedArchiveError || 'coverage.archive_incomplete');
      succeeded = false; report.complete = false; report.error = failure.message;
    }
    report.handlesReaped = handlesReaped; report.rawArchived = rawArchived; report.mergedArchived = mergedArchived;
    report.observedEvidenceBytesBeforeReport = evidenceBytes();
    // seal repeats each path in its log map and rows. Bound that final receipt
    // before allowing success, including JSON escaping/field/digest overhead.
    const uniqueLogs = [...new Set([...logs, reportPath])];
    report.reservedCoverageReceiptBytes = lstatSync(job.receipt).size
      + 4 * Buffer.byteLength(JSON.stringify(uniqueLogs)) + 1024 * (uniqueLogs.length + 1);
    // Keep the original failure even when a producer exceeded its observed
    // budget. Failure metadata is retained; no oversize owner is cleaned up.
    const reportBytes = Buffer.byteLength(JSON.stringify(report, null, 2) + '\n');
    if (report.observedEvidenceBytesBeforeReport + reportBytes + report.reservedCoverageReceiptBytes > EVIDENCE_LIMIT || reportBytes > FILE_LIMIT / 2) {
      const error = new Error('coverage.evidence_total_budget'); failure ??= error; succeeded = false; report.complete = false;
      report.error = failure.message; report.evidenceBudgetError = error.message;
      if (reportBytes > FILE_LIMIT / 2) {
        // Exact process outputs/parts remain on disk. Keep a bounded failure
        // record instead of masking the cause with disk.seal's per-log limit.
        report.omittedFromOversizeFailureReport = ['compiler', 'coverage', 'artifact', 'profiles', 'merged', 'tests', 'steps'];
        for (const key of report.omittedFromOversizeFailureReport) delete report[key];
      }
    }
    save(reportPath, report); logs.push(reportPath);
    job.seal({state:handlesReaped ? (succeeded ? 'succeeded' : 'failed') : 'interrupted', handlesReaped, logs:[...new Set(logs)]});
    if (handlesReaped && rawArchived && (!existsSync(mergedPath) || mergedArchived) && !report.evidenceBudgetError) job.cleanup();
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
