// Structural pin for the current Rust product CI (N12 / CI-01..04).
//
// The old Go pipelines are retired: this file asserts the four-stage
// static -> build -> validate -> release-ready shape, the explicit artifact
// handoff between jobs, the absence of Go/legacy references and the absence of
// any implicit rebuild in downstream jobs. Line-based on purpose: these files
// are hand-maintained and every load-bearing line is a single constant. It
// cannot prove a GitHub Actions run succeeded; it proves the pipeline shape.
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import test from 'node:test';
import assert from 'node:assert/strict';

const repository = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const workflowDirectory = join(repository, '.github/workflows');
const workflowNames = readdirSync(workflowDirectory).sort();
const ci = readFileSync(join(workflowDirectory, 'ci.yml'), 'utf8');
const release = readFileSync(join(workflowDirectory, 'release.yml'), 'utf8');
const workflows = [['ci.yml', ci], ['release.yml', release]];
const RETIRED_WORKFLOWS = [
  'toolkit-ci.yml', 'toolkit-release.yml', 'rust-parity.yml', 'rust-mailbox-oracle.yml',
  'rust-performance.yml', 'rust-luac-rebuild.yml', 'rust-toolchain-identity.yml',
];

function jobBodies(text) {
  const lines = text.split('\n');
  const jobsAt = lines.findIndex(line => /^jobs:\s*$/.test(line));
  assert.ok(jobsAt >= 0, 'workflow has no jobs section');
  const jobs = new Map();
  let current = null;
  let body = [];
  for (let index = jobsAt + 1; index < lines.length; index++) {
    const line = lines[index];
    const start = /^  ([A-Za-z][A-Za-z0-9_-]*):\s*$/.exec(line);
    if (start) {
      if (current) jobs.set(current, body.join('\n'));
      current = start[1];
      body = [];
      continue;
    }
    if (/^\S/.test(line) && current) break; // next top-level key
    if (current) body.push(line);
  }
  if (current) jobs.set(current, body.join('\n'));
  return jobs;
}

