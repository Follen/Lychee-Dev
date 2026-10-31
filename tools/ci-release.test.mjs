import test from 'node:test';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import fs from 'node:fs';
import {existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync} from 'node:fs';
import {syncBuiltinESMExports} from 'node:module';
import {join, resolve, sep} from 'node:path';
import { assertPackageReports, assertRustCoverage, assertValidationReports, sealRelease } from './ci-release.mjs';
import { CLIENTS, discoverSuites } from '../tests/addon/run-all.mjs';

function sealedFixture(t) {
  const base = resolve(import.meta.dirname, '../.tmp/rust-audit-next');
  mkdirSync(base, {recursive: true});
  const directory = mkdtempSync(join(base, 'seal-'));
  t.after(() => {
    assert(directory.startsWith(base + sep));
    rmSync(directory, {recursive: true, force: true});
  });
  const candidate = join(directory, 'candidate'), output = join(directory, 'sealed');
  mkdirSync(join(candidate, 'reports'), {recursive: true});
  const digest = bytes => createHash('sha256').update(bytes).digest('hex');
  const record = (name, bytes) => ({name, bytes: bytes.length, sha256: digest(bytes)});
  const tgz = Buffer.from('the original validated package'), addon = Buffer.from('the validated addon');
  const report = Buffer.from('{"state":"passed"}\n');
  const manifest = {schema: 'lycheedev.ci-release-candidate.v1', version: '3.5.0', tag: 'v3.5.0',
    commit: 'a'.repeat(40), distTag: 'beta', sourceTree: 'b'.repeat(64), publication: 'not_performed',
    artifacts: {npmTgz: record('lycheedev-3.5.0.tgz', tgz), addonZip: record('lycheedev-3.5.0-addon.zip', addon)},
    validation: {reports: [record('package-business', report)]}, rust: {rustc: 'fixed'}};
  writeFileSync(join(candidate, manifest.artifacts.npmTgz.name), tgz);
  writeFileSync(join(candidate, manifest.artifacts.addonZip.name), addon);
  writeFileSync(join(candidate, 'reports/package-business'), report);
  writeFileSync(join(candidate, 'release-manifest.json'), JSON.stringify(manifest));
  const identity = join(directory, 'identity.json');
  writeFileSync(identity, JSON.stringify({schema: 'lycheedev.ci-release-identity.v1', version: manifest.version,
    tag: manifest.tag, commit: manifest.commit, distTag: manifest.distTag, tagVerified: true}));
  const names = ['release-manifest.json', ...Object.values(manifest.artifacts).map(row => row.name), 'reports/package-business'];
  const sums = (selected = names) => writeFileSync(join(candidate, 'SHA256SUMS'),
    selected.map(name => `${digest(readFileSync(join(candidate, name)))}  ${name}\n`).join(''));
  sums();
  return {candidate, output, identity, manifest, names, sums};
}

test('tag sealing preserves all manifest-declared package and validation bytes', t => {
  const fx = sealedFixture(t);
  sealRelease(fx);
  for (const name of fx.names) assert.deepEqual(readFileSync(join(fx.output, name)), readFileSync(join(fx.candidate, name)));
});

for (const name of ['lycheedev-3.5.0.tgz', 'reports/package-business']) {
  test(`tag sealing rejects substituted ${name} even with regenerated SHA256SUMS`, t => {
    const fx = sealedFixture(t);
    writeFileSync(join(fx.candidate, name), 'substituted bytes');
    fx.sums();
    assert.throws(() => sealRelease(fx), /ci-release\.sealed_file_(identity|changed)/);
    assert.equal(existsSync(fx.output), false);
  });
}

test('tag sealing requires every declared validation report in SHA256SUMS', t => {
  const fx = sealedFixture(t);
  fx.sums(fx.names.filter(name => !name.startsWith('reports/')));
  assert.throws(() => sealRelease(fx), /ci-release\.sealed_file_unlisted/);
  assert.equal(existsSync(fx.output), false);
});

