// Focused public spell-info parity. Uses existing binaries and authenticated
// fixtures; no build, provider implementation or real-client claim.
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

// JSONL repeats the complete result in record and end. The 65,537-row probe
// exceeds the existing owned wrapper's 16 MiB log cap. For both large probes
// this tiny owned parent streams exact CLI stdout to the case file, then waits
// for its exit. The existing Job owns and reaps both Node and CLI.
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
  assert(rustArg&&fixtureArg&&outArg,'usage: spell-info-public.mjs <cli.exe> <seven-fixture-directory> <new-output> [profile-name,...]');
  const root=process.cwd(),out=resolve(outArg),fixtureRoot=resolve(fixtureArg),self=fileURLToPath(import.meta.url);
  assert(!existsSync(out));mkdirSync(out,{recursive:true});
  const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
  const binaries={go:resolve('.tmp/lycheedev-oracle.exe'),rust:resolve(rustArg)};
  const binarySHA256=Object.fromEntries(Object.entries(binaries).map(([engine,path])=>[engine,hash(readFileSync(path))]));
  assert.equal(binarySHA256.go,'5738418756ee57c536518ff71223f2e7843ac3495a846b4f063d3104d83aae87');
  const fixtures={};
  for(const name of ['baseline','chain','firstInvalidThenMissing','missingRange','invalidNonmatchingEffect','rowBound','relationBound']) {
    const identity=JSON.parse(readFileSync(join(fixtureRoot,name,'input-identity.json'))),homes={};
    for(const engine of ['go','rust']) {homes[engine]=join(out,'homes',name+'-'+engine);cpSync(identity.home,homes[engine],{recursive:true});}
    fixtures[name]={identity,homes};
  }
  let profiles=[
    ...[100,10,30,40,50,999].map(seed=>({name:'seed-'+seed,fixture:'baseline',seed,exit:0})),
    {name:'charge-warm',fixture:'baseline',seed:100,exit:0},
    {name:'charge-depth-one',fixture:'baseline',seed:100,args:['--max-depth','1'],exit:0,expect:{chainDepth:1,totalCount:2,complete:false,truncationReason:'max_depth'}},
    {name:'charge-depth-zero',fixture:'baseline',seed:100,args:['--max-depth','0'],exit:0,expect:{chainDepth:2,complete:true}},
    {name:'chain-default',fixture:'chain',seed:1000,exit:0,expect:{chainDepth:5,totalCount:6,truncationReason:'max_depth'}},
    {name:'chain-depth-one',fixture:'chain',seed:1000,args:['--max-depth','1'],exit:0,expect:{chainDepth:1,totalCount:2,truncationReason:'max_depth'}},
    {name:'chain-depth-max',fixture:'chain',seed:1000,args:['--max-depth','32'],exit:0,expect:{chainDepth:32,totalCount:33,truncationReason:'max_depth'}},
    {name:'row-bound',fixture:'rowBound',seed:100,exit:0,large:true,expect:{complete:false,truncationReason:'row_budget',totalCount:1}},
    {name:'relation-bound',fixture:'relationBound',seed:100,exit:0,large:true,expect:{complete:false,truncationReason:'relation_budget',totalCount:2}},
    {name:'first-invalid-before-missing',fixture:'firstInvalidThenMissing',seed:100,exit:5},
    {name:'missing-range',fixture:'missingRange',seed:100,exit:3},
    {name:'missing-range-absent-seed',fixture:'missingRange',seed:999,exit:3},
    {name:'invalid-nonmatching-effect',fixture:'invalidNonmatchingEffect',seed:100,exit:5},
    {name:'missing-seed',fixture:'baseline',exit:2},
    ...['0','-1','4294967296'].map(seed=>({name:'invalid-seed-'+seed.replace('-','negative'),fixture:'baseline',seed,exit:2})),
    ...['-1','33','bad'].map(depth=>({name:'invalid-depth-'+depth.replace('-','negative'),fixture:'baseline',seed:100,args:['--max-depth',depth],exit:2})),
  ];
  if(profileArg) {
    const selected=new Set(profileArg.split(','));
    assert([...selected].every(name=>profiles.some(profile=>profile.name===name)),'unknown profile name');
    profiles=profiles.filter(profile=>selected.has(profile.name));
  }
  const processes=[],results=[],diagnostics=[];
  function save() {
    writeFileSync(join(out,'report.json'),JSON.stringify({passed:results.length===profiles.length*2&&results.every(row=>row.passed),binarySHA256,results,diagnostics,processes,
      scope:'Seven authenticated synthetic CASC/DBD/WDC fixtures, ordinary offline Go/Rust CLI JSON/JSONL, complete stable envelopes/relations/details and exact independently verified capture bodies/records. Shared resource counters and additive Rust verification declarations are audited separately. Each large JSONL stream is retained exactly through an owned Node file sink; existing process log limits are unchanged. No real-client, CDN, complete accounting or performance acceptance.'},null,2));
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
  function verifyCapture(engine,name,env,fx) {
    assert.equal(env.captures.length,1);const capture=env.captures[0],home=fx.homes[engine],result=env.result;
    assert.match(capture.id,/^CAP-[0-9a-f]{64}$/);assert(Number.isFinite(Date.parse(capture.capturedAt)));
    assert.equal(capture.mediaType,'application/json');assert.equal(capture.complete,result.complete);assert.equal(capture.truncated,result.truncated);
    assert.deepEqual(capture.provenance,{kind:'spell-info',locator:'spell:info:'+result.spellID,snapshot:fx.identity.snapshot,operationId:'',dataBuild:fx.identity.pin.fullBuild});
    const path=join(home,'blobs',capture.blob.sha256.slice(0,2),capture.blob.sha256.slice(2)),bytes=readFileSync(path);
    assert.equal(hash(bytes),capture.blob.sha256);assert.equal(bytes.length,capture.blob.bytes);assert.deepEqual(JSON.parse(bytes),result);
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
      const args=['data','spell','info',...(profile.seed===undefined?[]:['--spell-id',String(profile.seed)]),...(profile.args||[]),
        '--snapshot',fx.identity.snapshot,'--installation',fx.identity.installation,'--home',fx.homes[engine],'--format',format,'--offline'];
      const run=execute(engine,name,args,profile.large&&format==='jsonl'),env=output(run,format);
      runs[engine]={exit:run.exit,stdout:run.stdoutPath,ok:env.ok,error:env.error};
      assert.equal(run.exit,profile.exit);assert.equal(env.ok,run.exit===0);assert.equal(env.schema,'lycheedev.result.v1');
      if(env.ok) {
        for(const [field,wanted]of Object.entries(profile.expect||{}))assert.deepEqual(env.result[field],wanted);
        if(profile.name==='row-bound')assert.equal(env.result.spells['100'].effects.length,65536);
        if(profile.name==='relation-bound')assert.equal(env.result.relations.length,65536);
        runs[engine].summary={totalCount:env.result.totalCount,chainDepth:env.result.chainDepth,complete:env.result.complete,truncated:env.result.truncated,truncationReason:env.result.truncationReason,relationCount:env.result.relations.length};
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
