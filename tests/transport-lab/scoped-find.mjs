import {readFileSync} from 'node:fs';
import {command} from './parsers.mjs';
const [pid,...files]=process.argv.slice(2),ranges=new Map();
for(const file of files)for(const c of JSON.parse(readFileSync(file,'utf8')).candidates)ranges.set(c.region.range.join(','),c.region.range);
const begun=Date.now(),results=[];
for(const [from,to] of ranges.values()){
 const x=command(['memory','find','--pid',pid,'--text','LYCHEE_MEMORY_LAB2_','--from',from,'--to',to,'--max-hits','256','--read-budget','67108864']);
 results.push({range:[from,to],complete:x.complete,truncated:x.truncated,scannedBytes:x.bytesScanned,hits:x.hits.map(h=>h.address)});
}
console.log(JSON.stringify({elapsedMs:Date.now()-begun,results},null,2));