test('tag sealing cannot change the channel frozen by the tag identity', t => {
  const fx = sealedFixture(t);
  fx.manifest.distTag = 'latest';
  writeFileSync(join(fx.candidate, 'release-manifest.json'), JSON.stringify(fx.manifest));
  fx.sums();
  assert.throws(() => sealRelease(fx), /ci-release\.candidate_identity_mismatch/);
  assert.equal(existsSync(fx.output), false);
});

test('tag sealing verifies copied bytes before writing the sealed manifest', t => {
  const fx = sealedFixture(t), original = fs.copyFileSync;
  const mocked = t.mock.method(fs, 'copyFileSync', (source, target, ...args) => {
    original(source, target, ...args);
    if (source.endsWith('.tgz')) writeFileSync(target, 'changed during copy');
  });
  syncBuiltinESMExports();
  t.after(() => { mocked.mock.restore(); syncBuiltinESMExports(); });
  assert.throws(() => sealRelease(fx), /ci-release\.sealed_file_copy_changed/);
  assert.equal(existsSync(join(fx.output, 'release-sealed.json')), false);
});

test('tag sealing rejects repeated sum entries and paths outside the candidate', t => {
  for (const extra of ['lycheedev-3.5.0.tgz', '../identity.json']) {
    const fx = sealedFixture(t);
    fx.sums([...fx.names, extra]);
    assert.throws(() => sealRelease(fx), /ci-release\.sealed_(duplicate|unexpected)_file/);
    assert.equal(existsSync(fx.output), false);
  }
});

test('candidate requires complete Rust coverage for the same built program record', () => {
  const identity = {commit: 'a'.repeat(40), sourceTree: 'b'.repeat(64), recordSHA256: 'c'.repeat(64), programs: 93};
  const report = {schema: 'lycheedev.ci-test-run.v1', commit: identity.commit, sourceTree: identity.sourceTree,
    testRecordSHA256: identity.recordSHA256, state: 'passed', handlesReaped: true,
    programs: 93, passed: 93, failed: 0, coverage: {complete: true, expectedPrograms: 93, observedPrograms: 93, shards: 2}};
  assert.doesNotThrow(() => assertRustCoverage(report, identity));
  for (const key of ['commit', 'sourceTree', 'testRecordSHA256']) {
    assert.throws(() => assertRustCoverage({...report, [key]: 'd'.repeat(64)}, identity), /rust_report_identity/);
  }
  for (const change of [
    value => { value.state = 'failed'; }, value => { value.handlesReaped = false; },
    value => { value.passed = 92; value.failed = 1; }, value => { value.programs = 92; },
    value => { value.coverage.complete = false; }, value => { value.coverage.observedPrograms = 92; },
    value => { value.coverage.shards = 1; }, value => { delete value.coverage; },
  ]) { const value = structuredClone(report); change(value); assert.throws(() => assertRustCoverage(value, identity), /rust_coverage_incomplete/); }
});

