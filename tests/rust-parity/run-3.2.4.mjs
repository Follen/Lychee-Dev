#!/usr/bin/env node
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { buildRelease324Fixture, compareRelease324 } from './oracle-3.2.4.mjs';
import { loadRelease324Candidate } from './candidate-adapter-3.2.4.mjs';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const args = process.argv.slice(2);
try {
  if (args.some(arg => !['--json', '--verify-fixture'].includes(arg)) || new Set(args).size !== args.length) {
    throw new Error('Usage: run-3.2.4.mjs [--json] [--verify-fixture]');
  }
  const oracle = buildRelease324Fixture(root);
  if (args.includes('--verify-fixture')) console.log(`PASS fixed Go 3.2.4 sources: ${oracle.oracle.commit}; 84 reference commands, 7 products, 55 addon files`);
  else {
    const report = compareRelease324(oracle, loadRelease324Candidate(root));
    if (args.includes('--json')) console.log(JSON.stringify(report, null, 2));
    else {
      console.log(`Go 3.2.4 @ ${oracle.oracle.commit}: raw exact ${report.rawExactStatus}; approved candidate static ${report.acceptedStaticStatus}`);
      console.log('Behavioral parity UNVERIFIED; Rust/Windows/runtime/game/performance NOT_RUN. Raw differences retained in --json.');
    }
    process.exitCode = report.acceptedStaticStatus === 'pass' ? 0 : report.acceptedStaticStatus === 'fail' ? 1 : 2;
  }
} catch (error) { console.error(`ERROR: ${error.message}`); process.exitCode = 2; }
