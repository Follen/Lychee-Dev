// Current owned collector; no Rust SQL fallback.
import assert from 'node:assert/strict';
import {parse} from './agent-json.mjs';
import {bounded} from './agent-fixture-inputs.mjs';
assert.equal(parse(bounded(process.argv[2],2<<20)).mode,'data');
await import('./agent-paired-run.mjs');