function fixture() {
  const tgzSHA256 = '1'.repeat(64), binarySHA256 = '2'.repeat(64), productionSHA256 = '3'.repeat(64), installationSHA256 = '4'.repeat(64);
  return { productionSHA256, installationSHA256,
    production: {schema: 'lycheedev.rust-npm-package.v1', state: 'packed', tgzSHA256, binarySHA256},
    installation: {tgzSHA256, packageReportSHA256: productionSHA256},
    audit: {ok: true, violations: [], tgzSHA256},
    business: {state: 'passed', source: {state: 'passed'}, data: {state: 'passed'}, inputsStable: true, handlesReaped: true,
      tgzSHA256, binarySHA256, packageReportSHA256: productionSHA256, npmReportSHA256: installationSHA256},
  };
}
test('candidate accepts only one TGZ identity across production, install, audit and business checks', () => {
  assert.doesNotThrow(() => assertPackageReports(fixture()));
  for (const key of ['installation', 'audit', 'business']) {
    const value = fixture(); value[key].tgzSHA256 = '9'.repeat(64);
    assert.throws(() => assertPackageReports(value), /package_tgz_mismatch/, key);
  }
});
test('candidate rejects substituted production, installation or binary evidence', () => {
  for (const [section, field] of [['installation', 'packageReportSHA256'], ['business', 'packageReportSHA256'],
    ['business', 'npmReportSHA256'], ['business', 'binarySHA256']]) {
    const value = fixture(); value[section][field] = '9'.repeat(64);
    assert.throws(() => assertPackageReports(value), /mismatch|business_identity/, `${section}.${field}`);
  }
});
test('partial, missing and unreaped package business checks cannot seal a candidate', () => {
  for (const change of [
    value => { value.business.source.state = 'not_run'; },
    value => { value.business.data.state = 'failed'; },
    value => { value.business.state = 'partial_passed'; },
    value => { value.business.inputsStable = false; },
    value => { value.business.handlesReaped = false; },
    value => { delete value.business; },
    value => { value.audit.ok = false; },
    value => { value.audit.violations.push('unlisted payload'); },
    value => { value.production.state = 'assembled'; },
  ]) { const value = fixture(); change(value); assert.throws(() => assertPackageReports(value)); }
});

function validationFixture() {
  const packages = fixture(), suites = discoverSuites(), lua51SHA256 = 'e'.repeat(64);
  const testIdentity = {commit: 'a'.repeat(40), sourceTree: 'b'.repeat(64), recordSHA256: 'c'.repeat(64), programs: 93};
  const rust = {schema: 'lycheedev.ci-test-run.v1', commit: testIdentity.commit, sourceTree: testIdentity.sourceTree,
    testRecordSHA256: testIdentity.recordSHA256, state: 'passed', handlesReaped: true, programs: 93, passed: 93, failed: 0,
    coverage: {complete: true, expectedPrograms: 93, observedPrograms: 93, shards: 2}};
  const cases = CLIENTS.flatMap(client => suites.map(file => ({file, client, state: 'passed'})));
  const reports = new Map([
    ['rust', rust], ['package-audit', packages.audit], ['package-business', packages.business],
    ['addon-lua', {schema: 'lycheedev.addon-lua-regression.v1', suites: suites.length, clients: [...CLIENTS],
      counts: {passed: cases.length, failed: 0}, cases, interpreter: {executableSHA256: lua51SHA256}}],
    ['addon-zip', {ok: true, violations: [], files: ['Lychee Dev/Lychee Dev.toc']}],
    ['skill', {checkKind: 'static-reference-consistency', files: 21, contracts: 68, references: 30, violations: []}],
  ]);
  return {reports, identity: {testIdentity, packageIdentity: packages, lua51SHA256}};
}

test('candidate rejects failed addon and Skill validations even when all named reports are present', () => {
  const positive = validationFixture();
  assert.doesNotThrow(() => assertValidationReports(positive.reports, positive.identity));
  for (const [section, change] of [
    ['addon-lua', value => { value.counts.failed = 1; value.cases[0].state = 'failed'; }],
    ['addon-zip', value => { value.ok = false; value.violations.push('missing bindings'); }],
    ['skill', value => { value.violations.push('unknown command'); }],
  ]) {
    const {reports, identity} = validationFixture(); change(reports.get(section));
    assert.throws(() => assertValidationReports(reports, identity), /ci-release\.(addon|skill)/, section);
  }
});

test('candidate requires the complete current addon matrix and the sealed Lua interpreter', () => {
  for (const change of [
    value => { value.cases.pop(); value.counts.passed--; },
    value => { value.cases[1] = {...value.cases[0]}; },
    value => { value.clients.pop(); },
    value => { value.suites--; },
    value => { value.interpreter.executableSHA256 = 'f'.repeat(64); },
  ]) {
    const {reports, identity} = validationFixture(); change(reports.get('addon-lua'));
    assert.throws(() => assertValidationReports(reports, identity), /ci-release\.addon/);
  }
});