function withoutComments(text) {
  return text.split('\n').filter(line => !/^\s*#/.test(line)).join('\n');
}

function usesLines(text) {
  return text.split('\n').map(line => line.trim()).filter(line => /^(?:- )?uses:/.test(line));
}

test('only the current Rust CI workflows exist', () => {
  assert.deepEqual(workflowNames, ['ci.yml', 'release.yml']);
  for (const retired of RETIRED_WORKFLOWS) assert.equal(existsSync(join(workflowDirectory, retired)), false, `${retired} must be retired`);
});

test('ci has the four stages with machine-readable job names', () => {
  const jobs = jobBodies(ci);
  assert.deepEqual([...jobs.keys()], [
    'static', 'build-test-programs', 'build-release', 'validate-rust',
    'validate-addon', 'validate-package', 'release-ready', 'ci-required',
  ]);
  assert.match(ci, /^on:\s*$/m);
  assert.match(ci, /^  push:\s*$/m);
  assert.match(ci, /^  pull_request:\s*$/m);
  assert.match(ci, /^  workflow_dispatch:\s*$/m);
  assert.match(ci, /^  workflow_call:\s*$/m);
});

test('no Go or retired pipeline reference remains in the workflows', () => {
  const forbidden = [
    /setup-go/i, /go-version/i, /\bgo\s+(?:build|test|vet|run|mod|version)\b/, /CGO_ENABLED/, /GOOS\s*=/, /GOARCH\s*=/,
    /toolkit-ci/, /toolkit-release/, /rust-parity/, /rust-mailbox-oracle/, /rust-performance/,
    /rust-luac-rebuild/, /rust-toolchain-identity/, /tests\/rust-parity/,
  ];
  for (const [name, text] of workflows) {
    for (const pattern of forbidden) assert.doesNotMatch(text, pattern, `${name} references retired Go pipeline: ${pattern}`);
  }
});

test('every job targets windows-2025 and normalizes TMP', () => {
  for (const [name, text] of workflows) {
    const jobs = jobBodies(text);
    for (const [job, body] of jobs) {
      if (body.includes('uses: ./.github/workflows/')) {
        assert.match(body, /uses: \.\/\.github\/workflows\/ci\.yml/, `${name}:${job} must call the reusable CI workflow`);
        continue;
      }
      assert.match(body, /runs-on: windows-2025/, `${name}:${job} must run on windows-2025`);
      assert.match(body, /Normalize TMP to the long runner temp path/, `${name}:${job} lacks TMP normalization`);
      assert.doesNotMatch(body, /ubuntu-|macos-|self-hosted|darwin/i, `${name}:${job} references a removed platform`);
    }
  }
});

test('Node and Rust toolchains are pinned consistently', () => {
  for (const [name, text] of workflows) {
    assert.match(text, /node-version: '24\.18\.1'/, `${name} must pin Node 24.18.1`);
    assert.match(text, /rustup toolchain install 1\.99\.0 --profile minimal/, `${name} must install the pinned Rust toolchain`);
  }
  assert.match(ci, /if \(\$LASTEXITCODE -ne 0 -or \$version -cnotcontains 'release: 1\.99\.0'\)/);
  const staticJob = jobBodies(ci).get('static');
  const install = staticJob.indexOf('rustup toolchain install 1.99.0 --profile minimal');
  const select = staticJob.indexOf('rustup override set 1.99.0');
  const tests = staticJob.indexOf('node --test');
  assert(install >= 0 && select > install && tests > select,
    'static identity tests must install and select the pinned toolchain before running');
  assert.match(staticJob, /rustup -V/);
  assert.match(staticJob, /Get-Command cargo -CommandType Application/);
  assert.match(staticJob, /Get-FileHash -LiteralPath \$cargo -Algorithm SHA256/);
  assert.match(staticJob, /fileVersion = \$cargoVersion\.FileVersion; productVersion = \$cargoVersion\.ProductVersion/);
});

test('actual PowerShell toolchain checks accept multiline pinned output and reject drift or command failure', {skip: process.platform !== 'win32'}, () => {
  const lines = ['rustc 1.99.0 (example)', 'binary: rustc', 'commit-hash: example',
    'commit-date: 2026-01-01', 'host: x86_64-pc-windows-msvc', 'release: 1.99.0', 'LLVM version: 22.1.0'];
  const fixtures = [
    {name: 'pinned-multiline', lines, exit: 0, rejected: false},
    {name: 'command-failed', lines, exit: 1, rejected: true},
    ...['1.98.0', '1.99.01', '1.99.0-nightly'].map(version => ({name: version,
      lines: lines.map(line => line.startsWith('release: ') ? `release: ${version}` : line), exit: 0, rejected: true})),
    {name: 'missing-release', lines: lines.filter(line => !line.startsWith('release: ')), exit: 0, rejected: true},
    {name: 'non-field-substring', lines: lines.map(line => line.startsWith('release: ') ? `note: ${line}` : line), exit: 0, rejected: true},
  ];
  const literal = value => "'" + value.replaceAll("'", "''") + "'";
  const commands = [], expected = [];
  for (const [job, body] of jobBodies(ci)) {
    if (!body.includes('$version = rustc -Vv')) continue;
    const check = /^\s*if \(\$LASTEXITCODE[^\n]*\$version[^\n]*\) \{ throw "rustc drift: \$version" \}\s*$/m.exec(body)?.[0].trim();
    assert(check, `${job} needs its actual toolchain rejection condition`);
    for (const fixture of fixtures) {
      commands.push(`$version=@(${fixture.lines.map(literal).join(',')});$LASTEXITCODE=${fixture.exit};$rejected=$false;try{${check}}catch{$rejected=$true};[Console]::Out.WriteLine((${literal(`${job}:${fixture.name}:`)}+$rejected))`);
      expected.push(`${job}:${fixture.name}:${fixture.rejected ? 'True' : 'False'}`);
    }
  }
  assert.equal(expected.length, 21, 'exercise static and both build jobs with all positive and negative outputs');
  const result = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-EncodedCommand',
    Buffer.from(commands.join('\n'), 'utf16le').toString('base64')], {
    encoding: 'utf8', windowsHide: true, shell: false, timeout: 10000, maxBuffer: 1 << 20,
  });
  assert.equal(result.error, undefined); assert.equal(result.status, 0, result.stderr);
  assert.deepEqual(result.stdout.trim().split(/\r?\n/), expected);
});

