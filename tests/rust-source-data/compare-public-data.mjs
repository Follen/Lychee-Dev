import {readFileSync,writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {isDeepStrictEqual} from 'node:util';
import {resolve,join} from 'node:path';
const directory=resolve(process.argv[2]);
const parse=path=>JSON.parse(readFileSync(path,'utf8').replace(/^\uFEFF/,''),(key,value,context)=>{
  if(typeof value==='number' && context?.source && /^-?\d+$/.test(context.source)){
    const exact=BigInt(context.source);if(exact>BigInt(Number.MAX_SAFE_INTEGER)||exact<BigInt(Number.MIN_SAFE_INTEGER))return exact;
  }return value;
});
const hash=path=>createHash('sha256').update(readFileSync(path)).digest('hex');
const cases=[];
for(const kind of ['sql','asset']){
  const go=parse(join(directory,`go-${kind}.json`)),rust=parse(join(directory,`rust-${kind}.json`));
  const differences=[],equal=[];
  const check=(path,a,b)=>{if(isDeepStrictEqual(a,b))equal.push(path);else differences.push({path,kind:a===undefined?'rust_added':b===undefined?'rust_missing':'different'});};
  check('ok',go.ok,rust.ok);
  if(kind==='sql'){
    for(const key of ['complete','partial','query','result'])check(`result.${key}`,go.result[key],rust.result[key]);
    check('result.sources.length',go.result.sources.length,rust.result.sources.length);
    for(let i=0;i<go.result.sources.length;i++){
      for(const key of ['complete','truncated','sources','layoutHash','schema'])check(`result.sources[${i}].${key}`,go.result.sources[i][key],rust.result.sources[i]?.[key]);
      const a=go.result.sources[i].file,b=rust.result.sources[i]?.file;
      for(const key of Object.keys(a))check(`result.sources[${i}].file.${key}`,a[key],b?.[key]);
    }
    for(const key of Object.keys(go.result).filter(k=>!['complete','partial','query','result','sources','timings'].includes(k)))check(`result.${key}`,go.result[key],rust.result[key]);
  }else for(const key of Object.keys(go.result))check(`result.${key}`,go.result[key],rust.result[key]);
  cases.push({kind,goEnvelopeSHA256:hash(join(directory,`go-${kind}.json`)),rustEnvelopeSHA256:hash(join(directory,`rust-${kind}.json`)),equal,differences,passed:differences.length===0});
}
const report={schema:'lycheedev.public-data-differential.v1',cases,scope:'same explicit DataPin/local installation/locale/variant, selected SQL and raw file; timings and ephemeral capture IDs excluded',passed:cases.every(c=>c.passed)};
writeFileSync(join(directory,'public-differential-report.json'),JSON.stringify(report,null,2)+'\n');
process.stdout.write(JSON.stringify(report,null,2)+'\n');
