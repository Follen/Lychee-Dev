// Source-authored declarations. No native process is started by these cases.
import test from 'node:test';
import assert from 'node:assert/strict';
import {AGENT_PROTOCOL, parseAgentJson, decodeAgentFrame, assertSafeContext, stringifyExactIntegerJson, isExactNumber} from '../../tools/rust-agent-client.mjs';

test('Agent payload integers keep exact tokens while identity numbers fail closed', () => {
  const frame = decodeAgentFrame(Buffer.from(`{"protocol":"${AGENT_PROTOCOL}","id":1,"ok":true,"result":{"rows":[[9007199254740993,-9223372036854775808]]}}`), 1);
  assert.deepEqual(frame.result.rows, [[{exactNumber:'9007199254740993'}, {exactNumber:'-9223372036854775808'}]]);
  assert.throws(() => decodeAgentFrame(Buffer.from(`{"protocol":"${AGENT_PROTOCOL}","id":9007199254740993,"ok":true,"result":{}}`), 1));
  assert.throws(() => decodeAgentFrame(Buffer.from(`{"protocol":"${AGENT_PROTOCOL}","id":1,"ok":true,"result":{"provenance":{"offset":9007199254740993}}}`), 1));
  assert.throws(() => assertSafeContext({id: 9007199254740992}));
});
test('Agent parser rejects duplicate keys, invalid UTF8, nonfinite values and partial JSON', () => {
  for (const text of ['{"id":1,"id":2}', '{"x":1e9999}', '{"x":1,}', '[1,]', '{"x":', 'true false', '\u00a0{}']) {
    assert.throws(() => parseAgentJson(Buffer.from(text)));
  }
  assert.throws(() => parseAgentJson(Buffer.from([0x22,0xff,0x22])));
  assert.deepEqual(parseAgentJson(Buffer.from('{"__proto__":{"safe":true},"n":null,"text":"a\\nb"}')), {
    ['__proto__']: {safe:true}, n:null, text:'a\nb',
  });
  assert.equal({}.safe, undefined);
});
test('Agent frame protocol ID outcome and context are independently checked', () => {
  const valid = {protocol:AGENT_PROTOCOL,id:7,ok:true,result:{offset:0,complete:true,rows:[]}};
  for (const change of [
    x => {x.id=8;}, x => {x.protocol='old';}, x => {x.ok='true';},
    x => {x.error={};}, x => {delete x.result;},
    x => {x.result.totalRows={exactNumber:'9007199254740993'};},
  ]) {
    const value=structuredClone(valid);change(value);
    assert.throws(() => decodeAgentFrame(Buffer.from(JSON.stringify(value)),7));
  }
  assert.deepEqual(decodeAgentFrame(Buffer.from(JSON.stringify(valid)),7),valid);
});

test('only authenticated layout partition u64 identity permits exact wide context',()=>{
 const prefix=`{"protocol":"${AGENT_PROTOCOL}","id":1,"ok":true,"result":{"provenance":`;
 const raw=Buffer.from(prefix+'{"sourceIdentity":{"layout":{"partitions":[{"keyId":1311768467294899695}]}},"sourceLayout":{"partitions":[{"keyId":1311768467294899695}]}}}}');
 const value=decodeAgentFrame(raw,1).result.provenance;assert(isExactNumber(value.sourceLayout.partitions[0].keyId));assert.equal(stringifyExactIntegerJson(value.sourceLayout),'{"partitions":[{"keyId":1311768467294899695}]}');
 for(const bad of ['{"resources":{"retainedBytes":1311768467294899695}}','{"sourceLayout":{"partitions":[{"keyId":18446744073709551616}]}}','{"sourceLayout":{"partitions":[{"keyId":{"exactNumber":"1311768467294899695"}}]}}'])assert.throws(()=>decodeAgentFrame(Buffer.from(prefix+bad+'}}'),1));
});
