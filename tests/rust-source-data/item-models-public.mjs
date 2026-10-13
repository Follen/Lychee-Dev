// Public item-model navigation parity, using existing binaries and authentic
// synthetic fixtures. No build or real-client acceptance claim.
import assert from 'node:assert/strict';
import {cpSync,createWriteStream,existsSync,mkdirSync,readFileSync,writeFileSync} from 'node:fs';
import {resolve,join} from 'node:path';
import {createHash} from 'node:crypto';
import {DatabaseSync} from 'node:sqlite';
import {isDeepStrictEqual as equal} from 'node:util';
import {fileURLToPath} from 'node:url';
import {spawn} from 'node:child_process';
import {pipeline} from 'node:stream/promises';
import {ownedSpawnSync} from '../../tools/rust-owned-process.mjs';

// Large JSONL repeats the full result in record and end frames. As in the
// spell runner, an owned parent streams exact CLI output to a case file and
// waits for the child. Existing Job ownership and 16 MiB log caps are unchanged.
if(process.argv[2]==='--capture-cli') {
  const [binary,path,...args]=process.argv.slice(3);
  const child=spawn(binary,args,{windowsHide:true,shell:false,stdio:['ignore','pipe','pipe']});
  const closed=new Promise((ok,reject)=>{child.once('error',reject);child.once('close',(code,signal)=>ok({code,signal}));});
  child.stderr.pipe(process.stderr,{end:false});
  const [,status]=await Promise.all([pipeline(child.stdout,createWriteStream(path,{flags:'wx'})),closed]);
  console.log(JSON.stringify({binary,args,stdout:path,...status}));process.exitCode=status.code??8;
} else {
  await main();
}

