// Pure JS contract checks; importing prepare never starts Go/Rust or a download.
import test from 'node:test';
import assert from 'node:assert/strict';
import {currentRetailRow, probesFor, profileCommands, rowJSON, SQL} from './prepare.mjs';
import {parseAgentJson} from '../../tools/rust-agent-client.mjs';
const catalog=(version,build='a'.repeat(32))=>Buffer.from(`Branch!STRING:0|Active!DEC:1|Product!STRING:0|Version!STRING:0|Build Key!HEX:16|CDN Key!HEX:16\ncn|1|wow_classic|5.5.4.1|${build}|${'b'.repeat(32)}\ncn|1|wow|${version}|${build}|${'b'.repeat(32)}\n`);
test('installed Retail identity follows the current active row rather than an old build',()=>{
  assert.equal(currentRetailRow(catalog('12.1.5.70448')).Version,'12.1.5.70448');
  assert.equal(currentRetailRow(catalog('12.2.0.80001')).Version,'12.2.0.80001');
  assert.throws(()=>currentRetailRow(Buffer.from(catalog('12.2.0.80001').toString().replace('cn|1|wow|','cn|0|wow|'))));
  assert.throws(()=>currentRetailRow(Buffer.concat([catalog('12.2.0.80001'),Buffer.from(`cn|1|wow|12.2.0.80002|${'c'.repeat(32)}|${'d'.repeat(32)}\n`)])));
});
test('record, case-fold search, foreign-key and truncation probes come from complete Go rows',()=>{
  const probes=probesFor([{ID:17,Directory:'FirstMap'},{ID:25,Directory:'SecondMap'},{ID:39,Directory:'ThirdMap'}]);
  assert.equal(probes.recordId,17);assert.equal(probes.searchQuery,'FIRSTMAP');
  assert(['FirstMap','SecondMap','ThirdMap'].every(s=>s.toLowerCase().includes(probes.truncatedSearchQuery)));
  const commands=Object.fromEntries(profileCommands(25,probes).map(([id,args,status])=>[id,{args,status}]));
  assert.equal(commands.record.args.at(-1),'17');assert.equal(commands.following.args.at(-1),'25');
  assert(commands.foreignKey.args.includes('17'));assert.equal(commands.invalidForeignKey.status,2);
  assert.equal(Object.keys(SQL).length,7);assert(SQL.sql.includes('ID=:id'));
});
test('joined full rows retain wide integer values, fractional cells and negative zero',()=>{
  const rows=parseAgentJson('{"ids":[17],"rows":[{"ID":17,"wide":18446744073709551615,"fraction":0.125,"negativeZero":-0}]}');
  const encoded=rowJSON(rows);assert(encoded.includes('18446744073709551615'));assert(encoded.includes('"fraction":0.125'));assert(encoded.includes('"negativeZero":-0'));
  assert.equal(rowJSON(parseAgentJson(encoded)),encoded);
});
