import assert from 'node:assert/strict';
import test from 'node:test';
import {decodeAgentFrame, isExactNumber} from './rust-agent-client.mjs';

const frame = provenance => Buffer.from(`{"protocol":"lycheedev.data-agent.v1","id":1,"ok":true,"result":{"provenance":${provenance}}}`);

test('Agent preserves the actual Classic Map CASC u64 name hash', () => {
  const result=decodeAgentFrame(frame('{"sources":[{"sourceReading":{"entry":{"fileDataID":1349477,"nameHash":8549075809966084983}}}]}'),1);
  const hash=result.result.provenance.sources[0].sourceReading.entry.nameHash;
  assert(isExactNumber(hash));
  assert.equal(hash.exactNumber,'8549075809966084983');
});

test('CASC hashes and existing WDC key IDs accept exactly the u64 domain', () => {
  for(const value of ['0','9007199254740991','9007199254740992','18446744073709551615']) {
    assert.doesNotThrow(()=>decodeAgentFrame(frame(`{"sourceReading":{"entry":{"nameHash":${value}}},"sourceLayout":{"partitions":[{"keyId":${value}}]}}`),1));
  }
  for(const value of ['-1','1.5','18446744073709551616','{"exactNumber":"8549075809966084983"}']) {
    assert.throws(()=>decodeAgentFrame(frame(`{"sourceReading":{"entry":{"nameHash":${value}}}}`),1));
  }
});

test('identity payload allowance does not admit wide accounting or unrelated context', () => {
  for(const provenance of [
    '{"availableRows":8549075809966084983}',
    '{"sourceReading":{"entry":{"fileDataID":8549075809966084983}}}',
    '{"entry":{"nameHash":8549075809966084983}}',
    '{"sourceReading":{"entry":{"nameHash":1},"budget":8549075809966084983}}',
  ]) assert.throws(()=>decodeAgentFrame(frame(provenance),1),/agent.unsafe_provenance/);
});