test('build stage compiles each required configuration exactly once', () => {
  const jobs = jobBodies(ci);
  for (const build of ['build-test-programs', 'build-release']) {
    const body = jobs.get(build);
    assert.match(body, /if: needs\.static\.outputs\.native == 'true'/, `${build} must only run for native change sets`);
    assert.doesNotMatch(body, /continue-on-error/, `${build} must not hide a build failure`);
  }
  const testBuild = jobs.get('build-test-programs');
  assert.equal((testBuild.match(/node tools\/ci-test-programs\.mjs build/g) ?? []).length, 1, 'test configuration must be compiled once');
  assert.match(testBuild, /--owner ci-test-programs-build/);
  const releaseBuild = jobs.get('build-release');
  assert.equal((releaseBuild.match(/node tools\/rust-build\.mjs/g) ?? []).length, 1, 'release configuration must be compiled once');
  assert.match(releaseBuild, /node tools\/rust-build\.mjs \.tmp\/ci\/build-release\/cli release --one-shot/);
  // Native dependencies and packages are produced in the same build stage.
  for (const command of [
    /node tools\/luac\.mjs build \.tmp\/ci\/build-release\/luac/,
    /node tools\/rust-formal-lua\.mjs build \.tmp\/ci\/build-release\/lua51/,
    /node tools\/addon-package\.mjs build --out \.tmp\/ci\/build-release\/addon/,
    /node tools\/rust-skill\.mjs \.tmp\/ci\/build-release\/skill\/staged \.tmp\/ci\/build-release\/cli\/lycheedev\.exe/,
    /node tools\/rust-package-smoke\.mjs \.tmp\/ci\/build-release\/npm-package \.tmp\/ci\/build-release\/luac --artifact \.tmp\/ci\/build-release\/cli\/artifact\.json --profile release --assembly-only/,
  ]) assert.match(releaseBuild, command);
  // The pinned Emmy source is an authenticated external input; both native build
  // jobs prepare it before any Cargo fetch/build rather than relying on a cache.
  for (const body of [testBuild, releaseBuild]) {
    assert.match(body, /python third_party\/emmy\/prepare\.py --download/, 'native build must prepare the authenticated Emmy source');
  }
  assert.doesNotMatch(releaseBuild, /build-lua\.mjs \.tmp\/ci\/build-release\/lua51 gcc/, 'Windows runners have no gcc; the pinned MSVC identity is used');
});

test('build stage seals and uploads its artifacts', () => {
  const jobs = jobBodies(ci);
  const testBuild = jobs.get('build-test-programs');
  const releaseBuild = jobs.get('build-release');
  assert.match(testBuild, /ci-artifacts\.mjs seal --out \.tmp\/ci\/build-test\/build-manifest\.json --component test-programs=\.tmp\/ci\/build-test/);
  assert.match(releaseBuild, /ci-artifacts\.mjs seal --out \.tmp\/ci\/build-release\/build-manifest\.json/);
  for (const kind of ['cli-release', 'luac', 'lua51', 'addon-zip', 'skill', 'npm-package']) {
    assert.match(releaseBuild, new RegExp(`--component ${kind}=\\.tmp\\/ci\\/build-release\\/`), `release manifest must seal ${kind}`);
  }
  assert.match(testBuild, /name: ci-build-test/);
  assert.match(releaseBuild, /name: ci-build-release/);
});

