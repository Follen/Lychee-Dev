// Current addon Lua regression entry. Enumerates every tests/addon/t_*.lua suite
// and runs it under every supported client profile with the managed Lua 5.1
// interpreter prepared by tools/rust-formal-lua.mjs. This is an execution
// validator, not an acceptance platform: it never builds, downloads, or
// substitutes an interpreter, and it does not replace real-client validation.
//
// usage:
//   node tests/addon/run-all.mjs <lua-build-report.json> [options]
// options:
//   --file <t_file.lua>    run only one suite (repeatable)
//   --client <profile>     run only one client profile (repeatable)
//   --addon-root <path>    explicitly override the addon root (default: <repo>/addon)
//
// The Lua executable and its hash come only from the explicit build report so a
// host `lua` on PATH can never silently satisfy the suite.
import { createHash } from 'node:crypto';
import { existsSync,readdirSync,readFileSync,statSync } from 'node:fs';
import {dirname, join, resolve, sep} from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import { verifyInterpreter as verifyManagedInterpreter } from '../../tools/rust-formal-lua.mjs';

export const CLIENTS = Object.freeze(['retail', 'classic', 'titan', 'forever', 'classic-era', 'anniversary']);
const repo = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..');
const suiteDirectory = join(repo, 'tests/addon');
const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');

export function parseArguments(argv) {
  let reportPath = null;
  const files = [], clients = [];
  let addonRoot = join(repo, 'addon');
  for (let index = 0; index < argv.length; index++) {
    const argument = argv[index];
    if (argument === '--file' || argument === '--client' || argument === '--addon-root') {
      const value = argv[++index];
      if (!value || value.startsWith('--')) throw new Error(`addon-lua.option_value_missing: ${argument}`);
      if (argument === '--file') files.push(value);
      else if (argument === '--client') clients.push(value);
      else addonRoot = resolve(repo, value);
      continue;
    }
    if (argument.startsWith('--')) throw new Error(`addon-lua.unknown_option: ${argument}`);
    if (reportPath !== null) throw new Error('addon-lua.unexpected_positional');
    reportPath = argument;
  }
  if (!reportPath) throw new Error('usage: node tests/addon/run-all.mjs <lua-build-report.json> [--file t_x.lua] [--client profile] [--addon-root path]');
  if (files.some(value => !/^t_[a-z0-9_]+\.lua$/.test(value))) throw new Error('addon-lua.invalid_suite_name');
  if (clients.some(value => !CLIENTS.includes(value))) throw new Error(`addon-lua.unknown_client: ${clients.find(value => !CLIENTS.includes(value))}`);
  if (!existsSync(addonRoot)) throw new Error(`addon-lua.addon_root_missing: ${addonRoot}`);
  return { reportPath: resolve(repo, reportPath), files, clients, addonRoot };
}

function regular(path, cap) {
  if (!existsSync(path)) throw new Error(`addon-lua.file_missing: ${path}`);
  const metadata = statSync(path);
  if (!metadata.isFile() || metadata.isSymbolicLink() || metadata.size > cap) throw new Error(`addon-lua.file_shape: ${path}`);
  return readFileSync(path);
}

// The interpreter is an explicit build-stage product. No PATH lookup and no
// version guessing: the sealed report names one executable and its exact bytes.
export function verifyInterpreter(reportPath, run = spawnSync) {
  const admitted = verifyManagedInterpreter(reportPath), {report} = admitted;
  const executable = resolve(report.executable);
  const probe = run(executable, ['-v'], { encoding: 'utf8', shell: false, windowsHide: true, timeout: 30000, env: cleanEnvironment() });
  if (probe.error || probe.status !== 0) throw new Error(`addon-lua.interpreter_probe: ${probe.error?.message ?? probe.status}`);
  const output = `${probe.stdout ?? ''}${probe.stderr ?? ''}`;
  if (!/^Lua 5\.1\.\d+(\s|$)/m.test(output)) throw new Error('addon-lua.interpreter_version_mismatch');
  return { report, reportSHA256: admitted.reportSHA256, executable, executableSHA256: report.executableSHA256, version: report.version };
}

export function cleanEnvironment() {
  return Object.fromEntries(Object.entries(process.env).filter(([name]) => !['LUA_PATH', 'LUA_CPATH', 'LUA_INIT'].includes(name.toUpperCase())));
}

export function discoverSuites(directory = suiteDirectory) {
  const suites = readdirSync(directory).filter(name => /^t_[a-z0-9_]+\.lua$/.test(name)).sort();
  if (!suites.length) throw new Error('addon-lua.no_suites');
  for (const helper of ['run.lua', 'env.lua']) regular(join(directory, helper), 1 << 20);
  return suites;
}

