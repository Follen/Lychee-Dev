#!/usr/bin/env node
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { buildRelease323Fixture, compareRelease323 } from './oracle-3.2.3.mjs';
import { loadRelease323Candidate } from './candidate-adapter-3.2.3.mjs';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const args = process.argv.slice(2);
try {
  if (args.some(arg => !['--json', '--verify-fixture'].includes(arg)) || new Set(args).size !== args.length) {
    throw new Error('Usage: run-3.2.3.mjs [--json] [--verify-fixture]');
  }
  const oracle = buildRelease323Fixture(root);
  if (args.includes('--verify-fixture')) console.log(`PASS fixed Go 3.2.3 sources: ${oracle.oracle.commit}; 84 reference commands, 7 products, 55 addon files`);
  else {
    const report = compareRelease323(oracle, loadRelease323Candidate(root));
    if (args.includes('--json')) console.log(JSON.stringify(report, null, 2));
    else {
      console.log(`Go 3.2.3 @ ${oracle.oracle.commit}: raw exact ${report.rawExactStatus}; approved candidate static ${report.acceptedStaticStatus}`);
      console.log('Behavioral parity UNVERIFIED; Rust/Windows/runtime/game/performance NOT_RUN. Raw differences retained in --json.');
    }
    process.exitCode = report.acceptedStaticStatus === 'pass' ? 0 : report.acceptedStaticStatus === 'fail' ? 1 : 2;
  }
} catch (error) { console.error(`ERROR: ${error.message}`); process.exitCode = 2; }
