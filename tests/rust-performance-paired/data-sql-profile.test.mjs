import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {DATA_PHASE_NAMES, DATA_DETAIL_NAMES, DATA_PUBLISHER_NAMES, DATA_SQL_NAMES, DATA_PHASE_TIMING, DATA_CAPTURE_TIMING, DATA_CLI_TIMING, dataPhaseProfile, dataCliProfile, dataPhaseEnvironment} from './data-phase-profile.mjs';
const raw = value => Buffer.from(JSON.stringify(value) + '\n');
const counters = names => names.map(phase => ({phase, calls: 0, elapsedMicros: 0}));
const data = () => ({schema: 'lycheedev.data-query-phases.v5', elapsedMicros: 10, timing: DATA_PHASE_TIMING, captureTiming: DATA_CAPTURE_TIMING, phases: counters(DATA_PHASE_NAMES), details: counters(DATA_DETAIL_NAMES), publisher: counters(DATA_PUBLISHER_NAMES), sql: counters(DATA_SQL_NAMES)});
test('v5 SQL profile exact counter order matches Rust and retains historical v4', () => {
  const source = readFileSync(new URL('../../crates/lycheedev-data/src/phase_profile.rs', import.meta.url), 'utf8');
  assert.deepEqual([...source.match(/const SQL_NAMES:.*?= \[([\s\S]*?)\];/)[1].matchAll(/"([^"]+)"/g)].map(x => x[1]), DATA_SQL_NAMES);
  const v5 = data(); v5.sql[0] = {phase: DATA_SQL_NAMES[0], calls: 2, elapsedMicros: 30};
  assert.deepEqual(dataPhaseProfile(raw(v5)), v5); // nesting is not additive
  const v4 = {...v5, schema: 'lycheedev.data-query-phases.v4'}; delete v4.sql;
  assert.deepEqual(dataPhaseProfile(raw(v4)), v4);
  for (const change of [x => delete x.sql, x => x.sql.reverse(), x => x.sql.pop(), x => x.sql.push(x.sql[0]), x => x.sql[0].phase = 'unknown', x => x.sql[0].calls = 0, x => x.sql[0].calls = -1, x => x.sql[0].elapsedMicros = 0.5, x => x.sql[0].elapsedMicros = 2**53, x => x.sql[0].extra = true, x => x.schema = v4.schema]) {
    const bad = structuredClone(v5); change(bad); assert.throws(() => dataPhaseProfile(raw(bad)));
  }
  assert.throws(() => dataPhaseProfile(raw({...v4, schema: v5.schema})));
});
const cli = () => ({schema: 'lycheedev.data-cli-phases.v1', timing: DATA_CLI_TIMING, command: 'data sql', mainEntryToDispatchMicros: 3, dispatchMicros: 7, renderMicros: 5, mainElapsedMicros: 15, renderSucceeded: true});
test('CLI exact profile preserves independent framing, failed rendering and truncation', () => {
  const source = readFileSync(new URL('../../crates/lycheedev-cli/src/data_phase_profile.rs', import.meta.url), 'utf8'); assert(source.includes(DATA_CLI_TIMING));
  for (const command of ['data sql', 'data cache prepare']) for (const renderSucceeded of [true, false]) for (const delta of [0, 1, 2]) {
    const value = {...cli(), command, renderSucceeded, mainElapsedMicros: 15 + delta};
    assert.deepEqual(dataCliProfile(raw(value)), value);
  }
  for (const change of [x => delete x.renderSucceeded, x => x.extra = 1, x => x.renderSucceeded = 'true', x => x.command = 'source query', x => x.schema = 'unknown', x => x.mainElapsedMicros = 14, x => x.mainElapsedMicros = 18, x => x.dispatchMicros = 0.5, x => x.renderMicros = -1, x => x.mainEntryToDispatchMicros = 2**53]) {
    const bad = cli(); change(bad); assert.throws(() => dataCliProfile(raw(bad)));
  }
  for (const bytes of [Buffer.concat([raw(cli()), raw(cli())]), Buffer.from('\ufeff' + raw(cli())), Buffer.from('{"schema":1,"schema":2}'), Buffer.from([255]), Buffer.from(''), Buffer.alloc(4097)]) assert.throws(() => dataCliProfile(bytes));
});

 test('unrelated benchmark modes strip inherited CLI diagnostics case-insensitively', async () => {
  const {csvEnvironment} = await import('./data-csv-diagnostics.mjs');
  const env = {KEEP: 'yes', LYCHEEDEV_DATA_CLI_PROFILE: '1', lycheedev_data_cli_profile: '1'};
  assert.deepEqual(dataPhaseEnvironment(env), {KEEP: 'yes'});
  assert.deepEqual(dataPhaseEnvironment(env, {enabled: true, measured: true, engine: 'rust'}), {KEEP: 'yes', LYCHEEDEV_DATA_PHASE_PROFILE: '1'});
  assert.deepEqual(csvEnvironment(env), {KEEP: 'yes'});
  assert.equal(Object.keys(env).length, 3);
});