// One manifest for every supported client; Core\ClientGate.lua selects the
// running profile. Every referenced load must exist and all six event catalogs
// must be present. This is the current business contract, not a historical TOC.
export function verifyTOC(addonRoot) {
  const toc = regular(join(addonRoot, 'Lychee Dev.toc'), 1 << 20).toString('utf8').replaceAll('\r\n', '\n');
  const lines = toc.split('\n').map(line => line.trimEnd());
  const loads = lines.filter(line => line && !line.startsWith('#') && !line.startsWith('##'));
  if (!loads.length) throw new Error('addon-lua.toc_empty');
  if (!/^Core[\\/]ClientGate\.lua$/.test(loads[0])) throw new Error('addon-lua.toc_first_load');
  const interfaceLine = lines.find(line => line.startsWith('## Interface:'));
  if (!interfaceLine) throw new Error('addon-lua.toc_interface_missing');
  const declared = new Set(interfaceLine.slice('## Interface:'.length).split(',').map(value => value.trim()).filter(Boolean));
  for (const value of ['120105', '120100', '50504', '38002', '16001', '11509', '20506']) {
    if (!declared.has(value)) throw new Error(`addon-lua.toc_interface_missing_value: ${value}`);
  }
  for (const load of loads) {
    const relative = load.replaceAll('\\', sep).replaceAll('/', sep);
    if (!existsSync(join(addonRoot, relative))) throw new Error(`addon-lua.toc_load_missing: ${load}`);
  }
  const catalogs = loads.filter(load => /^Modules[\\/]Events[\\/]CatalogData_/.test(load)).sort();
  const expected = ['Anniversary', 'Era', 'Forever', 'Mainline', 'Mists', 'Titan'].map(name => `Modules\\Events\\CatalogData_${name}.lua`).sort();
  if (catalogs.length !== 6 || catalogs.join('\n') !== expected.join('\n')) throw new Error('addon-lua.toc_event_catalogs');
  return { loads: loads.length, interfaces: [...declared], catalogs };
}

export function runSuite(interpreter, file, client, addonRoot, run = spawnSync) {
  const result = run(interpreter, ['run.lua', file, client, addonRoot], {
    cwd: suiteDirectory,
    encoding: 'utf8',
    shell: false,
    windowsHide: true,
    timeout: 120000,
    maxBuffer: 4 << 20,
    env: cleanEnvironment(),
  });
  const output = `${result.stdout ?? ''}${result.stderr ?? ''}`.slice(-8192);
  if (result.error || result.status !== 0) return { file, client, state: 'failed', status: result.status, output };
  // run.lua normalizes the suite argument to a path; accept only its explicit
  // PASS line for this exact suite and profile.
  const pass = new RegExp(`PASS [^\\r\\n]*[/\\\\]${file.replaceAll('.', '\\.')} \\[${client}\\]`);
  if (!pass.test(output)) return { file, client, state: 'failed', status: result.status, output };
  return { file, client, state: 'passed' };
}

export function main(argv = [], run = spawnSync) {
  const { reportPath, files, clients, addonRoot } = parseArguments(argv);
  const interpreter = verifyInterpreter(reportPath, run);
  const suites = discoverSuites();
  const selectedSuites = files.length ? suites.filter(name => files.includes(name)) : suites;
  if (files.length && selectedSuites.length !== new Set(files).size) throw new Error('addon-lua.unknown_suite');
  const selectedClients = clients.length ? CLIENTS.filter(name => clients.includes(name)) : [...CLIENTS];
  const toc = verifyTOC(addonRoot);
  const cases = [];
  for (const client of selectedClients) {
    for (const file of selectedSuites) {
      cases.push(runSuite(interpreter.executable, file, client, addonRoot, run));
    }
  }
  const counts = { passed: cases.filter(row => row.state === 'passed').length, failed: cases.filter(row => row.state === 'failed').length };
  const report = {
    schema: 'lycheedev.addon-lua-regression.v1',
    interpreter: { version: interpreter.version, executable: interpreter.executable, executableSHA256: interpreter.executableSHA256, reportSHA256: interpreter.reportSHA256 },
    addonRoot, toc, suites: selectedSuites.length, clients: selectedClients, counts, cases,
  };
  process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
  return report;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const report = main(process.argv.slice(2));
    if (report.counts.failed) process.exitCode = 1;
  } catch (error) {
    console.error(JSON.stringify({ error: error.message }));
    process.exitCode = 1;
  }
}
