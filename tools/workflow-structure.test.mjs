// CI graph and artifact contracts; these checks do not run native builds.
import { readFileSync, readdirSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import test from 'node:test';
import assert from 'node:assert/strict';

const repository = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const directory = join(repository, '.github/workflows');
const read = name => readFileSync(join(directory, name), 'utf8').replaceAll('\r\n', '\n');
const ci = read('ci.yml'), release = read('release.yml'), workflows = [ci, release];
const toolchain = JSON.parse(readFileSync(join(repository, 'release/tools/rust.json')));
const clean = text => text.split('\n').filter(line => !/^\s*#/.test(line)).join('\n');
function jobBodies(text) {
  const section = text.slice(text.indexOf('\njobs:\n') + '\njobs:\n'.length), jobs = new Map();
  for (const match of section.matchAll(/^  ([\w-]+):\n([\s\S]*?)(?=^  [\w-]+:\n|$(?![\s\S]))/gm)) jobs.set(match[1], clean(match[2]));
  assert(jobs.size, 'workflow must declare jobs'); return jobs;
}
const jobs = jobBodies(ci);
const needs = body => (/^    needs:\s*\[([^\]]*)\]/m.exec(body)?.[1] ?? '').split(',').map(name => name.trim()).filter(Boolean);
const steps = body => body.split(/^      - /m).slice(1);
const commands = body => body.replace(/`\n\s*/g, ' ');
function oneStep(body, pattern) {
  const matches = steps(body).filter(step => pattern.test(step)); assert.equal(matches.length, 1, String(pattern)); return matches[0];
}
function action(body, kind, artifact) {
  return oneStep(body, new RegExp(`actions/${kind}-artifact@[\\s\\S]*?^          name: ${artifact}$`, 'm'));
}
function requireNeeds(job, dependencies) {
  assert.deepEqual(needs(jobs.get(job)).sort(), [...dependencies].sort(), `${job} dependency graph`);
}
function directDependencies(map) {
  for (const [name, body] of map) for (const [, dependency] of body.matchAll(/needs\.([\w-]+)\.(?:outputs|result)\b/g)) {
    assert(needs(body).includes(dependency), `${name} reads ${dependency} without a direct needs dependency`);
  }
}
function verifiedBefore(body, manifest, required, command) {
  const text = commands(body), verify = `ci-artifacts.mjs verify --manifest ${manifest} --require ${required}`;
  assert(text.indexOf(verify) >= 0 && text.indexOf(verify) < text.indexOf(command), `${verify} must precede ${command}`);
}

test('only current workflows, immutable actions and read-only release permissions remain', () => {
  assert.deepEqual(readdirSync(directory).sort(), ['ci.yml', 'release.yml']);
  for (const text of workflows) {
    for (const [, reference] of clean(text).matchAll(/\buses: (\S+)/g)) if (!reference.startsWith('./')) assert.match(reference, /@[a-f0-9]{40}$/);
    assert.match(text, /contents: read/);
    assert.doesNotMatch(clean(text), /npm publish|gh release|id-token: write|NODE_AUTH_TOKEN|NPM_TOKEN|setup-go|go-version|CGO_ENABLED|\bgo (?:build|test|vet|run|mod)\b|tests\/rust-parity/);
    for (const body of jobBodies(text).values()) {
      assert.doesNotMatch(body, /continue-on-error/);
      if (body.includes('uses: ./.github/workflows/')) continue;
      assert.match(body, /runs-on: windows-2025/);
      assert.match(body, /TMP=\$RUNNER_TEMP/); assert.match(body, /TEMP=\$RUNNER_TEMP/);
    }
  }
});

test('PR triggers and documentation classification preserve the complete native path', () => {
  assert.match(ci, /push:\n\s+branches: \[main\]/);
  for (const event of ['pull_request', 'workflow_dispatch', 'workflow_call']) assert.match(ci, new RegExp(`^  ${event}:`, 'm'));
  const body = jobs.get('static');
  for (const command of ['ci-changes.mjs --github-output', 'ci-docs.mjs check', 'version.mjs --check', 'skill-contract.mjs', 'lua-tool-supply.mjs verify']) assert(body.includes(command));
  assert.match(body, /native: \$\{\{ steps.changes.outputs.native \}\}/);
  assert.doesNotMatch(body, /node tools\/(?:rust-build|luac)\.mjs|cargo (?:build|test|check)/);
  for (const [name, body] of jobs) if (!['static', 'ci-required'].includes(name)) assert.match(body, /^    if: needs.static.outputs.native == 'true'$/m, name);
});

test('addon and Rust validation do not wait for optimized Release', () => {
  assert.deepEqual([...jobs.keys()].sort(), ['static', 'prepare-resources', 'build-test-programs', 'build-release-native', 'build-release', 'validate-rust', 'validate-addon', 'validate-package', 'audit-package', 'release-ready', 'ci-required'].sort());
  requireNeeds('prepare-resources', ['static']);
  for (const job of ['build-test-programs', 'build-release-native', 'validate-addon']) requireNeeds(job, ['static', 'prepare-resources']);
  requireNeeds('validate-rust', ['static', 'build-test-programs', 'prepare-resources']);
  requireNeeds('build-release', ['static', 'build-release-native', 'prepare-resources']);
  requireNeeds('validate-package', ['static', 'build-release', 'build-test-programs']);
  requireNeeds('audit-package', ['static', 'build-release']);
  for (const text of workflows) directDependencies(jobBodies(text));
  const broken = new Map(jobs); broken.set('validate-addon', jobs.get('validate-addon').replace('needs: [static, ', 'needs: ['));
  assert.throws(() => directDependencies(broken), /validate-addon reads static/);
});

test('fixed tools are imported once and the addon ZIP is produced once', () => {
  const body = jobs.get('prepare-resources');
  assert.equal((ci.match(/node tools\/lua-tool-supply\.mjs import\b/g) ?? []).length, 1);
  assert.equal((ci.match(/node tools\/addon-package\.mjs build\b/g) ?? []).length, 1);
  assert.match(body, /lua-tool-supply.mjs import \.tmp\/ci\/build-release/);
  assert.match(body, /addon-package.mjs build --out \.tmp\/ci\/build-release\/addon/);
  for (const component of ['luac', 'lua51', 'addon-zip']) assert.match(body, new RegExp(`--component ${component}=`));
  assert.match(body, /ci-artifacts.mjs seal --out \.tmp\/ci\/build-release\/resources-manifest.json/);
  assert.doesNotMatch(body, /actions\/cache|cargo |rust-cargo.mjs|rust-build.mjs|ci-test-programs.mjs/);
  assert.doesNotMatch(ci, /node tools\/(?:luac|rust-formal-lua)\.mjs build/);
});

test('native configurations compile once with timing and explicit cached build identity', () => {
  for (const [name, command] of [['build-test-programs', 'ci-test-programs.mjs build'], ['build-release-native', 'rust-build.mjs']]) {
    const body = jobs.get(name), compile = oneStep(body, /id: compile/);
    assert.equal((ci.match(new RegExp(`node tools/${command.replaceAll('.', '\\.')}`, 'g')) ?? []).length, 1);
    assert.match(compile, /--jobs 2\b/); assert.match(compile, /--timings\b/);
    assert.match(compile, /LYCHEEDEV_BUILD_COMMIT: \$\{\{ github.sha \}\}/);
    assert.match(compile, /LYCHEEDEV_BUILD_DIRTY: 'false'/);
    assert(body.includes('python third_party/emmy/prepare.py --download'));
    assert(body.indexOf('python third_party/emmy/prepare.py --download') < body.indexOf(`node tools/${command}`));
    assert.match(body, /rust-cargo.mjs --owner \S+ -- fetch --locked/);
  }
  assert.match(jobs.get('build-test-programs'), /ci-test-programs.mjs build \.tmp\/ci\/build-test/);
  const native = jobs.get('build-release-native');
  assert.match(native, /rust-build.mjs \.tmp\/ci\/build-release\/cli release --one-shot/);
  assert.match(native, /rust-dependencies.mjs \.tmp\/ci\/build-release\/cli\/license-closure/);
  assert.doesNotMatch(native, /rust-package-smoke.mjs|rust-skill.mjs|addon-package.mjs/);
});

test('download and compilation caches have distinct identities and success rules', () => {
  const keys = [];
  for (const [name, profile] of [['build-test-programs', 'test'], ['build-release-native', 'release']]) {
    const body = jobs.get(name), dependencies = oneStep(body, /id: cargo-deps/), target = oneStep(body, /id: cargo-target/);
    for (const restore of [dependencies, target]) {
      assert.match(restore, /actions\/cache\/restore@/); assert(body.indexOf(restore) < body.indexOf('node tools/rust-cargo.mjs'));
    }
    assert.match(dependencies, /hashFiles\('Cargo.lock'\)/); assert.match(dependencies, /~\/\.cargo\/registry\/cache/);
    assert.doesNotMatch(dependencies, /path: target|\.tmp\/ci|credentials/);
    assert.match(target, /path: target/); assert.match(target, /restore-keys:/);
    const key = /^          key: (.*)$/m.exec(target)?.[1]; keys.push(key);
    assert(key.includes(`windows2025-${profile}-`) && key.includes('github.sha'));
    for (const input of ['Cargo.lock', 'Cargo.toml', '.cargo/config.toml', 'release/tools/rust.json']) assert(key.includes(input));
    const save = oneStep(body, /actions\/cache\/save@[\s\S]*path: target/);
    assert.match(save, /if: always\(\) && steps.compile.outcome == 'success' && steps.cargo-target.outputs.cache-hit != 'true'/);
    assert.match(save, /key: \$\{\{ steps.cargo-target.outputs.cache-primary-key \}\}/);
  }
  assert.notEqual(keys[0], keys[1]);
  for (const [name, body] of jobs) if (!['build-test-programs', 'build-release-native'].includes(name)) assert.doesNotMatch(body, /actions\/cache\//, name);
});

test('assembly verifies preserved native and resource inputs before producing the unique TGZ', () => {
  const body = jobs.get('build-release');
  for (const artifact of ['ci-build-release-native', 'ci-resources']) assert.match(action(body, 'download', artifact), /path: \.tmp\/ci\/build-release/);
  verifiedBefore(body, '.tmp/ci/build-release/native-manifest.json', 'cli-release', 'node tools/rust-skill.mjs');
  verifiedBefore(body, '.tmp/ci/build-release/resources-manifest.json', 'luac,lua51,addon-zip', 'node tools/rust-package-smoke.mjs produce');
  const produce = oneStep(body, /node tools\/rust-package-smoke.mjs produce\b/);
  assert.equal((ci.match(/node tools\/rust-package-smoke.mjs produce\b/g) ?? []).length, 1);
  assert.match(produce, /--artifact \.tmp\/ci\/build-release\/cli\/artifact.json --profile release/);
  assert.match(produce, /--dependency-closure \.tmp\/ci\/build-release\/cli\/license-closure/);
  for (const kind of ['cli-release', 'luac', 'lua51', 'addon-zip', 'skill', 'npm-package']) assert.match(body, new RegExp(`--component ${kind}=`));
});

test('validation verifies the producer manifests before execution', () => {
  const rust = jobs.get('validate-rust'), addon = jobs.get('validate-addon'), pkg = jobs.get('validate-package');
  assert.match(action(rust, 'download', 'ci-build-test'), /path: \.tmp\/ci\/build-test/);
  for (const body of [rust, addon]) assert.match(action(body, 'download', 'ci-resources'), /path: \.tmp\/ci\/build-release/);
  verifiedBefore(rust, '.tmp/ci/build-test/build-manifest.json', 'test-programs', 'ci-test-programs.mjs run');
  verifiedBefore(rust, '.tmp/ci/build-release/resources-manifest.json', 'lua51', 'ci-test-programs.mjs run');
  assert.match(rust, /--formal-lua \.tmp\/ci\/build-release\/lua51\/build-report.json/);
  verifiedBefore(addon, '.tmp/ci/build-release/resources-manifest.json', 'lua51,addon-zip', 'tests/addon/run-all.mjs');
  assert.match(addon, /addon-package.mjs verify --zip/);
  assert.match(action(pkg, 'download', 'ci-build-release'), /path: \.tmp\/ci\/build-release/);
  verifiedBefore(pkg, '.tmp/ci/build-release/build-manifest.json', 'cli-release,luac,skill,npm-package', 'rust-package-smoke.mjs validate');
  assert.match(pkg, /skill-contract.mjs --skill-root \S+ --bin \.tmp\/ci\/build-release\/cli\/lycheedev.exe/);
});

test('package install and business checks consume the producer TGZ and precompiled seed', () => {
  const body = jobs.get('validate-package');
  assert.match(body, /rust-package-smoke.mjs validate \.tmp\/ci\/validate-package\/npm --package-report \.tmp\/ci\/build-release\/npm-package\/report.json/);
  assert.match(action(body, 'download', 'ci-build-test'), /path: \.tmp\/ci\/build-test/);
  verifiedBefore(body, '.tmp/ci/build-test/build-manifest.json', 'test-programs', 'ci-package-business.mjs');
  assert.match(body, /ci-package-business.mjs --npm-report \.tmp\/ci\/validate-package\/npm\/report.json --test-record \.tmp\/ci\/build-test\/test-programs.json --out \.tmp\/ci\/validate-package\/business/);
  const upload = action(body, 'upload', 'ci-validate-package');
  assert.match(upload, /\.tmp\/ci\/validate-package\/npm\/report.json/); assert.match(upload, /\.tmp\/ci\/validate-package\/business/);
  assert.doesNotMatch(upload, /^\s+\.tmp\/ci\/validate-package\/?\s*$/m, 'upload reports rather than another node_modules tree');
});

test('independent audit and release candidate share producer and validation identities', () => {
  const audit = jobs.get('audit-package'), ready = jobs.get('release-ready');
  action(audit, 'download', 'ci-build-release');
  assert.match(audit, /Get-Content \$producer \| ConvertFrom-Json/);
  assert.match(audit, /Join-Path \(Split-Path \$producer\) \$report.tgz/);
  assert.match(audit, /release.mjs audit-tgz --tgz \$tgz --version \$report.packageVersion --commit \$report.commit/);
  for (const name of ['build-test-programs', 'build-release', 'validate-rust', 'validate-addon', 'validate-package', 'audit-package']) assert(needs(ready).includes(name));
  for (const artifact of ['ci-build-test', 'ci-build-release', 'ci-validate-rust', 'ci-validate-addon', 'ci-validate-package', 'ci-audit-package']) action(ready, 'download', artifact);
  const candidate = commands(oneStep(ready, /ci-release.mjs candidate/));
  assert.match(candidate, /--build-manifest \.tmp\/ci\/build-release\/build-manifest.json/);
  assert.match(candidate, /--test-manifest \.tmp\/ci\/build-test\/build-manifest.json/);
  assert.match(candidate, /--npm-report \.tmp\/ci\/validate-package\/npm\/report.json/);
  for (const report of ['rust', 'addon-lua', 'addon-zip', 'skill', 'package-audit', 'package-business']) assert.match(candidate, new RegExp(`--report ${report}=`));
  action(ready, 'upload', 'ci-release-candidate');
});

test('consumers never compile, resolve dependencies, rebuild Lua or reproduce packages', () => {
  const forbidden = /rust-build.mjs|rust-dependencies.mjs|\bbuildRust\b|cargo (?:build|test|check|clippy|run|fetch|metadata)\b|rust-cargo.mjs|(?:luac|rust-formal-lua).mjs build|npm pack\b/;
  for (const name of ['build-release', 'validate-rust', 'validate-addon', 'validate-package', 'audit-package', 'release-ready']) {
    const body = jobs.get(name); assert.doesNotMatch(body, forbidden, name);
    if (name !== 'build-release') assert.doesNotMatch(body, /rust-package-smoke.mjs produce|addon-package.mjs build|lua-tool-supply.mjs import/);
  }
  assert.match(release, /uses: \.\/\.github\/workflows\/ci.yml/);
  assert.match(release, /ci-release.mjs seal --candidate \.tmp\/ci\/release-candidate --identity \S+ --out \.tmp\/ci\/release-sealed/);
  assert.doesNotMatch(clean(release), forbidden);
});

test('finished outputs and reports survive failures without hiding job failures', () => {
  for (const [name, artifact] of [['prepare-resources', 'ci-resources'], ['build-test-programs', 'ci-build-test'], ['build-release-native', 'ci-build-release-native'], ['build-release', 'ci-build-release'], ['validate-rust', 'ci-validate-rust'], ['validate-addon', 'ci-validate-addon'], ['validate-package', 'ci-validate-package'], ['audit-package', 'ci-audit-package']]) {
    const upload = action(jobs.get(name), 'upload', artifact);
    assert.match(upload, /^        if: always\(\)$/m, name); assert.doesNotMatch(upload, /overwrite:/);
  }
  for (const text of workflows) for (const body of jobBodies(text).values()) for (const step of steps(body).filter(step => step.includes('actions/upload-artifact@'))) assert.match(step, /if-no-files-found: error/);
});

test('failed or cancelled native compilation uploads owned evidence even when no artifact directory was produced', () => {
  for (const name of ['build-test-programs', 'build-release-native']) {
    const body = jobs.get(name), diagnostics = oneStep(body, /actions\/upload-artifact@[\s\S]*path: \.tmp\/rust-disk\/evidence/);
    assert.match(diagnostics, /^        if: failure\(\) \|\| cancelled\(\)$/m);
    assert.match(diagnostics, /if-no-files-found: error/);
    assert.doesNotMatch(diagnostics, /steps\.compile\.outcome|cache-hit|continue-on-error|overwrite:/);
    assert(body.indexOf(diagnostics) > body.indexOf('id: compile'));
  }
});

test('the npm version probe and exported Node execution path refer to the same installed CLI', { skip: process.platform !== 'win32' }, () => {
  const literal = value => `'${value.replaceAll("'", "''")}'`, script = [], expected = [];
  const npmCli = 'C:\\global npm\\node_modules\\npm\\bin\\npm-cli.js';
  for (const job of ['build-release', 'validate-package']) {
    const body = jobs.get(job), step = oneStep(body, /npm install --global npm@/);
    const version = /npm install --global npm@(\d+\.\d+\.\d+)/.exec(step)?.[1]; assert(version);
    const pin = step.split('        run: |\n')[1].split('\n').map(line => line.startsWith('          ') ? line.slice(10) : line).join('\n');
    assert(body.indexOf(step) < body.indexOf('node tools/rust-package-smoke.mjs'));
    for (const fixture of [{name: 'pinned', version, install: 0, root: 0, probe: 0, accepted: true},
      {name: 'bundled-drift', version: '0.0.0', install: 0, root: 0, probe: 0, accepted: false},
      {name: 'install-failed', version, install: 1, root: 0, probe: 0, accepted: false},
      {name: 'root-failed', version, install: 0, root: 1, probe: 0, accepted: false},
      {name: 'probe-failed', version, install: 0, root: 0, probe: 1, accepted: false}]) {
      // Execute the actual workflow block with commands replaced by pure
      // functions. No npm installation, product process or output file runs.
      script.push(`$script:emitted='';$env:GITHUB_ENV='mock-output';
function npm { if($args[0] -eq 'install'){$global:LASTEXITCODE=${fixture.install};return};if($args[0] -eq 'root'){$global:LASTEXITCODE=${fixture.root};return 'C:\\global npm\\node_modules'};throw 'unexpected npm invocation' }
function node { if($args[0] -cne ${literal(npmCli)} -or $args[1] -ne '--version'){throw 'version probe used another npm CLI'};$global:LASTEXITCODE=${fixture.probe};return ${literal(fixture.version)} }
function Out-File { param([Parameter(ValueFromPipeline=$true)]$InputObject,[string]$FilePath,[string]$Encoding,[switch]$Append) process{$script:emitted=$InputObject} }
$accepted=$true;try{${pin}}catch{$accepted=$false};[Console]::Out.WriteLine((${literal(`${job}:${fixture.name}:`)}+$accepted+':'+$script:emitted))`);
      expected.push(`${job}:${fixture.name}:${fixture.accepted ? 'True:npm_execpath=' + npmCli : 'False:'}`);
    }
  }
  const bootstrap = '& ([ScriptBlock]::Create([Console]::In.ReadToEnd()))';
  const result = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-EncodedCommand', Buffer.from(bootstrap, 'utf16le').toString('base64')], {input: script.join('\n'), encoding: 'utf8', windowsHide: true, shell: false, timeout: 10000});
  assert.equal(result.error, undefined); assert.equal(result.status, 0, result.stderr);
  assert.deepEqual(result.stdout.trim().split(/\r?\n/), expected);
  const wrapper = readFileSync(join(repository, 'tools/rust-package-smoke.mjs'), 'utf8');
  assert.match(wrapper, /const npm = \(args, cwd\) => run\(process\.execPath, \[process\.env\.npm_execpath \|\|/);
});

test('Node and Rust setup commands agree with the declared fixed toolchains', () => {
  const node = /^  NODE_VERSION: '([^']+)'$/m.exec(ci)?.[1];
  assert.match(node, /^\d+\.\d+\.\d+$/);
  for (const text of workflows) {
    assert.equal(/^  NODE_VERSION: '([^']+)'$/m.exec(text)?.[1], node);
    for (const [, version] of text.matchAll(/node-version: '([^']+)'/g)) assert.equal(version, node);
    for (const [, version] of text.matchAll(/^\s+rustup (?:toolchain install|override set) (\S+)/gm)) assert.equal(version, toolchain.rustc);
  }
});

