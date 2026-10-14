// Prepare fixed, public integration inputs for the existing Windows Go suite.
// No release assembly, npm operation, game discovery or product fallback.
import { spawnSync } from 'node:child_process';
import { appendFileSync, copyFileSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { goCaseResult, modulePath, options, parseGoEvents, repository, sha256 } from './baseline-common.mjs';
import { luaRuntime, stageLuaLS } from './luals.mjs';
import { verifyBinaryIdentity } from './release.mjs';
import { oracleCommit, oracleTests } from '../tests/data-regression/evidence-policy.mjs';

export const retailCommit = '31c7f7b9cc79e56c986b365c06a6afbcf3c9177b';
export const foreverCommit = '4d5d706b8e01c5ebe01c8dd9b7a07151d8d37069';
const docsPrefix = 'Interface/AddOns/Blizzard_APIDocumentationGenerated';
export const unitDigests = {
  retail: 'a33d4c585bc3458ab1179070778a2ebef5c324bc5c56bee9511a9fa9eee04a60',
  forever: '3cefaa815d7e207465a9086b60ee92242d5bdef076af13a227591d23f71662ce',
};
const children = (name, names) => [name, ...names.map(child => `${name}/${child}`)];
export const requiredCases = [
  { package: 'internal/codebase', tests: ['TestRealLuaLSMapsAPIReferencesAndDefinitions', 'TestRealBrokerAcrossCLIProcesses'] },
  { package: 'internal/command', tests: ['TestPackagedSourceSemanticCLIWhenAvailable', ...children('TestRealBrokerCloseWaitsForProcessCleanup', ['reply', 'listener'])] },
  { package: 'internal/luals', tests: ['TestRealRuntimePublicInterfaces', 'TestRealRuntimeOversizedLuaKeepsOrdinaryCoverage', 'TestRealWorkspaceSessionWarmAndCancellation', 'TestRealSessionCancelsAnInflightReadinessRequest', 'TestRealSessionEnvironmentAndConfigIsolation', 'TestRealRuntimeGeneratedEnvironment', 'TestRealRuntimeFullRetailAPIReferences'] },
  { package: 'internal/codebase/environment', tests: children('TestBuildPinnedUnitDocs', ['retail-UnitDocumentation.lua', 'forever-UnitDocumentation.lua', 'retail-ContainerDocumentation.lua', 'retail-AuctionHouseEnumsDocumentation.lua']) },
  { package: 'internal/codebase/flow', tests: children('TestExtractPinnedUnitDocumentationWhenAvailable', ['retail', 'forever']) },
  { package: 'internal/records/schema', tests: children('TestPinnedPublishedDefinitions', ['Map', 'SpellName', 'ChrClasses']) },
  { package: 'internal/records/navigatetest', tests: oracleTests },
];

export function verifyRequiredIntegrations(text) {
  const evidence = parseGoEvents(text.replace(/^\uFEFF/, ''));
  const results = requiredCases.map(spec => {
    const result = goCaseResult(spec, evidence);
    const packageResult = evidence.packages.get(`${modulePath}${spec.package}`);
    if (packageResult !== 'pass') {
      result.state = 'failed';
      result.findings.push(`package: ${packageResult ?? 'missing'}`);
    }
    return result;
  });
  return { state: results.every(result => result.state === 'passed') ? 'passed' : 'failed', results };
}

function run(command, args, env = process.env) {
  const result = spawnSync(command, args, { cwd: repository, env, encoding: 'utf8', windowsHide: true, timeout: 300000, maxBuffer: 32 * 1024 * 1024 });
  if (result.status !== 0) throw new Error(`${command} ${args.join(' ')} failed: ${result.error ?? ''}\n${result.stdout ?? ''}\n${result.stderr ?? ''}`);
  return result.stdout.trim();
}

function checkout(repo, commit, destination) {
  mkdirSync(destination);
  const git = args => run('git', ['-c', 'core.autocrlf=false', '-c', 'core.longpaths=true', '-C', destination, ...args]);
  git(['init', '--quiet']);
  git(['fetch', '--depth=1', `https://github.com/${repo}.git`, commit]);
  git(['checkout', '--quiet', '--detach', 'FETCH_HEAD']);
  if (git(['rev-parse', 'HEAD']) !== commit || git(['status', '--porcelain'])) throw new Error(`pinned checkout mismatch: ${repo}`);
  return destination;
}

export function verifyUnitDocument(variant, bytes) {
  if (!Object.hasOwn(unitDigests, variant) || sha256(bytes) !== unitDigests[variant]) throw new Error(`pinned ${variant} UnitDocumentation.lua digest mismatch`);
}

export async function prepareDocumentation(out) {
  const retail = checkout('Gethe/wow-ui-source', retailCommit, join(out, 'retail'));
  const docs = join(out, 'docs');
  mkdirSync(docs);
  const records = [];
  for (const name of ['UnitDocumentation.lua', 'ContainerDocumentation.lua', 'AuctionHouseEnumsDocumentation.lua']) {
    const source = join(retail, docsPrefix, name);
    const bytes = readFileSync(source);
    if (name === 'UnitDocumentation.lua') verifyUnitDocument('retail', bytes);
    copyFileSync(source, join(docs, `retail-${name}`));
    records.push({ path: name, commit: retailCommit, bytes: bytes.length, sha256: sha256(bytes) });
  }
  const url = `https://raw.githubusercontent.com/Gethe/wow-ui-source/${foreverCommit}/${docsPrefix}/UnitDocumentation.lua`;
  const response = await fetch(url, { signal: AbortSignal.timeout(60000) });
  if (!response.ok) throw new Error(`pinned Forever documentation download failed: ${response.status}`);
  const bytes = Buffer.from(await response.arrayBuffer());
  verifyUnitDocument('forever', bytes);
  writeFileSync(join(docs, 'forever-UnitDocumentation.lua'), bytes, { flag: 'wx' });
  records.push({ path: 'UnitDocumentation.lua', commit: foreverCommit, url, bytes: bytes.length, sha256: sha256(bytes) });
  run('go', ['run', 'tools/ci-source-definitions.go', retail, retailCommit, join(out, 'definitions')]);
  writeFileSync(join(out, 'documentation-inputs.json'), JSON.stringify({ repository: 'Gethe/wow-ui-source', retailCommit, records }, null, 2) + '\n', { flag: 'wx' });
  return { retail, docs };
}

export async function prepareStage(stage, archivePath) {
  const commit = run('git', ['rev-parse', 'HEAD']);
  run(process.execPath, ['tools/live-stage.mjs', '--out', stage, '--commit', commit]);
  const manifestPath = join(stage, 'release.json');
  const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));
  await stageLuaLS(stage, manifest.resources, { archivePath });
  const binaryPath = 'native/windows-amd64/lycheedev.exe';
  const binary = join(stage, binaryPath);
  mkdirSync(dirname(binary), { recursive: true });
  run('go', ['build', '-trimpath', '-buildvcs=true', '-o', binary, './cmd/lycheedev'], { ...process.env, CGO_ENABLED: '0' });
  const bytes = readFileSync(binary);
  manifest.binaries['windows-amd64'] = { path: binaryPath, bytes: bytes.length, sha256: sha256(bytes) };
  writeFileSync(manifestPath, JSON.stringify(manifest, null, 2) + '\n');
  return binary;
}

