// Structural pin for the release pipelines (regression ARC/CI): the job graph,
// needs edges, hosted-runner scope and platform scope of the workflow
// files are asserted literally. A YAML/Actions mistake that changes the shape
// fails here before any runner burns minutes. Line-based on purpose: these
// files are hand-maintained and every load-bearing line is a single constant.
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import assert from 'node:assert/strict';

import { resolve } from 'node:path';
const repository = resolve(dirname(fileURLToPath(import.meta.url)), '..');

const release = readFileSync(join(repository, '.github/workflows/toolkit-release.yml'), 'utf8');
const ci = readFileSync(join(repository, '.github/workflows/toolkit-ci.yml'), 'utf8');

function jobsOf(text) {
  const jobs = {};
  let current = null;
  let section = '';
  for (const line of text.split('\n')) {
    const sectionKey = line.match(/^([a-zA-Z]+):$/);
    if (sectionKey) { section = sectionKey[1]; current = null; continue; }
    const job = section === 'jobs' && line.match(/^  ([a-zA-Z][a-zA-Z0-9_-]*):$/);
    if (job) { current = job[1]; jobs[current] = { steps: 0, usesJob: false }; continue; }
    if (!current) continue;
    if (line === '    steps:') jobs[current].steps += 1;
    if (new RegExp(`^    uses: \\./\\.github/workflows/`).test(line)) jobs[current].usesJob = true;
  }
  return jobs;
}

test('release pipeline job graph: identity -> ci/cgo -> assemble -> smoke -> seal -> gate -> publish', () => {
  const jobs = jobsOf(release);
  assert.deepEqual(Object.keys(jobs), [
    'identity', 'ci', 'verify-cgo', 'assemble', 'platform-smoke',
    'seal', 'release-gate', 'publish', 'release-complete',
  ]);
  for (const [name, job] of Object.entries(jobs)) {
    if (name === 'ci') assert.ok(job.usesJob, 'ci must call the shared workflow');
    else assert.ok(job.steps === 1, `job ${name} must have exactly one steps key, found ${job.steps}`);
  }
  assert.match(release, /  release-gate:\n[\s\S]*?needs: \[identity, ci, seal\]/);
  assert.match(release, /  publish:\n[\s\S]*?needs: \[identity, release-gate\]/);
  assert.match(release, /  seal:\n[\s\S]*?needs: \[identity, assemble, platform-smoke\]/);
});

test('release has no interactive desktop job or desktop evidence gate', () => {
  assert.equal(existsSync(join(repository, '.github/workflows/toolkit-desktop.yml')), false);
  assert.doesNotMatch(release, /^  desktop:$/m);
  assert.doesNotMatch(release, /self-hosted|desktop-evidence|verify-desktop-evidence/);
});

test('release publishes the sealed tarball via OIDC with no token fallback', () => {
  assert.match(release, /npm publish "\.\/out\/lycheedev-\$PUBLISH_VERSION\.tgz" --access public --tag "\$PUBLISH_DIST_TAG"/);
  const publishCode = release
    .slice(release.indexOf('  publish:'), release.indexOf('  release-complete:'))
    .split('\n')
    .filter(line => !line.trim().startsWith('#'))
    .join('\n');
  assert.doesNotMatch(publishCode, /authToken|NODE_AUTH_TOKEN\s*[=:]|NPM_TOKEN\s*[=:]/);
  assert.match(release, /environment: npm/);
  assert.match(release, /id-token: write/);
  assert.match(publishCode, /contents: write/);
  assert.match(publishCode, /GH_TOKEN: \$\{\{ github\.token \}\}/);
  assert.match(publishCode, /for attempt in \$\(seq 1 60\)/);
  assert.match(publishCode, /\[ "\$visible" = true \]/);
});

test('windows amd64 is the only shipped platform', () => {
  for (const [name, text] of [['toolkit-release.yml', release], ['toolkit-ci.yml', ci]]) {
    assert.doesNotMatch(text, /darwin|macos-|ubuntu-24\.04-arm|linux-arm64|linux-amd64/, `${name} references removed platforms`);
  }
  assert.match(release, /SMOKE_TARGET: windows-amd64/);
  assert.doesNotMatch(release, /for target in windows-amd64 linux-amd64/);
});

test('manual dry-run writes release identity outside the checkout', () => {
  assert.match(release, /fs\.writeFileSync\(require\('path'\)\.join\(process\.env\.RUNNER_TEMP, 'identity\.json'\)/);
  assert.doesNotMatch(release, /fs\.writeFileSync\('identity\.json'/);
});

test('ci gate: required windows jobs aggregate strictly and stay callable', () => {
  assert.match(ci, /workflow_call:/);
  for (const job of ['windows-contract', 'windows-process', 'windows-addon', 'windows-package', 'race', 'ci-required']) {
    assert.match(ci, new RegExp(`^  ${job}:$`, 'm'), `missing required job ${job}`);
  }
  assert.doesNotMatch(ci, /^  cross-platform-contract:$/m);
  for (const job of ['windows-contract', 'windows-process', 'windows-addon', 'windows-package', 'race']) {
    assert.match(ci, new RegExp(`^      - ${job}$`, 'm'), `ci-required must need ${job}`);
  }
  assert.doesNotMatch(ci, /      - cross-platform-contract/);
});

test('every windows job normalizes TMP before any test runs', () => {
  for (const [name, text, skip] of [
    ['toolkit-ci.yml', ci, ['ci-required']],
    ['toolkit-release.yml', release, ['ci']],
  ]) {
    const jobs = jobsOf(text);
    for (const job of Object.keys(jobs)) {
      if ((skip ?? []).includes(job) || jobs[job].usesJob) continue;
      const body = text.slice(text.indexOf(`  ${job}:\n`));
      assert.match(body, /Normalize TMP to the long runner temp path/, `job ${job} in ${name} lacks TMP normalization`);
    }
  }
});
