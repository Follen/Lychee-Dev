import fs from 'node:fs';
import path from 'node:path';
import { isDeepStrictEqual } from 'node:util';
import { createHash } from 'node:crypto';
const dir=path.resolve(process.argv[2]||'.tmp/rust-source-environment-real');
const aRaw=fs.readFileSync(path.join(dir,'rust.json')),bRaw=fs.readFileSync(path.join(dir,'go.json'));
const a=JSON.parse(aRaw)[0].result,b=JSON.parse(bRaw)[0].result;
const sourceSpans=a.sourceSpans;delete a.sourceSpans;
const differences=[];
for(const key of Object.keys(b)){
  if(!isDeepStrictEqual(a[key],b[key])){
    const first=Array.isArray(a[key])?a[key].findIndex((r,i)=>!isDeepStrictEqual(r,b[key]?.[i])):null;
    differences.push({field:key,firstIndex:first,rust:first!==null?a[key][first]:key==='definitions'?{bytes:Buffer.byteLength(a[key])}:a[key],go:first!==null?b[key][first]:key==='definitions'?{bytes:Buffer.byteLength(b[key])}:b[key]});
  }
}
const sha=x=>createHash('sha256').update(x).digest('hex');
const report={schema:'lycheedev.environment-differential.v1',passed:differences.length===0,scope:'fixed Blizzard generated API metadata; all legacy fields, definitions bytes and hashes',identity:a.manifest.identity,documents:a.coverage.documents,records:a.facts.length,mappings:a.mappings.length,gaps:a.coverage.gaps?.length||0,catalogComplete:a.coverage.complete,definitionsBytes:Buffer.byteLength(a.definitions),rustArtifactSHA256:sha(aRaw),goArtifactSHA256:sha(bRaw),additionalSourceSpans:sourceSpans.length,differences};
fs.writeFileSync(path.join(dir,'differential-report.json'),JSON.stringify(report,null,2));
console.log(JSON.stringify({...report,differences:report.differences.map(d=>({field:d.field,firstIndex:d.firstIndex}))},null,2));
if(!report.passed)process.exitCode=1;