async function prepare(out) {
  if (process.platform !== 'win32' || process.arch !== 'x64') throw new Error('CI integration preparation requires native Windows amd64');
  // New output only: no cleanup or replacement of prior evidence.
  mkdirSync(out);
  const receipt = { state: 'preparing', toolkitCommit: run('git', ['rev-parse', 'HEAD']), toolkitTree: run('git', ['rev-parse', 'HEAD^{tree}']), luaRuntime, retailCommit, foreverCommit, oracleCommit };
  const receiptPath = join(out, 'inputs.json');
  writeFileSync(receiptPath, JSON.stringify(receipt, null, 2) + '\n', { flag: 'wx' });
  receipt.compilers = Object.fromEntries(['gcc', 'g++'].map(compiler => [compiler, run(compiler, ['--version'])]));
  writeFileSync(receiptPath, JSON.stringify(receipt, null, 2) + '\n');
  const { retail, docs } = await prepareDocumentation(out);
  const stage = join(out, 'release');
  const binary = await prepareStage(stage, process.env.LYCHEEDEV_LUALS_ARCHIVE);
  const version = JSON.parse(readFileSync(join(stage, 'release.json'), 'utf8')).version;
  receipt.binaryIdentity = verifyBinaryIdentity(stage, { commit: receipt.toolkitCommit, dirty: run('git', ['status', '--porcelain']).length > 0, version });
  writeFileSync(receiptPath, JSON.stringify(receipt, null, 2) + '\n');
  const casc = checkout('ladislav-zezula/CascLib', oracleCommit, join(out, 'CascLib'));
  run(process.execPath, ['tests/data-regression/build-oracle.mjs', casc, join(out, 'oracle')]);
  const env = {
    LYCHEEDEV_LUALS_TEST_RUNTIME: join(stage, 'payload/tool/luals'),
    LYCHEEDEV_SOURCE_PACKAGE_RELEASE_ROOT: stage,
    LYCHEEDEV_SOURCE_PACKAGE_BINARY: binary,
    LYCHEEDEV_BROKER_TEST_RELEASE: stage,
    LYCHEEDEV_BROKER_TEST_BINARY: binary,
    LYCHEEDEV_ENVIRONMENT_TEST_DOCS: docs,
    LYCHEEDEV_FLOW_RETAIL_DOC: join(docs, 'retail-UnitDocumentation.lua'),
    LYCHEEDEV_FLOW_FOREVER_DOC: join(docs, 'forever-UnitDocumentation.lua'),
    LYCHEEDEV_LUALS_REAL_RETAIL_WORKSPACE: retail,
    LYCHEEDEV_LUALS_REAL_RETAIL_DEFINITIONS: join(out, 'definitions/api.d.lua'),
    LYCHEEDEV_TEST_DBD_NETWORK: '1',
    LYCHEEDEV_CASC_ORACLE: join(out, 'oracle/casc-oracle.exe'),
  };
  writeFileSync(receiptPath, JSON.stringify({ ...receipt, state: 'ready', environment: env }, null, 2) + '\n');
  if (!process.env.GITHUB_ENV) throw new Error('GITHUB_ENV is required to select the prepared CI inputs');
  appendFileSync(process.env.GITHUB_ENV, Object.entries(env).map(([name, value]) => `${name}=${value}\n`).join(''));
  return { out, inputs: Object.keys(env) };
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const [command, ...argv] = process.argv.slice(2);
  const opts = options(argv, command === 'prepare' ? ['--out'] : ['--events', '--out']);
  if (!opts['--out']) throw new Error('--out is required');
  if (command === 'prepare') console.log(JSON.stringify(await prepare(resolve(opts['--out'])), null, 2));
  else if (command === 'verify' && opts['--events']) {
    const result = verifyRequiredIntegrations(readFileSync(opts['--events'], 'utf8'));
    writeFileSync(opts['--out'], JSON.stringify(result, null, 2) + '\n');
    console.log(JSON.stringify(result, null, 2));
    if (result.state !== 'passed') process.exitCode = 1;
  } else throw new Error('usage: node tools/ci-inputs.mjs prepare --out <new directory> | verify --events <Go JSONL> --out <report>');
}
