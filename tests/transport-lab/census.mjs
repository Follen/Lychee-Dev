import {readFileSync} from 'node:fs';
import {parseHead,parseBody,readAt as rawReadAt,regionFor} from './parsers.mjs';
const [pidText,scanFile,run,epochText]=process.argv.slice(2),pid=Number(pidText),epoch=Number(epochText);
const scan=JSON.parse(readFileSync(scanFile,'utf8').replace(/^\uFEFF/,''));
const addresses=[...new Set(scan.results.flatMap(x=>x.hits))];
const started=Date.now(), candidates=[], readErrors=[]; function readAt(...args){try{return rawReadAt(...args)}catch(e){readErrors.push({address:args[1],error:e.message});return null}}
for(const address of addresses){
 const bytes=readAt(pid,address,512); if(!bytes)continue;
 const head=parseHead(bytes);
 if(head){candidates.push({address,type:'head',...head,stable:bytes.equals(readAt(pid,address,512)),region:regionFor(pid,address)});continue;}
 const m=/^LYCHEE_MEMORY_LAB2_BODY\|[0-9a-f]{16}\|REQ-LAB2-[A-E]\|[0-5]\|(?:ascii|utf8|binary|duplicate)\|(\d+)\|[0-9a-f]{8}\|/.exec(bytes.toString('latin1'));
 if(!m)continue;
 const size=m[0].length+Number(m[1])+34;
 const a=readAt(pid,address,size),b=readAt(pid,address,size),body=a&&parseBody(a);
 if(body)candidates.push({address,type:'body',...body,stable:!!b&&a.equals(b),region:regionFor(pid,address)});
}
const head=candidates.find(x=>x.type==='head'&&x.run===run&&x.epoch===epoch&&x.stable);
const reports=head?.rows.filter(x=>x.state==='reported').map(row=>({id:row.id,body:candidates.find(x=>x.type==='body'&&x.run===run&&x.id===row.id&&x.epoch===row.reportEpoch&&x.checksum===row.checksum&&x.stable)}));
console.log(JSON.stringify({ok:!!head&&reports.every(x=>x.body),run,epoch,elapsedMs:Date.now()-started,head,reports,candidates,readErrors,authority:'expected run/epoch from WGC chat; address stability is not freshness'},null,2));
