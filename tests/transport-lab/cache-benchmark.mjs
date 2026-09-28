import {spawn} from 'node:child_process';
import {readFileSync} from 'node:fs';
import {join} from 'node:path';
import {parseBody} from './parsers.mjs';
const [pid,file]=process.argv.slice(2),known=JSON.parse(readFileSync(file,'utf8'));
if(!known.ok)throw Error('verified current census required');
const body=known.reports[0].body,cli=join(process.env.APPDATA,'npm/node_modules/wowdump/dist/cli.js');
function read(){return new Promise((resolve,reject)=>{
 const started=Date.now(),p=spawn(process.execPath,[cli,'memory','read','--pid',pid,'--address',body.address,'--size',String(body.total)]);
 let out='',err='';const timer=setTimeout(()=>p.kill(),10000);
 p.stdout.on('data',x=>{out+=x;if(out.length>4*1024*1024)p.kill()});p.stderr.on('data',x=>err+=x);
 p.on('error',reject);p.on('close',code=>{clearTimeout(timer);try{
 if(code!==0)throw Error(err||out);const r=JSON.parse(out),v=parseBody(Buffer.from(r.dataHex,'hex'));
 resolve({elapsedMs:Date.now()-started,valid:r.complete&&v?.sha256===body.sha256&&v?.run===known.run,address:body.address});
 }catch(e){reject(e)}});
})}
const started=Date.now();const reads=await Promise.all(Array.from({length:8},()=>read()));
console.log(JSON.stringify({workers:8,elapsedMs:Date.now()-started,bytesPerRead:body.total,reads,caveat:'Concurrent repeat-read benchmark of one known immutable report. Freshness still requires current run authority.'},null,2));
