import test from 'node:test';
import assert from 'node:assert/strict';
import { assertPackageReports } from './ci-release.mjs';

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
