#!/usr/bin/env node
import {writeFileSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import {dirname, resolve} from 'node:path';
import {loadCandidate, sha256} from './oracle.mjs';
import {buildCurrentFixture, loadCurrentFixture, compareCurrent, CURRENT_FIXTURE} from './oracle-3.2.1.mjs';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const args = process.argv.slice(2), allowed = ['--json', '--write-fixture', '--verify-fixture'];
try {
  if (args.some(a => !allowed.includes(a)) || new Set(args).size !== args.length || args.includes('--write-fixture') && args.includes('--verify-fixture')) throw new Error('Usage: run-3.2.1.mjs [--json] [--write-fixture | --verify-fixture]');
  if (args.includes('--write-fixture')) {
    const fixture = buildCurrentFixture(root), bytes = JSON.stringify(fixture, null, 2) + '\n';
    // Writes only the separately named current fixture, never the historical file.
    writeFileSync(resolve(root, CURRENT_FIXTURE), bytes);
    console.log(`Wrote fixed current fixture: ${fixture.oracle.commit}; sha256=${sha256(bytes)}`);
  } else {
    const frozen = loadCurrentFixture(root);
    if (args.includes('--verify-fixture')) console.log(`PASS fixed current fixture ${frozen.oracle.oracle.commit}; sha256=${frozen.fixtureSHA256}`);
    else {
      const report = {...compareCurrent(frozen.oracle, loadCandidate(root)), fixtureSHA256: frozen.fixtureSHA256};
      if (args.includes('--json')) console.log(JSON.stringify(report, null, 2));
      else {
        console.log(`${report.oracle.release} @ ${report.oracle.commit}`);
        console.log(`Raw exact: ${report.rawExactStatus.toUpperCase()}; accepted static: ${report.acceptedStaticStatus.toUpperCase()}`);
        console.log(`Explicit engine-summary substitutions: ${report.accepted.commandMetadata.approvedEngineSummaryChanges.length}; retained release fields: ${report.accepted.addon.acceptedReleaseFields.length}`);
        console.log('Behavioral parity UNVERIFIED; binaries/Windows/game/performance NOT_RUN. Full raw differences remain in --json.');
      }
      process.exitCode = report.acceptedStaticStatus === 'pass' ? 0 : report.acceptedStaticStatus === 'fail' ? 1 : 2;
    }
  }
} catch (error) { console.error(`ERROR: ${error.message}`); process.exitCode = 2; }
