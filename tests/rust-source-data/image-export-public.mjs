// Public image semantics and evidence-chain parity against the frozen Go CLI.
import assert from 'node:assert/strict';
import {cpSync, existsSync, linkSync, mkdirSync, readFileSync, writeFileSync} from 'node:fs';
import {join, resolve} from 'node:path';
import {createHash} from 'node:crypto';
import {createRequire} from 'node:module';
import {DatabaseSync} from 'node:sqlite';
import {isDeepStrictEqual as equal} from 'node:util';
import {ownedSpawnSync} from '../../tools/rust-owned-process.mjs';
const [rustArg, fixtureArg, outArg, sharpArg] = process.argv.slice(2);
assert(rustArg && fixtureArg && outArg && sharpArg, 'usage: image-export-public.mjs <cli.exe> <fixture> <new-output> <sharp-module>');
const sharp = createRequire(import.meta.url)(resolve(sharpArg));
const fixture = resolve(fixtureArg), out = resolve(outArg);
assert(!existsSync(out)); mkdirSync(out, {recursive:true});
const identity = JSON.parse(readFileSync(join(fixture, 'input-identity.json')));
const cases = JSON.parse(readFileSync(join(fixture, 'image-cases.json')));
const binaries = {go:resolve('.tmp/lycheedev-oracle.exe'), rust:resolve(rustArg)};
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const binaryHashes = Object.fromEntries(Object.entries(binaries).map(([engine,path])=>[engine,hash(readFileSync(path))]));
const processes = [], results = [], counters = [];
const homes = {};
for (const engine of ['go','rust']) { homes[engine]=join(out,engine+'-home');cpSync(identity.home,homes[engine],{recursive:true}); }
function save() {
  writeFileSync(join(out,'report.json'),JSON.stringify({passed:results.every(row=>row.passed),binaryHashes,decoder:sharp.versions,results,counters,processes,
    scope:'Public JSON/JSONL; lossless RGBA, image/source metadata and three-capture chains. Encoded artifact and manifest bytes are individually authenticated, not required identical across codecs.'},null,2));
}
function execute(engine,name,format,flags) {
  const evidence=join(out,name+'-'+engine);mkdirSync(evidence);
  const output=join(evidence,'image.'+(flags.encoding||'png'));
  if(flags.existing) {writeFileSync(output,'old bytes');linkSync(output,join(evidence,'old-image.bin'));}
  const args=['asset','export','--file-id',String(flags.fileDataID),'--encoding',flags.encoding||'png','--output',output,
    '--snapshot',identity.snapshot,'--installation',identity.installation,'--home',homes[engine],'--format',format,'--offline',...(flags.args||[])];
  const run=ownedSpawnSync(binaries[engine],args,{cwd:process.cwd(),evidence,timeout:30000});
  processes.push({engine,name,args,exit:run.status,handlesReaped:run.handlesReaped,proof:run.proof});
  writeFileSync(join(evidence,'stdout'),run.stdout);writeFileSync(join(evidence,'stderr'),run.stderr);
  assert(!run.error&&run.handlesReaped);assert.equal(run.stderr.toString(),'');
  const frames=run.stdout.toString().trim().split(/\r?\n/).map(JSON.parse);
  const envelope=format==='json'?frames[0]:frames.at(-1).envelope;
  if(format==='jsonl') assert.deepEqual(frames.map(row=>row.type),run.status===0?['begin','record','end']:['begin','error']);
  return {exit:run.status,envelope,output,evidence};
}
function blob(home,reference) {
  const raw=readFileSync(join(home,'blobs',reference.sha256.slice(0,2),reference.sha256.slice(2)));
  assert.equal(raw.length,reference.bytes);assert.equal(hash(raw),reference.sha256);return raw;
}
function expectedPixels(test,channels) {
  const rgba=Buffer.from(test.pixelsRGBA);
  if(channels==='a') for(let i=0;i<rgba.length;i+=4) {rgba[i]=rgba[i+3];rgba[i+1]=rgba[i+3];rgba[i+2]=rgba[i+3];rgba[i+3]=255;}
  return rgba;
}
async function verify(engine,name,run,test,encoding,channels) {
  assert.equal(run.exit,0);const env=run.envelope, value=env.result;
  const raw=readFileSync(run.output), decoded=await sharp(raw).ensureAlpha().raw().toBuffer({resolveWithObject:true});
  assert.equal(decoded.info.width,test.texture.width);assert.equal(decoded.info.height,test.texture.height);
  assert.deepEqual(decoded.data,expectedPixels(test,channels));assert.equal(value.image.pixelsSHA256,hash(decoded.data));
  assert.deepEqual(value.image.texture,test.texture);assert.equal(value.image.channels,channels==='a'?'a':'rgba');
  assert.equal(value.encoding,encoding);assert.equal(value.output,run.output);assert.equal(env.captures.length,3);
  const [source,manifest,artifact]=env.captures;
  assert.deepEqual(source,value.sourceCapture);assert.deepEqual(artifact,value.artifactCapture);assert.deepEqual(artifact.blob,value.content);
  assert.deepEqual(blob(homes[engine],artifact.blob),raw);assert.equal(artifact.mediaType,'image/'+encoding);
  const input=identity.tables.find(row=>row.fileDataID===test.fileDataID);
  assert.equal(source.blob.sha256,input.sha256);assert.equal(blob(homes[engine],source.blob).length,input.bytes);
  assert.deepEqual(source.blob,value.source.content);assert.equal(source.mediaType,'application/octet-stream');
  assert.equal(manifest.mediaType,'application/json');assert.deepEqual(JSON.parse(blob(homes[engine],manifest.blob)),value);
  const db=new DatabaseSync(join(homes[engine],'state/toolkit.sqlite'),{readOnly:true});
  try {for(const [index,capture]of env.captures.entries()) {
    const row=db.prepare('SELECT value FROM documents WHERE key=?').get('capture/'+capture.id);
    assert(row);assert.deepEqual(JSON.parse(Buffer.from(row.value)),capture);assert.equal(capture.complete,true);assert.equal(capture.truncated,false);
    assert.equal(capture.provenance.kind,['asset','asset-export-manifest','asset-image'][index]);
    assert.equal(capture.provenance.snapshot,identity.snapshot);assert.equal(capture.provenance.dataBuild,identity.pin.fullBuild);
    if(index)assert.equal(capture.provenance.locator,source.id);
  }}finally{db.close();}
  counters.push({engine,name,decodedCache:value.source.decodedCache,artifact:artifact.blob,manifest:manifest.blob});
  const normalized=structuredClone(env), result=normalized.result;
  result.output='<output>';delete result.source.decodedCache;
  if(engine==='rust') {assert.equal(result.source.verification.contentWholeDecodedCKey,true);assert.equal(result.source.verification.rootWholeDecodedCKey,true);delete result.source.verification;delete result.source.encodingIO;}
  const ids=new Map(env.captures.map((capture,index)=>[capture.id,'<capture-'+index+'>']));
  function normalizeCapture(capture,index) {capture.id=ids.get(capture.id);capture.capturedAt='<time>';if(index)capture.provenance.locator=ids.get(capture.provenance.locator);if(index)capture.blob={sha256:'<codec-dependent-'+index+'>',bytes:'<codec-dependent>'};}
  normalized.captures.forEach(normalizeCapture);normalizeCapture(result.sourceCapture,0);normalizeCapture(result.artifactCapture,2);result.content=structuredClone(result.artifactCapture.blob);
  return normalized;
}
for(const test of cases.cases)for(const encoding of cases.encodings)for(const channels of cases.channels)for(const format of ['json','jsonl']) {
  const name=`${test.name}-${encoding}-${channels}-${format}`,runs={},errors={};
  for(const engine of ['go','rust'])try {
    const run=execute(engine,name,format,{fileDataID:test.fileDataID,encoding,args:['--mipmap',String(test.mipmap),'--channels',channels]});
    runs[engine]=await verify(engine,name,run,test,encoding,channels);
  } catch(error){errors[engine]=error.stack;}
  results.push({name,passed:Object.keys(errors).length===0&&equal(runs.go,runs.rust),runs,errors});save();
  console.log(JSON.stringify({name,passed:results.at(-1).passed}));
}
for(const format of ['json','jsonl'])for(const fault of [
  {name:'pixel-limit',fileDataID:501,args:['--max-pixels','3']},
  {name:'unavailable-mip',fileDataID:501,args:['--mipmap','15']},
  {name:'invalid-channels',fileDataID:501,args:['--channels','rr']},
  {name:'truncated',fileDataID:507},{name:'unsupported',fileDataID:508},
  {name:'conflict',fileDataID:501,existing:true}
]) {
  const name=fault.name+'-'+format,runs={},errors={};
  for(const engine of ['go','rust'])try {
    const run=execute(engine,name,format,fault);assert.notEqual(run.exit,0);assert.equal(run.envelope.ok,false);
    if(fault.existing) {
      assert.equal(readFileSync(run.output,'utf8'),'old bytes');
      assert.equal(run.envelope.error.message,run.envelope.error.code+': '+run.output);
      run.envelope.error.message=run.envelope.error.code+': <output>';
    }else assert(!existsSync(run.output));
    runs[engine]={exit:run.exit,envelope:run.envelope};
  }catch(error){errors[engine]=error.stack;}
  results.push({name,passed:Object.keys(errors).length===0&&equal(runs.go,runs.rust),runs,errors});save();
}
for(const encoding of cases.encodings) {
  const name='overwrite-'+encoding,runs={},errors={};
  for(const engine of ['go','rust'])try {
    const run=execute(engine,name,'json',{fileDataID:501,encoding,existing:true,args:['--overwrite']});
    runs[engine]=await verify(engine,name,run,cases.cases[0],encoding,'rgba');assert.equal(readFileSync(join(run.evidence,'old-image.bin'),'utf8'),'old bytes');
  }catch(error){errors[engine]=error.stack;}
  results.push({name,passed:Object.keys(errors).length===0&&equal(runs.go,runs.rust),runs,errors});save();
}
for(const engine of ['go','rust'])assert.equal(hash(readFileSync(binaries[engine])),binaryHashes[engine]);
console.log(JSON.stringify({passed:results.every(row=>row.passed),cases:results.length,failed:results.filter(row=>!row.passed).map(row=>row.name),report:join(out,'report.json')}));
if(results.some(row=>!row.passed))process.exitCode=1;