test('validate jobs download this run artifacts and verify the sealed manifests', () => {
  const jobs = jobBodies(ci);
  const rust = jobs.get('validate-rust');
  assert.match(rust, /needs: \[static, build-test-programs, build-release\]/);
  assert.equal((rust.match(/name: ci-build-test/g) ?? []).length, 1);
  assert.match(rust, /ci-artifacts\.mjs verify --manifest \.tmp\/ci\/build-test\/build-manifest\.json --require test-programs/);
  assert.match(rust, /ci-test-programs\.mjs run \.tmp\/ci\/build-test\/test-programs\.json/);
  assert.match(rust, /--report \.tmp\/ci\/validate-rust\/report\.json/);
  assert.match(rust, /--formal-lua \.tmp\/ci\/build-release\/lua51\/build-report\.json/);
  assert.match(rust, /name: ci-build-release/);
  assert.match(rust, /--require lua51/);

  const addon = jobs.get('validate-addon');
  assert.match(addon, /needs: \[static, build-release\]/);
  assert.match(addon, /name: ci-build-release/);
  assert.match(addon, /--require lua51,addon-zip/);
  assert.match(addon, /node tests\/addon\/run-all\.mjs \.tmp\/ci\/build-release\/lua51\/build-report\.json/);
  assert.match(addon, /addon-package\.mjs verify --zip/);

  const packageJob = jobs.get('validate-package');
  assert.match(packageJob, /needs: \[static, build-release\]/);
  assert.match(packageJob, /--require cli-release,luac,skill,npm-package/);
  assert.match(packageJob, /skill-contract\.mjs --skill-root \.tmp\/ci\/build-release\/skill\/staged --bin \.tmp\/ci\/build-release\/cli\/lycheedev\.exe/);
  assert.match(packageJob, /rust-package-smoke\.mjs \.tmp\/ci\/validate-package\/npm \.tmp\/ci\/build-release\/luac --artifact \.tmp\/ci\/build-release\/cli\/artifact\.json --profile release --dependency-closure \.tmp\/ci\/build-release\/npm-package\/license-closure/);
  assert.match(packageJob, /release\.mjs audit-tgz --tgz \$report\.tgz --version \$report\.version\.result\.version --commit \$report\.commit --development-only/);
});

test('downstream jobs never compile, rebuild luac or repack the native binary', () => {
  const jobs = jobBodies(ci);
  const forbidden = [
    /rust-build\.mjs/, /luac\.mjs/, /tests\/tools\/build-lua\.mjs/, /buildRust/,
    /cargo (?:build|test|check|clippy|run|fetch|metadata)/,
    /rust-cargo\.mjs --[^\n]*--\s*(?:build|test|check|clippy|fetch|metadata)/,
    /rust-dependencies\.mjs/, /rust-formal-lua\.mjs build/,
  ];
  for (const job of ['validate-rust', 'validate-addon', 'validate-package', 'release-ready']) {
    const body = jobs.get(job);
    for (const pattern of forbidden) assert.doesNotMatch(body, pattern, `${job} performs an implicit build: ${pattern}`);
  }
  // Package smoke receives the already-built binary; validate also receives
  // its complete license closure instead of invoking Cargo dependency inventory.
  for (const line of ci.split('\n').filter(line => line.includes('rust-package-smoke.mjs'))) {
    assert.match(line, /--artifact \.tmp\/ci\/build-release\/cli\/artifact\.json/, 'package smoke call must name its build artifact');
  }
  assert.match(jobs.get('validate-package'), /--dependency-closure \.tmp\/ci\/build-release\/npm-package\/license-closure/);
  assert.doesNotMatch(jobs.get('build-release'), /--dependency-closure/, 'build stage generates the original closure');
});

test('release-ready seals the validated artifacts of this run without compiling', () => {
  const body = jobBodies(ci).get('release-ready');
  assert.match(body, /needs: \[static, build-test-programs, build-release, validate-rust, validate-addon, validate-package\]/);
  for (const artifact of ['ci-build-test', 'ci-build-release', 'ci-validate-rust', 'ci-validate-addon', 'ci-validate-package']) {
    assert.match(body, new RegExp(`name: ${artifact}`), `release-ready must consume ${artifact}`);
  }
  assert.match(body, /ci-release\.mjs identity --tag "v\$version" --unverified/);
  assert.match(body, /ci-release\.mjs candidate --identity /);
  assert.match(body, /--build-manifest \.tmp\/ci\/build-release\/build-manifest\.json/);
  assert.match(body, /--test-manifest \.tmp\/ci\/build-test\/build-manifest\.json/);
  assert.match(body, /--npm-report \.tmp\/ci\/validate-package\/npm\/report\.json/);
  assert.match(body, /--report package-audit=\.tmp\/ci\/validate-package\/package-audit\.json/);
  assert.match(body, /name: ci-release-candidate/);
  assert.doesNotMatch(body, /rust-build\.mjs|luac\.mjs|cargo /);
});

