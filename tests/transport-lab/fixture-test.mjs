import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {parseBody,parseHead} from './parsers.mjs';
const dir=(process.argv[2] || '.tmp/transport-lab/fixtures')+'/';
let checks=0;
for(let epoch=0;epoch<=5;epoch++){
 const bytes=readFileSync(dir+`head-${epoch}.bin`),head=parseHead(bytes);
 assert.equal(head.epoch,epoch);checks++;
 assert.equal(parseHead(bytes.subarray(0,bytes.length-1)),null);checks++;
 for(const row of head.rows.filter(x=>x.state==='reported')){
  const body=readFileSync(dir+`${row.id}-${epoch}.bin`);
  assert.equal(parseBody(body).id,row.id);checks++;
  const corrupt=Buffer.from(body);corrupt[Math.floor(body.length/2)]^=1;
  assert.equal(parseBody(corrupt),null);checks++;
  assert.equal(parseBody(body.subarray(0,body.length-1)),null);checks++;
 }
}
console.log(JSON.stringify({ok:true,checks}));
