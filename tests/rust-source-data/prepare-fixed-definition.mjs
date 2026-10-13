// Test preparation only. Product Rust never delegates data reads to Node/Go.
import {request} from 'node:https';
import {createHash} from 'node:crypto';
import {mkdir,writeFile} from 'node:fs/promises';
import path from 'node:path';
const [commit,name,directory]=process.argv.slice(2);
if(!/^[a-f0-9]{40}$/.test(commit??'')||!/^[A-Za-z_][A-Za-z0-9_-]{0,127}$/.test(name??'')||!directory)throw Error('commit, table name and task-owned output directory required');
async function fetch(url){return new Promise((resolve,reject)=>{const req=request(url,{method:'GET',timeout:30_000},res=>{if(res.statusCode!==200){res.resume();reject(Error(`HTTP ${res.statusCode}`));return;}const parts=[];let total=0;res.on('data',bytes=>{total+=bytes.length;if(total>4*1024*1024){req.destroy(Error('definition byte budget'));return;}parts.push(bytes);});res.on('error',reject);res.on('end',()=>resolve(Buffer.concat(parts)));});req.on('timeout',()=>req.destroy(Error('definition deadline')));req.on('error',reject);req.end();});}
const base=`https://raw.githubusercontent.com/wowdev/WoWDBDefs/${commit}/`;
const manifest=await fetch(base+'manifest.json');
const entries=JSON.parse(manifest.toString('utf8'));
if(!Array.isArray(entries)||entries.length>16384)throw Error('invalid manifest');
const matches=entries.filter(v=>v.tableName.toLowerCase()===name.toLowerCase());
if(matches.length!==1||!/^[A-Za-z_][A-Za-z0-9_-]{0,127}$/.test(matches[0].tableName))throw Error('ambiguous identity');
const canonical=matches[0].tableName;
const definition=await fetch(base+`definitions/${canonical}.dbd`);
await mkdir(directory,{recursive:true});
await writeFile(path.join(directory,'manifest.json'),manifest);
await writeFile(path.join(directory,`${canonical}.dbd`),definition);
const hash=b=>createHash('sha256').update(b).digest('hex');
const report={schema:'lycheedev.fixed-definition-input.v1',commit,identity:matches[0],manifest:{url:base+'manifest.json',bytes:manifest.length,sha256:hash(manifest)},definition:{url:base+`definitions/${canonical}.dbd`,bytes:definition.length,sha256:hash(definition)}};
await writeFile(path.join(directory,'input-identity.json'),JSON.stringify(report,null,2)+'\n');
console.log(JSON.stringify(report));
