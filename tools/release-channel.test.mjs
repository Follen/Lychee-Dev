import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {auditTgz, distTagFor, policyFor, validateNpmChannel, verifyNpmChannelTags} from './release.mjs';

const authority = JSON.parse(readFileSync(new URL('../release/npm-channel.json', import.meta.url), 'utf8'));

test('release selection uses the reviewed authority and has no unbound-version fallback', () => {
  assert.equal(distTagFor(authority.version), authority.distTag);
  assert.equal(policyFor(authority.version).npmDistTag, authority.distTag);
  assert.equal(policyFor('9.0.0-dev').npmDistTag, null);
  for (const unbound of ['3.5.1', '9.0.0', '9.0.0-rc.1']) {
    if (unbound !== authority.version) assert.throws(() => distTagFor(unbound), /invalid_npm_channel/);
  }
});

test('channel shape permits a next-version binding without a version/tag table', () => {
  const candidate = {...authority, version: '9.0.0'};
  assert.equal(validateNpmChannel(candidate, '9.0.0'), candidate);
  assert.throws(() => validateNpmChannel(candidate, authority.version), /invalid_npm_channel/);
  for (const distTag of ['latest', '', '1.2.3', 'bad/tag', 'BETA']) {
    assert.throws(() => validateNpmChannel({...candidate, distTag}, candidate.version), /invalid_npm_channel/);
  }
});

test('package audit requires the original reviewed channel fields', () => {
  const check = npmChannel => auditTgz([{name:'package/release.json', bytes:Buffer.from(JSON.stringify({
    schema:'lycheedev.release.v1', runtime:'rust', version:authority.version, commit:'a'.repeat(40), npmChannel,
  }))}], {version:authority.version}).violations.filter(value => value.includes('npmChannel'));
  assert.deepEqual(check(authority), []);
  for (const changed of [
    {...authority, distTag:authority.distTag === 'next' ? 'beta' : 'next'},
    {...authority, preserveLatest:'0.0.1'},
    {...authority, version:'9.0.0'},
  ]) assert.equal(check(changed).length, 1);
});

test('registry readback preserves the stable tag before and after promotion', () => {
  const tags = {latest:authority.preserveLatest, [authority.distTag]:authority.version};
  for (const phase of ['before', 'after']) {
    assert.equal(verifyNpmChannelTags(tags, authority, authority.version, authority.distTag, phase).ok, true);
    assert.throws(() => verifyNpmChannelTags({...tags, latest:authority.version}, authority,
      authority.version, authority.distTag, phase), /npm_channel_mismatch/);
  }
});
