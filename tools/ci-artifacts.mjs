// CI build-artifact custody: seal the outputs of this run's build stage into a
// hashed manifest, and verify that manifest in every downstream job.
//
// The manifest binds the exact source tree, commit, toolchain and every byte the
// validate/release stages consume. Consumers must call `verify` with `--require`
// before using a path: a missing component, a changed byte, a stale source tree,
// a different toolchain or a workspace produced elsewhere fails with an explicit
// reason instead of any implicit rebuild. This is the mechanism behind DESIGN §8
// "跨 job 显式传递本轮构建产物".
import { createHash } from 'node:crypto';
import { existsSync, lstatSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { isAbsolute, dirname, join, relative, resolve, sep } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { sourceDigest, verifyArtifact } from './rust-build.mjs';
import { verifyAddonZip } from './addon-package.mjs';
import { verifyInterpreter } from './rust-formal-lua.mjs';

const repository = resolve(import.meta.dirname ?? fileURLToPath(new URL('.', import.meta.url)), '..');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
export const MANIFEST_SCHEMA = 'lycheedev.ci-build-artifacts.v1';
export const COMPONENT_KINDS = Object.freeze(['cli-release', 'test-programs', 'luac', 'lua51', 'addon-zip', 'skill', 'npm-package']);
const BUDGET = { files: 200000, bytes: 8 * 1024 ** 3, fileBytes: 1 * 1024 ** 3 };

function contained(root, path) {
  const absolute = resolve(root, path), remainder = relative(root, absolute);
  if (absolute !== root && (!absolute.startsWith(root + sep) || isAbsolute(remainder) || remainder.startsWith('..'))) {
    throw new Error(`ci-artifacts.outside_repository: ${path}`);
  }
  return absolute;
}

function gitHead(root) {
  const result = spawnSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8', windowsHide: true, shell: false, timeout: 60000 });
  if (result.error || result.status !== 0) throw new Error(`ci-artifacts.git_failed: ${result.error?.message ?? result.status}`);
  return result.stdout.trim();
}

function installedToolchain() {
  const rustc = spawnSync('rustc', ['-Vv'], { encoding: 'utf8', windowsHide: true, shell: false, timeout: 60000 });
  if (rustc.error || rustc.status !== 0) throw new Error(`ci-artifacts.rustc_failed: ${rustc.error?.message ?? rustc.status}`);
  const value = rustc.stdout.replaceAll('\r\n', '\n');
  const release = /^release:\s*(\S+)$/m.exec(value)?.[1];
  const host = /^host:\s*(\S+)$/m.exec(value)?.[1];
  const cargo = spawnSync('cargo', ['-V'], { encoding: 'utf8', windowsHide: true, shell: false, timeout: 60000 });
  if (cargo.error || cargo.status !== 0) throw new Error(`ci-artifacts.cargo_failed: ${cargo.error?.message ?? cargo.status}`);
  const cargoVersion = /^cargo\s+(\S+)/.exec(cargo.stdout.replaceAll('\r\n', '\n').trim())?.[1];
  if (!release || !host || !cargoVersion) throw new Error('ci-artifacts.unreadable_toolchain');
  return { rustc: release, cargo: cargoVersion, host, target: 'x86_64-pc-windows-msvc' };
}

function pinnedToolchain(root) {
  const pinned = JSON.parse(readFileSync(join(root, 'release/tools/rust.json'), 'utf8'));
  return { rustc: pinned.rustc, cargo: pinned.cargo, host: pinned.host, target: pinned.target };
}

function checkToolchain(root) {
  const installed = installedToolchain(), pinned = pinnedToolchain(root);
  for (const key of ['rustc', 'cargo', 'host', 'target']) {
    if (installed[key] !== pinned[key]) throw new Error(`ci-artifacts.toolchain_mismatch: ${key} ${installed[key]} != ${pinned[key]}`);
  }
  return installed;
}

function walk(root, directory) {
  const files = [];
  let bytes = 0;
  function visit(path) {
    const info = lstatSync(path);
    if (info.isSymbolicLink()) throw new Error(`ci-artifacts.reparse_point: ${relative(root, path)}`);
    if (info.isDirectory()) {
      for (const child of readdirSync(path).sort()) visit(join(path, child));
      return;
    }
    if (!info.isFile()) throw new Error(`ci-artifacts.non_regular_file: ${relative(root, path)}`);
    if (info.size > BUDGET.fileBytes) throw new Error(`ci-artifacts.file_budget: ${relative(root, path)}`);
    if (files.length + 1 > BUDGET.files) throw new Error('ci-artifacts.file_count_budget');
    bytes += info.size;
    if (bytes > BUDGET.bytes) throw new Error('ci-artifacts.byte_budget');
    const content = readFileSync(path);
    if (content.length !== info.size) throw new Error(`ci-artifacts.file_changed: ${relative(root, path)}`);
    files.push({ path: relative(root, path).replaceAll('\\', '/'), bytes: content.length, sha256: hash(content) });
  }
  visit(directory);
  return files;
}

function readJson(path) {
  return JSON.parse(readFileSync(path, 'utf8'));
}

function singleFile(directory, predicate, description) {
  const matches = readdirSync(directory).filter(predicate);
  if (matches.length !== 1) throw new Error(`ci-artifacts.expected_single_${description}: found ${matches.length}`);
  return join(directory, matches[0]);
}

function treeIdentity(root, directory) {
  const relativeDirectory = relative(root, directory);
  const prefix = relativeDirectory ? relativeDirectory.length + 1 : 0;
  const files = walk(root, directory).map(file => ({ path: prefix ? file.path.slice(prefix) : file.path, bytes: file.bytes, sha256: file.sha256 }));
  return { files: files.length, treeSHA256: hash(Buffer.from(JSON.stringify(files))) };
}

export function componentIdentity(root, kind, directory) {
  const dir = contained(root, directory);
  if (!existsSync(dir)) throw new Error(`ci-artifacts.component_missing: ${kind} ${directory}`);
  const sourceTree = sourceDigest(root);
  switch (kind) {
    case 'cli-release': {
      const record = verifyArtifact(root, join(dir, 'artifact.json'));
      if (record.profile !== 'release') throw new Error(`ci-artifacts.cli_profile: ${record.profile} != release`);
      if (record.inputHashes?.sourceTree !== sourceTree) throw new Error('ci-artifacts.cli_source_mismatch');
      return { dir: relative(root, dir).replaceAll('\\', '/'), artifactRecordSHA256: hash(readFileSync(join(dir, 'artifact.json'))),
        profile: record.profile, binarySHA256: record.binarySHA256, bytes: record.bytes, sourceTree: record.inputHashes.sourceTree };
    }
    case 'test-programs': {
      const recordPath = join(dir, 'test-programs.json');
      if (!existsSync(recordPath)) throw new Error('ci-artifacts.test_programs_record_missing');
      const record = readJson(recordPath);
      if (record.schema !== 'lycheedev.ci-test-programs.v1') throw new Error(`ci-artifacts.test_programs_schema: ${record.schema}`);
      if (resolve(record.root) !== root) throw new Error(`ci-artifacts.test_programs_workspace: ${record.root}`);
      if (record.commit !== gitHead(root)) throw new Error('ci-artifacts.test_programs_commit_mismatch');
      if (record.sourceTree !== sourceTree) throw new Error('ci-artifacts.test_programs_source_mismatch');
      return { dir: relative(root, dir).replaceAll('\\', '/'), recordSHA256: hash(readFileSync(recordPath)),
        sourceTree: record.sourceTree, programs: record.programs.length, support: record.support.length,
        programsSHA256: hash(Buffer.from(JSON.stringify(record.programs))) };
    }
    case 'luac': {
      const runtimePath = join(dir, 'runtime.json'), runtime = readJson(runtimePath);
      const pinned = readJson(join(root, 'release/tools/luac.json'));
      if (JSON.stringify(runtime) !== JSON.stringify(pinned.runtime)) throw new Error('ci-artifacts.luac_identity_mismatch');
      const executable = join(dir, 'luac.exe');
      if (!existsSync(executable)) throw new Error('ci-artifacts.luac_missing');
      const bytes = readFileSync(executable);
      if (bytes.length !== runtime.bytes || hash(bytes) !== runtime.sha256) throw new Error('ci-artifacts.luac_integrity');
      return { dir: relative(root, dir).replaceAll('\\', '/'), runtimeSHA256: hash(readFileSync(runtimePath)),
        executableSHA256: runtime.sha256, bytes: bytes.length, version: runtime.version, platform: runtime.platform };
    }
    case 'lua51': {
      const reportPath = join(dir, 'build-report.json'), admitted = verifyInterpreter(reportPath), report = admitted.report;
      const executable = join(dir, 'lua.exe');
      if (!existsSync(executable)) throw new Error('ci-artifacts.lua51_missing');
      const bytes = readFileSync(executable);
      if (hash(bytes) !== report.executableSHA256) throw new Error('ci-artifacts.lua51_integrity');
      if (report.schema !== 'lycheedev.formal-lua-interpreter.v1' || report.version !== '5.1.5' || report.platform !== 'windows-amd64') throw new Error(`ci-artifacts.lua51_version: ${report.version}`);
      return { dir: relative(root, dir).replaceAll('\\', '/'), reportSHA256: admitted.reportSHA256,
        buildReceiptSHA256: admitted.buildReceiptSHA256, supplyManifestSHA256: admitted.supplyManifestSHA256 ?? null,
        executableSHA256: report.executableSHA256, bytes: bytes.length, version: '5.1.5' };
    }
    case 'addon-zip': {
      const zip = singleFile(dir, name => name.endsWith('.zip'), 'addon_zip');
      const verification = verifyAddonZip(zip, { repoRoot: root });
      if (!verification.ok) throw new Error(`ci-artifacts.addon_zip_invalid: ${verification.violations.slice(0, 5).join('; ')}`);
      return { dir: relative(root, dir).replaceAll('\\', '/'), zip: relative(root, zip).replaceAll('\\', '/'),
        zipSHA256: hash(readFileSync(zip)), bytes: lstatSync(zip).size, files: verification.files.length };
    }
    case 'skill': {
      const staged = contained(root, join(dir, 'staged')), receiptPath = contained(root, join(dir, 'staged.contract.json'));
      if (!existsSync(staged) || !existsSync(receiptPath)) throw new Error('ci-artifacts.skill_missing');
      const receipt = readJson(receiptPath);
      if (receipt.schema !== 'lycheedev.skill-declared-contract.v1') throw new Error(`ci-artifacts.skill_receipt_schema: ${receipt.schema}`);
      if (receipt.missingCommands.length || receipt.missingFlags.length) throw new Error('ci-artifacts.skill_declaration_incomplete');
      const source = treeIdentity(root, join(root, 'skills/lycheedev'));
      const stagedIdentity = treeIdentity(root, staged);
      if (JSON.stringify(source) !== JSON.stringify(stagedIdentity)) throw new Error('ci-artifacts.skill_tree_differs_from_source');
      return { dir: relative(root, dir).replaceAll('\\', '/'), staged: relative(root, staged).replaceAll('\\', '/'),
        receiptSHA256: hash(readFileSync(receiptPath)), treeSHA256: stagedIdentity.treeSHA256, files: stagedIdentity.files };
    }
    case 'npm-package': {
      const reportPath = join(dir, 'report.json'), report = readJson(reportPath);
      if (report.schema !== 'lycheedev.rust-npm-package.v1') throw new Error(`ci-artifacts.npm_report_schema: ${report.schema}`);
      if (report.state !== 'packed' || report.tgzPathBase !== 'report') throw new Error('ci-artifacts.npm_package_not_packed');
      if (report.profile !== 'release') throw new Error(`ci-artifacts.npm_profile: ${report.profile}`);
      if (report.commit !== gitHead(root)) throw new Error('ci-artifacts.npm_commit_mismatch');
      if (report.sourceTreeSHA256 !== sourceTree) throw new Error('ci-artifacts.npm_source_mismatch');
      if (typeof report.tgz !== 'string' || isAbsolute(report.tgz)) throw new Error('ci-artifacts.npm_tgz_path');
      const tgz = contained(dir, report.tgz), tgzBytes = readFileSync(tgz);
      if (hash(tgzBytes) !== report.tgzSHA256) throw new Error('ci-artifacts.npm_tgz_changed');
      if (!existsSync(join(dir, 'package', 'release.json'))) throw new Error('ci-artifacts.npm_package_missing');
      return { dir: relative(root, dir).replaceAll('\\', '/'), reportSHA256: hash(readFileSync(reportPath)),
        binarySHA256: report.binarySHA256, sourceTree: report.sourceTreeSHA256, state: report.state,
        tgz: relative(root, tgz).replaceAll('\\', '/'), tgzSHA256: report.tgzSHA256, tgzBytes: tgzBytes.length };
    }
    default: throw new Error(`ci-artifacts.unknown_component: ${kind}`);
  }
}

function crossCheck(manifest) {
  const components = manifest.components;
  if (components['cli-release'] && components['npm-package']) {
    if (components['cli-release'].binarySHA256 !== components['npm-package'].binarySHA256) throw new Error('ci-artifacts.npm_binary_differs_from_cli');
  }
  for (const component of Object.values(components)) {
    if (component.sourceTree && component.sourceTree !== manifest.sourceTree) throw new Error('ci-artifacts.component_source_mismatch');
  }
}

export function sealArtifacts({ root = repository, output, components } = {}) {
  root = resolve(root);
  if (!output) throw new Error('ci-artifacts.output_required');
  if (!components || !components.length) throw new Error('ci-artifacts.components_required');
  const manifestPath = contained(root, output);
  if (existsSync(manifestPath)) throw new Error(`ci-artifacts.manifest_exists: ${output}`);
  const toolchain = checkToolchain(root);
  const manifest = {
    schema: MANIFEST_SCHEMA, commit: gitHead(root), sourceTree: sourceDigest(root), workspace: root,
    platform: `${process.platform}/${process.arch}`, node: process.versions.node, toolchain, components: {}, files: [],
  };
  const seen = new Set();
  for (const { kind, directory } of components) {
    if (!COMPONENT_KINDS.includes(kind)) throw new Error(`ci-artifacts.unknown_component: ${kind}`);
    if (seen.has(kind)) throw new Error(`ci-artifacts.duplicate_component: ${kind}`);
    seen.add(kind);
    const dir = contained(root, directory);
    manifest.components[kind] = componentIdentity(root, kind, directory);
    for (const file of walk(root, dir)) {
      if (manifest.files.some(row => row.path === file.path)) continue;
      manifest.files.push(file);
    }
  }
  manifest.files.sort((a, b) => (a.path < b.path ? -1 : a.path > b.path ? 1 : 0));
  crossCheck(manifest);
  mkdirSync(dirname(manifestPath), { recursive: true });
  writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`, { flag: 'wx' });
  return { manifest: relative(root, manifestPath).replaceAll('\\', '/'), sourceTree: manifest.sourceTree, commit: manifest.commit, components: Object.keys(manifest.components), files: manifest.files.length };
}

export function verifyArtifacts({ root = repository, manifestPath, require: required = [] } = {}) {
  root = resolve(root);
  const path = contained(root, manifestPath);
  if (!existsSync(path)) throw new Error(`ci-artifacts.manifest_missing: ${manifestPath}`);
  const manifest = readJson(path);
  if (manifest.schema !== MANIFEST_SCHEMA) throw new Error(`ci-artifacts.unknown_manifest: ${manifest.schema}`);
  if (resolve(manifest.workspace) !== root) throw new Error(`ci-artifacts.workspace_mismatch: built at ${manifest.workspace}`);
  if (manifest.commit !== gitHead(root)) throw new Error('ci-artifacts.commit_mismatch');
  const currentTree = sourceDigest(root);
  if (manifest.sourceTree !== currentTree) throw new Error('ci-artifacts.source_tree_mismatch: the checkout differs from the build-stage inputs');
  const toolchain = checkToolchain(root);
  if (JSON.stringify(toolchain) !== JSON.stringify(manifest.toolchain)) throw new Error('ci-artifacts.recorded_toolchain_mismatch');
  for (const [kind, recorded] of Object.entries(manifest.components)) {
    const identity = componentIdentity(root, kind, recorded.dir);
    if (JSON.stringify(identity) !== JSON.stringify(recorded)) throw new Error(`ci-artifacts.component_changed: ${kind}`);
    // A build job may seal its manifest inside the component directory (the
    // test-programs artifact keeps the manifest next to the executables). The
    // manifest is written after the component walk, so it is not part of the
    // recorded file set and must not count as an unexpected extra file here.
    const actual = walk(root, join(root, recorded.dir))
      .filter(file => resolve(root, file.path) !== path)
      .sort((a, b) => (a.path < b.path ? -1 : a.path > b.path ? 1 : 0));
    const recordedFiles = manifest.files.filter(file => file.path.startsWith(recorded.dir + '/'));
    if (JSON.stringify(actual) !== JSON.stringify(recordedFiles)) throw new Error(`ci-artifacts.component_files_changed: ${kind}`);
  }
  for (const kind of required) {
    if (!manifest.components[kind]) throw new Error(`ci-artifacts.required_component_missing: ${kind}`);
  }
  crossCheck(manifest);
  return { ok: true, manifest: relative(root, path).replaceAll('\\', '/'), sourceTree: manifest.sourceTree, commit: manifest.commit,
    components: Object.keys(manifest.components), required: [...required], files: manifest.files.length };
}

function parseComponents(argv) {
  const parsed = { components: [], out: undefined, manifestPath: undefined, required: [] };
  for (let index = 0; index < argv.length; index++) {
    const arg = argv[index], value = argv[index + 1];
    if (arg === '--component') {
      const match = /^([a-z0-9-]+)=(.+)$/.exec(value ?? '');
      if (!match) throw new Error(`ci-artifacts.invalid_component: ${value}`);
      parsed.components.push({ kind: match[1], directory: match[2] });
      index++;
    } else if (arg === '--out') { parsed.out = value; index++; }
    else if (arg === '--manifest') { parsed.manifestPath = value; index++; }
    else if (arg === '--require') { parsed.required = value.split(',').map(item => item.trim()).filter(Boolean); index++; }
    else throw new Error(`ci-artifacts.unknown_option: ${arg}`);
  }
  return parsed;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const [command, ...argv] = process.argv.slice(2);
    const options = parseComponents(argv);
    if (command === 'seal') {
      if (!options.out) throw new Error('ci-artifacts.out_required');
      const result = sealArtifacts({ output: options.out, components: options.components });
      process.stdout.write(`${JSON.stringify(result, null, 2)}\n`);
    } else if (command === 'verify') {
      if (!options.manifestPath) throw new Error('ci-artifacts.manifest_required');
      const result = verifyArtifacts({ manifestPath: options.manifestPath, require: options.required });
      process.stdout.write(`${JSON.stringify(result, null, 2)}\n`);
    } else throw new Error('usage: node tools/ci-artifacts.mjs seal --out <manifest.json> --component <kind>=<dir> [...] | verify --manifest <manifest.json> [--require kind,...]');
  } catch (error) { process.stderr.write(`${error.message}\n`); process.exitCode = 1; }
}