test('release workflow reuses CI and seals the same candidate without publishing', () => {
  assert.match(release, /^  push:\s*$/m);
  assert.match(release, /tags:\s*\n\s+- 'v\*'/);
  assert.match(release, /uses: \.\/\.github\/workflows\/ci\.yml/);
  assert.match(release, /ci-release\.mjs identity --tag \$env:GITHUB_REF_NAME --ref \$env:GITHUB_REF/);
  assert.match(release, /ci-release\.mjs seal --candidate \.tmp\/ci\/release-candidate --identity \.tmp\/ci\/release-identity\/identity\.json --out \.tmp\/ci\/release-sealed/);
  assert.match(release, /name: ci-release-candidate/);
  assert.match(release, /name: release-sealed/);
  for (const pattern of [/npm publish/, /gh release/, /id-token: write/, /environment: npm/, /NODE_AUTH_TOKEN|NPM_TOKEN/, /rust-build\.mjs/, /luac\.mjs/, /cargo /, /setup-go/i]) {
    assert.doesNotMatch(withoutComments(release), pattern, `release workflow must not publish or rebuild: ${pattern}`);
  }
});

test('static stage is native-free and documentation-only change sets skip native stages', () => {
  const body = jobBodies(ci).get('static');
  assert.match(body, /id: changes/);
  assert.match(body, /node tools\/ci-changes\.mjs --github-output \$env:GITHUB_OUTPUT/);
  assert.match(body, /node tools\/ci-docs\.mjs check/);
  assert.match(body, /node --test tools\/workflow-structure\.test\.mjs/);
  assert.match(body, /node tools\/version\.mjs --check/);
  assert.match(body, /node tools\/skill-contract\.mjs/);
  assert.match(body, /outputs:\s*\n\s+native: \$\{\{ steps\.changes\.outputs\.native \}\}/);
  assert.doesNotMatch(body, /node tools\/rust-build\.mjs|node tools\/luac\.mjs|tests\/tools\/build-lua\.mjs|cargo (?:build|test)/);
  assert.ok((body.match(/if: steps\.changes\.outputs\.native == 'true'/g) ?? []).length >= 4, 'non-doc static checks must be gated on native change sets');
});

test('required aggregations reject failures, skips and cancellation', () => {
  for (const [name, text, required] of [
    ['ci.yml', ci, ['static', 'build-test-programs', 'build-release', 'validate-rust', 'validate-addon', 'validate-package', 'release-ready']],
    ['release.yml', release, ['identity', 'ci', 'release-ready']],
  ]) {
    const aggregator = jobBodies(text).get(name === 'ci.yml' ? 'ci-required' : 'release-required');
    assert.match(aggregator, /if: always\(\)/, `${name} aggregation must always run`);
    for (const job of required) assert.match(aggregator, new RegExp(`(?:needs: \\[[^\\]]*|'${job}'|\\b${job}\\b)`), `${name} aggregation must require ${job}`);
    assert.match(aggregator, /did not succeed/, `${name} aggregation must fail on non-success results`);
  }
  const ciRequired = jobBodies(ci).get('ci-required');
  assert.match(ciRequired, /NATIVE: \$\{\{ needs\.static\.outputs\.native \}\}/);
  assert.match(ciRequired, /documentation-only change set: native build\/validate\/release stages intentionally skipped/);
});

test('uploads are explicit and no failure is silently tolerated', () => {
  const uploads = ci.split('\n').filter(line => line.includes('actions/upload-artifact@'));
  assert.ok(uploads.length >= 6, 'build and validate stages must upload artifacts');
  const guarded = ci.split('\n').filter(line => line.includes('if-no-files-found: error'));
  assert.equal(guarded.length, uploads.length, 'every artifact upload must fail when its files are missing');
  assert.doesNotMatch(ci, /if-no-files-found:\s*(?:warn|ignore)/);
});