test('actual PowerShell toolchain checks accept multiline output and reject drift or command failure', { skip: process.platform !== 'win32' }, () => {
  const version = toolchain.rustc, checked = [...jobs].filter(([, body]) => body.includes('$version = rustc -Vv'));
  assert.deepEqual(checked.map(([name]) => name).sort(), ['static', 'build-test-programs', 'build-release-native'].sort());
  const staticJob = jobs.get('static');
  const install = staticJob.indexOf(`rustup toolchain install ${version} --profile minimal`);
  const select = staticJob.indexOf(`rustup override set ${version}`), tests = staticJob.indexOf('node --test');
  assert(install >= 0 && select > install && tests > select);
  assert.match(staticJob, /Get-Command cargo -CommandType Application/); assert.match(staticJob, /Get-FileHash -LiteralPath \$cargo -Algorithm SHA256/);
  const literal = value => `'${value.replaceAll("'", "''")}'`, script = [], expected = [];
  const lines = ['rustc example', 'host: x86_64-pc-windows-msvc', `release: ${version}`, 'LLVM version: example'];
  const cases = [{ lines, exit: 0, rejected: false }, { lines, exit: 1, rejected: true },
    ...[version + '1', version + '-nightly', '1.98.0'].map(other => ({ lines: [...lines.filter(line => !line.startsWith('release:')), `release: ${other}`], exit: 0, rejected: true })),
    { lines: ['note: release: ' + version], exit: 0, rejected: true }, { lines: [], exit: 0, rejected: true }];
  for (const [job, body] of checked) {
    const condition = /^\s*if \(\$LASTEXITCODE[^\n]*\$version[^\n]*\) \{ throw [^\n]*\}$/m.exec(body)?.[0].trim(); assert(condition, job);
    for (const [i, fixture] of cases.entries()) {
      script.push(`$version=@(${fixture.lines.map(literal).join(',')});$LASTEXITCODE=${fixture.exit};$rejected=$false;try{${condition}}catch{$rejected=$true};[Console]::Out.WriteLine((${literal(`${job}:${i}:`)}+$rejected))`);
      expected.push(`${job}:${i}:${fixture.rejected ? 'True' : 'False'}`);
    }
  }
  const result = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-EncodedCommand', Buffer.from(script.join('\n'), 'utf16le').toString('base64')], { encoding: 'utf8', windowsHide: true, shell: false, timeout: 10000 });
  assert.equal(result.error, undefined); assert.equal(result.status, 0, result.stderr); assert.deepEqual(result.stdout.trim().split(/\r?\n/), expected);
});

