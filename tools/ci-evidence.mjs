// Reuse the full-suite invocation without weakening the named CI checks.
import { readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { spawnSync } from 'node:child_process';
import { createCoverageReport } from './go-coverage.mjs';
import { modulePath, options, repository } from './baseline-common.mjs';

export const groups = {
  process: {
    'tests/process': ['TestNativeWorkspaceAcrossProcesses'],
    'internal/delivery': ['TestIndependentProcessesInstallSameTarget', 'TestUpgradeSurvivesKilledProcessAfterOldMove'],
    'internal/vault': ['TestAcquireLeaseAfterKilledHolderProcess', 'TestDataResourcesCrossProcessAndKilledOwner'],
    'internal/selection': ['TestProjectLockReplacementIsAtomicForConcurrentReaders'],
    'internal/live/journal': ['TestWindowOwnerSeparateProcesses', 'TestWindowRunProcessExitAllowsOnlyRecordedRecovery'],
  },
  addon: {
    'tests/addon': ['TestWorkbenchLuaSuites', 'TestUnifiedTOCContract',
      ...readdirSync(join(repository, 'tests/addon')).filter(f => /^t_.*\.lua$/.test(f)).flatMap(file =>
        ['retail', 'classic', 'titan', 'forever', 'classic-era', 'anniversary'].map(client => `TestWorkbenchLuaSuites/${file}/${client}`))],
    'tests/protocol': ['TestLuaReceiverBindings', 'TestSlotDriverLuaRecoveryWithoutBusinessReplay', 'TestSlotDriverLuaReportPreservesFiniteNumbers'],
    'internal/codebase': [],
  },
  doctor: {
    'internal/live/channel': ['TestLiveDoctorUpgradeCreatesDefaultArchiveParent', 'TestLiveDoctorUpgradeDefaultArchiveAcrossVolumes', 'TestLiveDoctorUpgradePreservesExplicitArchive'],
  },
};
const optional = new Set(['TestLiveDoctorUpgradeDefaultArchiveAcrossVolumes']);

export function verifyEvidence(group, inputs, expected) {
  if (!groups[group]) throw new Error(`unknown evidence group: ${group}`);
  const { identity, events } = inputs;
  if (identity.testedCommit !== expected.commit || identity.testedTree !== expected.tree) throw new Error('full-suite commit/tree mismatch');
  if (identity.goos !== 'windows' || identity.goarch !== 'amd64' || identity.exitCode !== 0) throw new Error('full-suite platform or exit status mismatch');
  const report = createCoverageReport(inputs);
  if (!['passed', 'passed_with_skips'].includes(report.state)) throw new Error(`full-suite evidence ${report.state}`);
  const parsed = events.replace(/^\uFEFF/, '').split(/\r?\n/).filter(Boolean).map(JSON.parse);
  const results = [];
  for (const [name, required] of Object.entries(groups[group])) {
    const pkg = modulePath + name;
    const actual = report.tests.packages.find(p => p.importPath === pkg);
    if (actual?.result !== 'pass' || actual.tests.passed === 0) throw new Error(`missing completed package tests: ${name}`);
    for (const test of required) {
      const matching = parsed.filter(e => e.Package === pkg && (e.Test === test || e.Test?.startsWith(`${test}/`)));
      const terminal = matching.filter(e => ['pass', 'skip', 'fail'].includes(e.Action));
      if (!matching.some(e => e.Test === test && e.Action === 'run') || !terminal.some(e => e.Test === test)) throw new Error(`missing executed test: ${name}:${test}`);
      for (const event of terminal) {
        if (event.Action !== 'pass' && !(optional.has(test) && event.Action === 'skip')) throw new Error(`required test ${event.Action}: ${name}:${event.Test}`);
      }
    }
    // Keep helper/environment skips visible; these are not passing scenarios.
    results.push({ package: name, tests: actual.tests, skips: report.tests.skippedTests.filter(t => t.package === pkg),
      output: group === 'doctor' ? parsed.filter(e => e.Package === pkg && e.Action === 'output' && required.some(t => e.Test === t || e.Test?.startsWith(`${t}/`))).map(e => e.Output).join('') : undefined });
  }
  return { group, state: 'passed', commit: identity.testedCommit, tree: identity.testedTree, inputs: report.inputs, results };
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const opts = options(process.argv.slice(2), ['--group', '--directory']);
  if (!opts['--directory'] || !opts['--group']) throw new Error('--group and --directory are required');
  const directory = opts['--directory'];
  const git = arg => {
    const result = spawnSync('git', ['rev-parse', arg], { cwd: repository, encoding: 'utf8', windowsHide: true });
    if (result.status !== 0) throw new Error(result.stderr || 'git identity failed');
    return result.stdout.trim();
  };
  console.log(JSON.stringify(verifyEvidence(opts['--group'], {
    profile: readFileSync(join(directory, 'cover.out'), 'utf8'),
    events: readFileSync(join(directory, 'go-test.jsonl'), 'utf8'),
    identity: JSON.parse(readFileSync(join(directory, 'identity.json'), 'utf8').replace(/^\uFEFF/, '')),
  }, { commit: git('HEAD'), tree: git('HEAD^{tree}') }), null, 2));
}