async function main() {
  const [rustArg,fixtureArg,outArg,profileArg]=process.argv.slice(2);
  assert(rustArg&&fixtureArg&&outArg,'usage: item-models-public.mjs <cli.exe> <seven-fixture-directory> <new-output> [profile-name,...]');
  const root=process.cwd(),out=resolve(outArg),fixtureRoot=resolve(fixtureArg),self=fileURLToPath(import.meta.url);
  assert(!existsSync(out));mkdirSync(out,{recursive:true});
  const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
  const binaries={go:resolve('.tmp/lycheedev-oracle.exe'),rust:resolve(rustArg)};
  const binarySHA256=Object.fromEntries(Object.entries(binaries).map(([engine,path])=>[engine,hash(readFileSync(path))]));
  assert.equal(binarySHA256.go,'5738418756ee57c536518ff71223f2e7843ac3495a846b4f063d3104d83aae87');
  const fixtures={};
  for(const name of ['baseline','scalarResources','firstInvalidThenMissing','missingComponent','invalidNonmatchingModel','relationBound','rowBound']) {
    const identity=JSON.parse(readFileSync(join(fixtureRoot,name,'input-identity.json'))),homes={};
    for(const engine of ['go','rust']) {homes[engine]=join(out,'homes',name+'-'+engine);cpSync(identity.home,homes[engine],{recursive:true});}
    fixtures[name]={identity,homes};
  }
  const normal={displayID:1000,models:[502,3,502],textures:[4,4],geosetGroup:[-1,0],complete:true,partial:false,partialReasons:null,truncated:false,truncationReason:'none'};
  const empty={displayID:0,models:[],textures:[],geosetGroup:[],complete:true,partial:false,partialReasons:null,truncated:false,truncationReason:'none'};
  const unavailable={...normal,complete:false,partial:true,partialReasons:['variant_unavailable']};
  let profiles=[
    {name:'item-1',fixture:'baseline',item:1,exit:0,expect:normal},
    {name:'item-2-ambiguous-missing-appearance',fixture:'baseline',item:2,exit:0,expect:{...empty,complete:false,partial:true,partialReasons:['relation_ambiguous','relation_unresolved']}},
    {name:'item-3-missing-display',fixture:'baseline',item:3,exit:0,expect:{...empty,complete:false,partial:true,partialReasons:['relation_unresolved']}},
    {name:'item-4-no-appearance',fixture:'baseline',item:4,exit:0,expect:empty},
    {name:'item-5-ambiguous-selected',fixture:'baseline',item:5,exit:0,expect:{...normal,complete:false,partial:true,partialReasons:['relation_ambiguous']}},
    {name:'item-6-no-resources',fixture:'baseline',item:6,exit:0,expect:{...empty,displayID:6000,geosetGroup:[0,0]}},
    {name:'item-999-missing',fixture:'baseline',item:999,exit:3},
    {name:'item-1-warm',fixture:'baseline',item:1,exit:0,expect:normal},
    {name:'variant-exact',fixture:'baseline',item:1,args:['--race-id','2','--gender','1'],exit:0,expect:{...normal,models:[508,3,508],raceID:2,gender:1}},
    {name:'variant-race-only',fixture:'baseline',item:1,args:['--race-id','1'],exit:0,expect:{...unavailable,raceID:1,gender:0}},
    {name:'variant-gender-only',fixture:'baseline',item:1,args:['--gender','1'],exit:0,expect:{...unavailable,raceID:0,gender:1}},
    {name:'variant-negative',fixture:'baseline',item:1,args:['--race-id','-1','--gender','-2'],exit:0,expect:{...unavailable,raceID:-1,gender:-2}},
    {name:'variant-positive-sign',fixture:'baseline',item:1,args:['--race-id','+2','--gender','+1'],exit:0,expect:{...normal,models:[508,3,508],raceID:2,gender:1}},
    {name:'variant-signed-limits',fixture:'baseline',item:1,args:['--race-id','-9223372036854775808','--gender','9223372036854775807'],exit:0,expect:unavailable},
    {name:'variant-no-resources',fixture:'baseline',item:6,args:['--race-id','2','--gender','1'],exit:0,expect:{...empty,displayID:6000,geosetGroup:[0,0],raceID:2,gender:1}},
    {name:'variant-ambiguous-fallback',fixture:'baseline',item:5,args:['--race-id','-1'],exit:0,expect:{...unavailable,partialReasons:['relation_ambiguous','variant_unavailable'],raceID:-1}},
    {name:'scalar-resources',fixture:'scalarResources',item:1,exit:0,expect:{...empty,displayID:1000}},
    {name:'relation-bound',fixture:'relationBound',item:1,exit:0,large:true,expect:{complete:false,truncated:true,truncationReason:'relation_budget',partial:false,partialReasons:null,models:[100001],textures:[],geosetGroup:[0,0]}},
    {name:'row-bound',fixture:'rowBound',item:1,exit:0,large:true,expect:{complete:false,truncated:true,truncationReason:'row_budget',partial:false,partialReasons:null,models:[100001],textures:[],geosetGroup:[0,0]}},
    {name:'first-invalid-before-missing',fixture:'firstInvalidThenMissing',item:1,exit:5},
    ...[1,4,999].map(item=>({name:'missing-component-item-'+item,fixture:'missingComponent',item,exit:3})),
    {name:'invalid-nonmatching-model',fixture:'invalidNonmatchingModel',item:1,exit:5},
    {name:'missing-item-argument',fixture:'baseline',exit:2},
    ...['0','-1','4294967296'].map(item=>({name:'invalid-item-'+item.replace('-','negative'),fixture:'baseline',item,exit:2})),
    {name:'invalid-race',fixture:'baseline',item:1,args:['--race-id','bad'],exit:2},
    {name:'invalid-race-overflow',fixture:'baseline',item:1,args:['--race-id','9223372036854775808'],exit:2},
    {name:'invalid-gender',fixture:'baseline',item:1,args:['--gender','bad'],exit:2},
    {name:'invalid-gender-underflow',fixture:'baseline',item:1,args:['--gender','-9223372036854775809'],exit:2},
  ];
  if(profileArg) {
    const selected=new Set(profileArg.split(','));
    assert([...selected].every(name=>profiles.some(profile=>profile.name===name)),'unknown profile name');
    profiles=profiles.filter(profile=>selected.has(profile.name));
  }
  const processes=[],results=[],diagnostics=[];
  function save() {
    writeFileSync(join(out,'report.json'),JSON.stringify({passed:results.length===profiles.length*2&&results.every(row=>row.passed),binarySHA256,results,diagnostics,processes,
      scope:'Seven authenticated synthetic CASC/DBD/WDC fixtures, ordinary offline Go/Rust CLI JSON/JSONL, complete stable envelopes, model/material/appearance/variant choices, all relations, and exact independently verified capture bodies/metadata. Shared resource counters and additive Rust verification declarations are audited separately. Large JSONL is retained exactly through an owned Node file sink; existing log limits remain fixed. No real-client, CDN, complete accounting or performance acceptance.'},null,2));
  }
  function execute(engine,name,args,large=false) {
    const evidence=join(out,name+'-'+engine);mkdirSync(evidence);
    const output=join(evidence,'stdout.jsonl');
    const run=large
      ?ownedSpawnSync(process.execPath,[self,'--capture-cli',binaries[engine],output,...args],{cwd:root,evidence,timeout:180000})
      :ownedSpawnSync(binaries[engine],args,{cwd:root,evidence,timeout:180000});
    const stdoutPath=large?output:run.logs.find(path=>path.endsWith('.stdout.bin'));
    const stderrPath=run.logs.find(path=>path.endsWith('.stderr.bin'));
    processes.push({engine,name,binary:binaries[engine],args,exit:run.status,handlesReaped:run.handlesReaped,largeFileSink:large,stdout:stdoutPath,stderr:stderrPath,proof:run.proof,error:run.error?.message});
    assert(!run.error&&run.handlesReaped,run.error?.message);assert.equal(run.stderr.toString(),'');
    if(large) {const sink=JSON.parse(run.stdout);assert.equal(sink.code,run.status);assert.equal(sink.signal,null);assert.equal(sink.binary,binaries[engine]);assert.deepEqual(sink.args,args);}
    return {exit:run.status,stdout:large?readFileSync(output).toString():run.stdout.toString(),stdoutPath};
  }
  function output(run,format) {
    if(format==='json')return JSON.parse(run.stdout);
    const frames=run.stdout.trimEnd().split(/\r?\n/).map(line=>JSON.parse(line));
    assert.deepEqual(frames.map(row=>row.type),run.exit===0?['begin','record','end']:['begin','error']);
    const envelope=frames.at(-1).envelope;
    assert.deepEqual(frames[0],{type:'begin',schema:envelope.schema});
    if(run.exit===0)assert.deepEqual(frames[1],{type:'record',result:envelope.result});
    return envelope;
  }
  function normalizePaths(value,home) {
    if(typeof value==='string')return value.split(home).join('<home>');
    if(Array.isArray(value))return value.map(row=>normalizePaths(row,home));
    if(value&&typeof value==='object')return Object.fromEntries(Object.entries(value).map(([key,row])=>[key,normalizePaths(row,home)]));
    return value;
  }
  function differences(a,b,path='',found=[]) {
    if(equal(a,b)||found.length>=12)return found;
    if(a&&b&&typeof a==='object'&&typeof b==='object') {
      for(const key of new Set([...Object.keys(a),...Object.keys(b)])) {differences(a[key],b[key],path+'/'+key,found);if(found.length>=12)break;}
    }else found.push({path,go:a,rust:b});
    return found;
  }
  function auditResult(value,engine,name) {
    const limits={retainedBytes:512<<20,metadataBytes:128<<20,decodeWork:100000000,networkRequests:4096,networkBytes:1<<30};
    assert.deepEqual(Object.keys(value.resources).sort(),Object.keys(limits).sort());
    for(const [field,max]of Object.entries(limits))assert(Number.isSafeInteger(value.resources[field])&&value.resources[field]>=0&&value.resources[field]<=max);
    assert.equal(value.resources.networkRequests,0);assert.equal(value.resources.networkBytes,0);
    assert.deepEqual(value.tables.map(table=>table.name),['ItemSparse','ItemModifiedAppearance','ItemAppearance','ItemDisplayInfo','ModelFileData','TextureFileData','ComponentModelFileData']);
    const audit={engine,name,resources:value.resources};
    if(engine==='rust') {
      const proof={encodingFramingEKey:true,encodingRequestedPagesMD5:true,encodingWholeDecodedCKey:false,rootWholeDecodedCKey:true,contentWholeDecodedCKey:true};
      assert.equal(value.tableVerification.length,7);for(const actual of value.tableVerification)assert.deepEqual(actual,proof);
      assert.equal(value.resourceAccounting.wholeQueryAccountingComplete,false);assert.equal(value.resourceAccounting.strategy,'conservative cumulative admission');
      assert.deepEqual(value.resourceAccounting.remainingGaps,['uninterruptible single codec call']);
      assert(value.resourceAccounting.wdcMetadataAdmittedBytes>0&&value.resourceAccounting.wdcMetadataAdmittedBytes<=value.resources.metadataBytes);
      audit.resourceAccounting=value.resourceAccounting;delete value.tableVerification;delete value.resourceAccounting;
    }
    diagnostics.push(audit);delete value.resources;
  }
  function exactSignedLimits(raw,copies) {
    for(const [field,decimal]of [['raceID','-9223372036854775808'],['gender','9223372036854775807']]) {
      const pattern=new RegExp('"'+field+'"\\s*:\\s*(-?\\d+)(?=\\s*[,}])','g');
      assert.deepEqual([...raw.matchAll(pattern)].map(match=>match[1]),Array(copies).fill(decimal));
    }
  }
  function verifyCapture(engine,name,env,fx) {
    assert.equal(env.captures.length,1);const capture=env.captures[0],home=fx.homes[engine],result=env.result;
    assert.match(capture.id,/^CAP-[0-9a-f]{64}$/);assert(Number.isFinite(Date.parse(capture.capturedAt)));
    assert.equal(capture.mediaType,'application/json');assert.equal(capture.complete,result.complete);assert.equal(capture.truncated,result.truncated);
    assert.deepEqual(capture.provenance,{kind:'item-models',locator:'item:models:'+result.itemID,snapshot:fx.identity.snapshot,operationId:'',dataBuild:fx.identity.pin.fullBuild});
    const path=join(home,'blobs',capture.blob.sha256.slice(0,2),capture.blob.sha256.slice(2)),bytes=readFileSync(path);
    assert.equal(hash(bytes),capture.blob.sha256);assert.equal(bytes.length,capture.blob.bytes);assert.deepEqual(JSON.parse(bytes),result);
    if(name.startsWith('variant-signed-limits-'))exactSignedLimits(bytes.toString(),1);
    const db=new DatabaseSync(join(home,'state/toolkit.sqlite'),{readOnly:true});
    try {const row=db.prepare('SELECT value FROM documents WHERE key=?').get('capture/'+capture.id);assert(row);assert.deepEqual(JSON.parse(Buffer.from(row.value)),capture);}finally{db.close();}
    const verify=execute(engine,name+'-capture',['evidence','verify',capture.id,'--home',home,'--format','json']);
    assert.equal(verify.exit,0);assert.deepEqual(JSON.parse(verify.stdout),{schema:'lycheedev.result.v1',ok:true,operationId:'',context:{},result:capture,captures:[],warnings:[],error:null});
    const identity={id:capture.id,blob:capture.blob,path,complete:capture.complete,truncated:capture.truncated,publicVerify:true};
    capture.id='<verified-capture>';capture.capturedAt='<time>';capture.blob={sha256:'<verified-captured-result>',bytes:'<verified-captured-result>'};return identity;
  }
  for(const profile of profiles)for(const format of ['json','jsonl']) {
    const name=profile.name+'-'+format,fx=fixtures[profile.fixture],runs={},errors={},contracts={};
    for(const engine of ['go','rust'])try {
      const args=['data','item','models',...(profile.item===undefined?[]:['--item-id',String(profile.item)]),...(profile.args||[]),
        '--snapshot',fx.identity.snapshot,'--installation',fx.identity.installation,'--home',fx.homes[engine],'--format',format,'--offline'];
      const run=execute(engine,name,args,profile.large&&format==='jsonl'),env=output(run,format);
      runs[engine]={exit:run.exit,stdout:run.stdoutPath,ok:env.ok,error:env.error};
      assert.equal(run.exit,profile.exit);assert.equal(env.ok,run.exit===0);assert.equal(env.schema,'lycheedev.result.v1');
      if(env.ok) {
        assert.equal(env.result.itemID,profile.item);
        for(const [field,wanted]of Object.entries(profile.expect||{}))assert.deepEqual(env.result[field],wanted);
        if(profile.name==='variant-signed-limits') {
          // JSON.parse rounds i64 extremes. Check their original decimal tokens
          // before the semantic comparison so this boundary is not normalized.
          exactSignedLimits(run.stdout,format==='jsonl'?2:1);
        }
        if(profile.large) {assert.equal(env.result.relations.length,65536);assert.equal(env.result.relations[65535].fromID,65533);}
        runs[engine].summary={itemID:env.result.itemID,displayID:env.result.displayID,models:env.result.models,textures:env.result.textures,complete:env.result.complete,partialReasons:env.result.partialReasons,truncated:env.result.truncated,truncationReason:env.result.truncationReason,relationCount:env.result.relations.length};
        runs[engine].capture=verifyCapture(engine,name,env,fx);auditResult(env.result,engine,name);
        contracts[engine]={exit:run.exit,envelope:env};
      }else {assert.deepEqual(env.captures,[]);contracts[engine]=normalizePaths({exit:run.exit,envelope:env},fx.homes[engine]);}
    }catch(error){errors[engine]=error.stack;}
    const passed=Object.keys(errors).length===0&&equal(contracts.go,contracts.rust);
    results.push({name,fixture:profile.fixture,format,passed,runs,errors,differences:Object.keys(errors).length?[]:differences(contracts.go,contracts.rust)});save();
    console.log(JSON.stringify({name,passed}));
  }
  for(const engine of ['go','rust'])assert.equal(hash(readFileSync(binaries[engine])),binarySHA256[engine]);
  save();console.log(JSON.stringify({passed:results.every(row=>row.passed),cases:results.length,processes:processes.length,failed:results.filter(row=>!row.passed).map(row=>row.name),report:join(out,'report.json')}));
  if(results.some(row=>!row.passed))process.exitCode=1;
}