test('validation report uploads run after failures without masking the failed jobs', () => {
  const jobs = jobBodies(ci);
  for (const [job, artifact] of [
    ['validate-rust', 'ci-validate-rust'],
    ['validate-addon', 'ci-validate-addon'],
    ['validate-package', 'ci-validate-package'],
  ]) {
    const body = jobs.get(job);
    const uploads = body.split(/^      - /m).filter(step => step.includes('actions/upload-artifact@')
      && new RegExp(`^          name: ${artifact}\\s*$`, 'm').test(step));
    assert.equal(uploads.length, 1, `${job} must retain its existing report artifact`);
    assert.match(uploads[0], /^        if: always\(\)\s*$/m, `${job} must upload its reports after a validation failure`);
    assert.match(uploads[0], new RegExp(`^          path: \\.tmp/ci/${job}\\s*$`, 'm'));
    assert.match(uploads[0], /if-no-files-found: error/);
    assert.doesNotMatch(body, /continue-on-error/, `${job} must retain its validation failure`);
    assert.match(body, /if \(\$LASTEXITCODE -ne 0\) \{ throw /);
  }
});

test('all action references are immutable, including named steps', () => {
  for (const [name, text] of workflows) {
    for (const line of usesLines(text)) {
      const uses = /^(?:- )?uses: (\S+)/.exec(line)?.[1];
      if (!uses || uses.startsWith('./')) continue;
      assert.match(uses, /@[0-9a-f]{40}$/,
        `${name}: action ${uses} is not pinned to an immutable commit`);
    }
  }
});

test('release helpers hold no Go, LuaLS or build/publish path', () => {
  const releaseHelpers = readFileSync(join(repository, 'tools/release.mjs'), 'utf8');
  for (const pattern of [
    /stageLuaLS\(|generateGoIdentity\(/, /CGO_ENABLED/, /spawnSync\('go'|runOk\('go'|'go', \[/,
    /\bgoos\b|\bgoarch\b|\bGOOS\b|\bGOARCH\b/, /tool\/luals\/|require\('\.\/luals|from '\.\/luals'/,
    /assembleCommand|devPackageCommand|verifyCgoCommand|platformEvidenceCommand|registryStateCommand/,
    /npm publish|gh release/, /writeTarGz\(|buildCorrespondingSourceArchive/,
  ]) assert.doesNotMatch(releaseHelpers, pattern, `tools/release.mjs still carries a retired path: ${pattern}`);
  for (const exported of ['policyFor', 'auditTgz', 'releaseJson', 'TARGETS', 'validateNpmChannel']) {
    assert.match(releaseHelpers, new RegExp(`export (?:const|function) ${exported}\\b`), `tools/release.mjs must export ${exported}`);
  }
  assert.match(releaseHelpers, /payload\/tool\/luac/);
  assert.match(ci, /release\.mjs audit-tgz/);
});

// GitHub needs exposes direct dependencies only, not their ancestors.
function checkOutputDependencies(jobs) {
 for(const [job,body]of jobs){
  const declaration=/^    needs:\s*\[([^\]]*)\]\s*$/m.exec(body);
  const direct=new Set(declaration?declaration[1].split(',').map(name=>name.trim()):[]);
  for(const [,dependency]of withoutComments(body).matchAll(/needs\.([A-Za-z_][A-Za-z0-9_-]*)\.outputs\b/g)){
   assert(direct.has(dependency),`${job} reads ${dependency} outputs without a direct needs dependency`);
  }
 }
}
test('every needs output reference names a direct job dependency',()=>{
 for(const [,workflow]of workflows)checkOutputDependencies(jobBodies(workflow));
 for(const job of ['validate-rust','validate-addon','validate-package','release-ready']){
  const jobs=jobBodies(ci);jobs.set(job,jobs.get(job).replace('needs: [static, ','needs: ['));
  assert.throws(()=>checkOutputDependencies(jobs),new RegExp(`${job} reads static outputs`));
 }
});
