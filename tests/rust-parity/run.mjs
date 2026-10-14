#!/usr/bin/env node
import { writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { resolve, dirname } from 'node:path';
import { buildFixture, loadCandidate, compare, sha256, readBounded } from './oracle.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const fixturePath = resolve(root, 'tests/rust-parity/go-3.2.0.manifest.json');
const args = process.argv.slice(2);
const valid = ['--json', '--write-fixture', '--verify-fixture'];
function main() {
  if (args.some(a => !valid.includes(a)) || new Set(args).size !== args.length || (args.includes('--write-fixture') && args.includes('--verify-fixture'))) throw new Error('Usage: node tests/rust-parity/run.mjs [--json] [--write-fixture | --verify-fixture]');
  const oracle = buildFixture(root);
  const encoded = JSON.stringify(oracle, null, 2) + '\n';
  if (args.includes('--write-fixture')) {
    writeFileSync(fixturePath, encoded, { flag: 'w' });
    console.log(`Wrote frozen Go fixture: ${oracle.commands.length} commands, ${oracle.products.length} products, ${oracle.addon.length} addon files; sha256=${sha256(encoded)}`);
    return;
  }
  const fixture = readBounded(root, 'tests/rust-parity/go-3.2.0.manifest.json');
  if (!fixture.equals(Buffer.from(encoded))) throw new Error('Fixture mismatch with fixed Go commit. Review the diff; never auto-update from moving main.');
  if (args.includes('--verify-fixture')) { console.log(`PASS frozen fixture: ${sha256(fixture)}`); return; }
  const report = compare(oracle, loadCandidate(root));
  report.fixtureSha256 = sha256(fixture);
  if (args.includes('--json')) console.log(JSON.stringify(report, null, 2));
  else {
    console.log(`${oracle.oracle.release} @ ${oracle.oracle.commit}`);
    console.log(`Fixture verified: ${oracle.commands.length} commands, ${oracle.products.length} products, ${oracle.addon.length} addon files`);
    for (const key of ['commands', 'flags', 'parserFlagArity', 'advertised', 'products', 'addon']) {
      const check = report[key];
      console.log(`${key}: ${check.state.toUpperCase()}${check.expectedCount === undefined ? ` (${check.reason})` : ` expected=${check.expectedCount} actual=${check.actualCount} missing=${check.missing.length} extra=${check.extra.length} changed=${check.changed.length}`}`);
      if (check.missing?.length) console.log(`  missing: ${check.missing.join(', ')}`);
      if (check.extra?.length) console.log(`  extra: ${check.extra.join(', ')}`);
      if (check.changed?.length) console.log(`  changed: ${check.changed.map(v => v.path ?? v.id).join(', ')}`);
    }
    console.log(`source-family mapping: UNVERIFIED (regional resolution and cross-region isolation require behavioral fixtures)`);
    console.log(`Static status: ${report.staticStatus.toUpperCase()}; behavioral parity: UNVERIFIED; runtime/performance/game tests: NOT_RUN`);
    console.log('Use --json for exact per-field expected/actual differences and candidate source hashes.');
  }
  process.exitCode = report.staticStatus === 'pass' ? 0 : report.staticStatus === 'fail' ? 1 : 2;
}
try { main(); } catch (error) {
  // Extraction/integrity failures are not represented as a passing empty result.
  console.error(`ERROR: ${error.message}`); process.exitCode = 2;
}