test('required aggregations observe every job and reject failures, cancellation and skips', () => {
  for (const [text, aggregator] of [[ci, 'ci-required'], [release, 'release-required']]) {
    const map = jobBodies(text), body = map.get(aggregator), required = [...map.keys()].filter(name => name !== aggregator);
    assert.deepEqual(needs(body).sort(), required.sort()); assert.match(body, /^    if: always\(\)$/m);
    for (const name of required) assert(body.includes(`needs.${name}.result`), `${aggregator} must observe ${name}`);
    assert.match(body, /-ne 'success'/); assert.match(body, /throw /);
  }
  const body = jobs.get('ci-required');
  for (const job of [...jobs.keys()].filter(name => !['static', 'ci-required'].includes(name))) assert.match(body, new RegExp(`'${job}' = \\$env:`));
  assert.match(body, /NATIVE: \$\{\{ needs.static.outputs.native \}\}/);
  assert(body.indexOf("if ($env:STATIC_RESULT -ne 'success')") < body.indexOf("if ($env:NATIVE -ne 'true')"));
  assert.match(body, /if \(\$entry.Value -ne 'success'\)/);
});

test('actual required-job PowerShell accepts only complete native success or successful documentation checks', { skip: process.platform !== 'win32' }, () => {
  const body = jobs.get('ci-required');
  const step = oneStep(body, /STATIC_RESULT: \$\{\{ needs.static.result \}\}/);
  const script = step.split('        run: |\n')[1].split('\n').map(line => line.startsWith('          ') ? line.slice(10) : line).join('\n');
  const mappings = [...step.matchAll(/^          ([A-Z_]+): \$\{\{ needs\.([\w-]+)\.result \}\}$/gm)];
  assert.equal(mappings.length, needs(body).length);
  const success = Object.fromEntries(mappings.map(([, variable]) => [variable, 'success']));
  const cases = [{ name: 'complete', env: success, native: 'true', accepted: true },
    ...mappings.map(([, variable, job]) => ({ name: job, env: { ...success, [variable]: 'failure' }, native: 'true', accepted: false })),
    ...['cancelled', 'skipped'].map(result => ({ name: result, env: { ...success, RELEASE_READY_RESULT: result }, native: 'true', accepted: false })),
    { name: 'docs', env: { ...Object.fromEntries(mappings.map(([, variable]) => [variable, 'skipped'])), STATIC_RESULT: 'success' }, native: 'false', accepted: true },
    { name: 'docs-static-failure', env: { ...success, STATIC_RESULT: 'failure' }, native: 'false', accepted: false }];
  for (const fixture of cases) {
    const result = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-EncodedCommand', Buffer.from(script, 'utf16le').toString('base64')], {
      env: { ...process.env, ...fixture.env, NATIVE: fixture.native }, encoding: 'utf8', windowsHide: true, shell: false, timeout: 5000,
    });
    assert.equal(result.error, undefined, fixture.name);
    assert.equal(result.status === 0, fixture.accepted, `${fixture.name}: ${result.stderr}`);
  }
});
