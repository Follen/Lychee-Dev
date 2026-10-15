// Static custody/coverage only; this does not execute the Rust regression.
import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {resolve} from 'node:path';
import {createHash} from 'node:crypto';
import {execFileSync} from 'node:child_process';
const root=resolve(import.meta.dirname,'../..');
const read=p=>readFileSync(resolve(root,p),'utf8');
const hash=b=>createHash('sha256').update(b).digest('hex');
const map=JSON.parse(read('tests/rust-parity/release-321-journal-test-map.json'));
const source=read(map.rustOwner);
const body=source.slice(source.indexOf('struct HeldKeyJournalPeer'));
test('E pins released Go without relabeling historical evidence',()=>{
  assert.equal(map.oracleCommit,'10b561db5706702cd42e047eaeed4ccb5e825351');
  assert.equal(map.previousOracleCommit,'3057a9666ab078270b63c7f06de748069b0d1f4b');
  for(const row of map.goSources) assert.equal(hash(execFileSync('git',['show',`${map.oracleCommit}:${row.path}`],{cwd:root})),row.sha256,row.path);
});
test('E uses actual journal reload, wire decode and production continuation',()=>{
  for(const s of ['DurableChannelJournal','driver.journal.close().unwrap()','journal.load(&OperationControl::default())','driver.continue_until(','decode_input(','decode_record(&wire)','filesystem.seed([initial.state.clone()])']) assert.ok(body.includes(s),s);
  assert.ok(!body.includes('FakeJournal'));assert.ok(!body.includes('known_zero_checkpoints'));
});
test('E covers held/quiet outcomes, uncertain resume and original deadline',()=>{
  for(const s of ['for turn in 0..=600','desktop.keyboard_active','desktop.key_already_held','fixture_submission_unknown','InputDisposition::Uncertain','expected_outcome','for _ in 0..2','d.clock.0.set(deadline)','live.channel_budget_exhausted','d.request_close(','closing_exchange_unconfirmed']) assert.ok(body.includes(s),s);
  assert.match(body,/d\.backend\.0\.input_reads, 601/);assert.match(body,/d\.backend\.0\.published\.len\(\), 1/);
});
test('E checks immutable chain and every exact intent/outcome checksum',()=>{
  for(const s of ['read_log_bytes(&bytes)','event_hash(event)','immutable_archives.get(&path)','expected_predecessor','hydrate(snapshot','expected_inputs.get(&input.id)','assert_eq!(input, &expected','assert_eq!(intents.len(), 601)','assert_eq!(outcomes, intents)','assert_eq!(zero_outcomes, 600)','visited.len() >= 2','log.events.len() < MAX_EVENTS']) assert.ok(body.includes(s),s);
});
test('E identifies its stacked B2 base and leaves runtime evidence NOT_RUN',()=>{
  assert.equal(map.evidence.rustTests,'NOT_RUN');assert.equal(map.evidence.windowsTests,'NOT_RUN');
  assert.equal(map.b2BaseSHA256,'07d84a39d94c907aa87bd4bdb694884483a84fe3e575321bfbabe3d38f242024');
  assert.match(read('tests/rust-parity/release-321-journal-contract.md'),/not claimed to be that\s+commit's bytes/);
  assert.equal(map.tests.length,1);assert.ok(source.includes(`fn ${map.tests[0].rustTest}()`));
});
