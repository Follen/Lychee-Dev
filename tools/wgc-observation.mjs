// Optional capture outcome, strict evidence. Reuse the full-suite JSON parser.
import { appendFileSync, readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { resolve } from 'node:path';
import { parseTestEvents } from './go-coverage.mjs';

const pkg = 'github.com/follenfang/lycheedev/internal/desktop';
const testName = 'TestWGCLifetimeBetweenSessions';

export function createWGCObservation(events, exitCode, commit) {
  if (!Number.isInteger(exitCode) || exitCode < 0 || !/^[0-9a-f]{40}$/.test(commit)) throw new Error('invalid WGC execution identity');
  const tests = parseTestEvents(events, [{ importPath: pkg, hasTests: true, instrumented: false }]);
  for (const line of events.replace(/^\uFEFF/, '').split(/\r?\n/).filter(line => line.trim())) {
    const event = JSON.parse(line);
    if (event.Test && event.Test !== testName) throw new Error('unexpected WGC test');
  }
  const result = tests.packages[0];
  const passed = exitCode === 0 && tests.passed === 1 && tests.failed === 0 && result.result === 'pass';
  const failed = exitCode !== 0 && tests.failed === 1 && tests.passed === 0 && result.result === 'fail';
  if (tests.errors.length || tests.buildFailures.length || tests.incomplete || tests.skipped || !result.started || (!passed && !failed)) {
    throw new Error('native WGC observation missing consistent terminal test evidence');
  }
  return { scope: 'test-owned Win32 fixture; WGC session lifetime, no game or external PID',
    commit, exitCode, state: passed ? 'passed' : 'failed',
    note: 'Optional observation outside required offline CI; failed capture remains unverified, not unsupported.' };
}

export function main(args = process.argv.slice(2)) {
  const [eventsPath, exitCode, commit, output] = args;
  if (args.length !== 4) throw new Error('usage: wgc-observation <events> <exit-code> <commit> <output>');
  const result = createWGCObservation(readFileSync(eventsPath, 'utf8'), Number(exitCode), commit);
  writeFileSync(output, `${JSON.stringify(result, null, 2)}\n`);
  const message = `WGC observation: ${result.state === 'passed' ? 'passed' : 'FAILED, verification remains open'}. See windows-wgc-probe; outside required offline CI.`;
  if (process.env.GITHUB_STEP_SUMMARY) appendFileSync(process.env.GITHUB_STEP_SUMMARY, `${message}\n`);
  if (result.state === 'failed') console.log(`::warning::${message}`);
  return result;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { main(); } catch (error) { console.error(error.message); process.exitCode = 1; }
}
