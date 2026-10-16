// Optional one-time preparation, separate from all measured paths. Build the
// fixed release's own private dev package; never npm publish or edit Go source.
import assert from 'node:assert/strict';
import {copyFileSync, existsSync, lstatSync, mkdirSync, readFileSync, readdirSync, writeFileSync} from 'node:fs';
import {dirname, join, resolve} from 'node:path';
import {performance} from 'node:perf_hooks';
import {beginScratch, contained} from './rust-disk.mjs';
import {ownedSpawnSync} from './rust-owned-process.mjs';
import {hashFile} from './rust-build.mjs';
import {GO_COMMIT} from '../tests/rust-performance-paired/contract.mjs';
import {inventory, verifyGoMetadata, verifyGoPackage} from '../tests/rust-performance-paired/inputs.mjs';

const [sourceArg, archiveArg, npmArg, outputArg, ...extra] = process.argv.slice(2);
assert(process.platform === 'win32' && sourceArg && archiveArg && npmArg && outputArg && !extra.length,
  'usage (Windows): node tools/rust-performance-prepare-go.mjs <clean-fixed-Go-checkout> <pinned-LuaLS.zip> <npm-cli.js> <new-artifact-directory>');
const root = resolve('.'), source = resolve(sourceArg), archive = resolve(archiveArg), npm = resolve(npmArg), output = contained(root, resolve(outputArg));
assert(!existsSync(output), 'new artifact directory required');
const job = beginScratch(root, 'paired-fixed-go-prepare', {command: process.argv, inputs: {commit: GO_COMMIT, archiveSHA256: hashFile(archive), npmCliSHA256: hashFile(npm)}, admissionOptions: {requiredBytes: 1 << 30, scratchReserveBytes: 1 << 30}});
let state = 'failed', handlesReaped = true; const calls = [], start = performance.now();
const env = Object.fromEntries(Object.entries(process.env).filter(([key]) => !key.toUpperCase().startsWith('GIT_') && key.toUpperCase() !== 'GCM_INTERACTIVE' && !/^(GOCACHE|GOMODCACHE|GOTMPDIR|GOENV|GOTOOLCHAIN)$/i.test(key)));
Object.assign(env, {GIT_CONFIG_NOSYSTEM: '1', GIT_CONFIG_GLOBAL: 'NUL', GIT_TERMINAL_PROMPT: '0', GCM_INTERACTIVE: 'never', GIT_NO_REPLACE_OBJECTS: '1', GOTOOLCHAIN: 'local', GOENV: 'off', GOCACHE: join(job.directory, 'go-cache'), GOMODCACHE: join(job.directory, 'go-mod-cache'), GOTMPDIR: join(job.directory, 'go-tmp')});
function run(command, args, timeout = 60000) {
  handlesReaped = false; const begin = performance.now();
  const result = ownedSpawnSync(command, args, {cwd: source, env, evidence: job.evidence, timeout}); handlesReaped = result.handlesReaped === true;
  calls.push({command, args, wallMillis: performance.now() - begin, exit: result.status, error: result.error?.message, handlesReaped, logs: result.logs});
  writeFileSync(join(job.evidence, 'preparation.json'), JSON.stringify({state: 'running', calls}, null, 2) + '\n');
  assert(handlesReaped && !result.error && result.status === 0, `preparation command failed: ${command}`); return result.stdout.toString();
}
function checkout() {
  assert.equal(run('git', ['--no-optional-locks', '-c', 'core.fsmonitor=false', 'rev-parse', 'HEAD']).trim(), GO_COMMIT);
  assert.equal(run('git', ['--no-optional-locks', '-c', 'core.fsmonitor=false', 'status', '--porcelain', '--untracked-files=all']).trim(), '', 'fixed Go checkout must be clean');
}
try {
  for (const directory of [env.GOCACHE, env.GOMODCACHE, env.GOTMPDIR]) mkdirSync(directory);
  checkout(); assert.match(run('go', ['version']), /^go version go1\.27\.1 windows\/amd64\s*$/);
  const prepared = join(job.directory, 'go-package');
  run(process.execPath, [join(source, 'tools/release.mjs'), 'dev-package', '--out', prepared, '--npm-cli', npm, '--luals-archive', archive], 600000);
  checkout();
  const packageRoot = join(prepared, 'dev-npm-stage'), verified = verifyGoPackage(packageRoot, archive);
  const build = verifyGoMetadata(run('go', ['version', '-m', verified.binary]));
  // Only authenticated complete runtime/package bytes and the private tarball
  // become artifacts; temporary npm cache stays inside this owner's scratch.
  mkdirSync(output);
  for (const row of inventory(packageRoot).rows) {
    const target = join(output, 'dev-npm-stage', row.path); mkdirSync(dirname(target), {recursive: true}); copyFileSync(join(packageRoot, row.path), target);
  }
  const tarball = join(prepared, 'lycheedev-local-3.2.3.tgz'); copyFileSync(tarball, join(output, 'lycheedev-local-3.2.3.tgz'));
  assert.equal(hashFile(join(output, 'lycheedev-local-3.2.3.tgz')), hashFile(tarball));
  const preserved = verifyGoPackage(join(output, 'dev-npm-stage'), archive); assert.equal(preserved.packageSHA256, verified.packageSHA256);
  const record = {schema: 'lycheedev.fixed-go-performance-artifact.v1', commit: GO_COMMIT, privateDevelopmentPackage: true, publishEligible: false, package: preserved, build, source, sourceCleanBeforeAndAfter: true, toolchain: 'go1.27.1', preparationWallMillis: performance.now() - start, calls, receipt: job.receipt, tarballSHA256: hashFile(tarball)};
  writeFileSync(join(output, 'artifact.json'), JSON.stringify(record, null, 2) + '\n');
  writeFileSync(join(job.evidence, 'artifact.json'), JSON.stringify(record, null, 2) + '\n'); state = 'succeeded';
  console.log(JSON.stringify({state, artifact: join(output, 'artifact.json'), package: preserved.root}));
} finally {
  writeFileSync(join(job.evidence, 'preparation.json'), JSON.stringify({state, wallMillis: performance.now() - start, calls}, null, 2) + '\n');
  const logs = readdirSync(job.evidence).filter(name => name !== 'receipt.json' && lstatSync(join(job.evidence, name)).isFile()).map(name => join(job.evidence, name));
  job.seal({state: handlesReaped ? state : 'interrupted', handlesReaped, logs}); if (handlesReaped && state === 'succeeded') job.cleanup();
}
