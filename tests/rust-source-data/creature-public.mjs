// Offline creature business differential. This entry never builds or generates
// fixtures. The original 17 cases remain an input; extra cases name the exact
// fixture construction contract for the next authorized Rust preparation.
// Usage: node creature-public.mjs <cli.exe> <fixture-root> <NEW-output> [case-id,...]
import assert from 'node:assert/strict';
import {cpSync,createWriteStream,existsSync,mkdirSync,readFileSync,writeFileSync} from 'node:fs';
import {resolve,join,dirname,relative,isAbsolute} from 'node:path';
import {createHash} from 'node:crypto';
import {DatabaseSync} from 'node:sqlite';
import {isDeepStrictEqual as equal} from 'node:util';
import {fileURLToPath} from 'node:url';
import {spawn} from 'node:child_process';
import {pipeline} from 'node:stream/promises';
import {ownedSpawnSync} from '../../tools/rust-owned-process.mjs';

const self=fileURLToPath(import.meta.url),here=dirname(self);
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
const parse=(raw,tokens)=>JSON.parse(String(raw),(_key,value,context)=>{
  if(typeof value==='number'&&context?.source&&/^-?\d+$/.test(context.source)) {
    tokens?.push(context.source);
    if(!Number.isSafeInteger(value))return BigInt(context.source);
  }
  return value;
});
const stringify=value=>JSON.stringify(value,(_key,item)=>typeof item==='bigint'?{$exactInteger:String(item)}:item,2)+'\n';
function expected(value) {
  if(value&&typeof value==='object'&&Object.keys(value).length===1&&typeof value.$exactInteger==='string')return BigInt(value.$exactInteger);
  if(Array.isArray(value))return value.map(expected);
  if(value&&typeof value==='object')return Object.fromEntries(Object.entries(value).map(([key,item])=>[key,expected(item)]));
  return value;
}
function rawIntegers(raw,format,values,multiple) {
  if(!values?.length)return;
  const tokens=[];
  for(const line of format==='jsonl'?String(raw).trimEnd().split(/\r?\n/):[String(raw)])parse(line,tokens);
  for(const value of values??[])assert.equal(tokens.filter(token=>token===value).length,multiple,'exact raw decimal token '+value);
}
function normalized(value,home) {
  if(typeof value==='string')return value.split(home).join('<home>');
  if(Array.isArray(value))return value.map(item=>normalized(item,home));
  if(value&&typeof value==='object')return Object.fromEntries(Object.entries(value).map(([key,item])=>[key,normalized(item,home)]));
  return value;
}
function differences(a,b,path='',found=[]) {
  if(equal(a,b)||found.length>=12)return found;
  if(a&&b&&typeof a==='object'&&typeof b==='object') {
    for(const key of new Set([...Object.keys(a),...Object.keys(b)])) {
      differences(a[key],b[key],path+'/'+key,found);if(found.length>=12)break;
    }
  }else found.push({path,go:a,rust:b});
  return found;
}
const displayLink=id=>({fromTable:'CreatureDisplayInfo',fromField:'ModelID',fromID:id,toKind:'creature_display',toID:id,depth:0});
const modelLink=id=>({fromTable:'CreatureModelData',fromField:'FileDataID',fromID:id,toKind:'model_file',toID:500,depth:1});
function boundary(result,test) {
  if(test.check==='geosetBound') {
    assert.deepEqual(result.variations,Array.from({length:65536},(_,i)=>101+i));
    assert.deepEqual(result.relations,[modelLink(2)]);
  }else if(['relationExact','relationBound','displayRowBound'].includes(test.check)) {
    assert.deepEqual([result.displayID,result.modelID,result.fileDataID,result.modelFileDataID,result.textures,result.variations],[1,2,500,500,[],[]]);
    const relations=Array.from({length:test.check==='relationExact'?65535:65536},(_,i)=>displayLink(i+1));
    if(test.check==='relationExact')relations.push(modelLink(2));
    assert.deepEqual(result.relations,relations);
  }else if(test.check==='modelBound') {
    const display={displayID:10,modelID:1,fileDataID:500,textures:[],variations:[]};
    if(test.args[2]==='model') {
      assert.deepEqual(result.displays,[display]);assert.deepEqual(result.relations,[displayLink(10)]);
    }else {
      for(const [key,value]of Object.entries(display))assert.deepEqual(result[key],value);
      assert.equal(result.modelFileDataID,500);assert.deepEqual(result.relations,[displayLink(10),modelLink(1)]);
    }
  }
}
function captureKeys(home) {
  const db=new DatabaseSync(join(home,'state/toolkit.sqlite'),{readOnly:true});
  try{return db.prepare("SELECT key FROM documents WHERE key LIKE 'capture/%' ORDER BY key").all().map(row=>row.key);}
  finally{db.close();}
}

// An owned parent streams the complete large JSONL result and waits for its
// child, exactly as in the item runners. The 16 MiB owned-process log cap stays.
if(process.argv[2]==='--capture-cli') {
  const [binary,path,...args]=process.argv.slice(3);
  const child=spawn(binary,args,{windowsHide:true,shell:false,stdio:['ignore','pipe','pipe']});
  const closed=new Promise((ok,reject)=>{child.once('error',reject);child.once('close',(code,signal)=>ok({code,signal}));});
  child.stderr.pipe(process.stderr,{end:false});
  const [,status]=await Promise.all([pipeline(child.stdout,createWriteStream(path,{flags:'wx'})),closed]);
  console.log(JSON.stringify({binary,args,stdout:path,...status}));process.exitCode=status.code??8;
}else await main();

async function main() {
  const [rustArg,fixtureArg,outArg,selection]=process.argv.slice(2);
  assert(rustArg&&fixtureArg&&outArg,'usage: creature-public.mjs <cli.exe> <fixture-root> <NEW-output> [case-id,...]');
  assert.equal(parse('{"value":-9223372036854775709}').value,-9223372036854775709n,'Node JSON reviver source support required');
  const root=process.cwd(),fixtureRoot=resolve(fixtureArg),out=resolve(outArg);
  const basePath=join(here,'creature-public-cases.json'),extraPath=join(here,'creature-public-extra-cases.json');
  const baseRaw=readFileSync(basePath),extraRaw=readFileSync(extraPath),base=parse(baseRaw),extra=parse(extraRaw);
  assert.equal(base.schema,'lycheedev.catalog-public-cases.v1');assert.equal(base.cases.length,17);assert.deepEqual(base.formats,['json','jsonl']);
  assert.equal(extra.schema,'lycheedev.creature-public-extra-cases.v1');
  let cases=[...base.cases,...extra.cases];
  assert.equal(new Set(cases.map(test=>test.id)).size,cases.length);
  if(selection) {
    const ids=new Set(selection.split(','));assert([...ids].every(id=>cases.some(test=>test.id===id)),'unknown selected case');
    cases=cases.filter(test=>ids.has(test.id));
  }
  for(const test of cases) {
    assert(extra.fixtures[test.fixture],'fixture recipe missing');
    assert(['data creature display','data creature model'].includes(test.args.slice(0,3).join(' ')));
  }
  const binaries={go:resolve('.tmp/lycheedev-oracle.exe'),rust:resolve(rustArg)};
  const binarySHA256=Object.fromEntries(Object.entries(binaries).map(([engine,path])=>[engine,hash(readFileSync(path))]));
  assert.equal(binarySHA256.go,'5738418756ee57c536518ff71223f2e7843ac3495a846b4f063d3104d83aae87');
  const fixtures={};
  for(const name of new Set(cases.map(test=>test.fixture))) {
    const directory=join(fixtureRoot,name),descriptor=join(directory,'input-identity.json'),raw=readFileSync(descriptor),identity=parse(raw);
    assert.equal(identity.schema,'lycheedev.catalog-navigation-fixture.v1');
    for(const path of [identity.home,identity.installation]) {
      const child=relative(directory,path);assert(child&&!child.startsWith('..')&&!isAbsolute(child),'fixture path outside its prepared directory');
    }
    assert.deepEqual(captureKeys(identity.home),[],'prepared fixture home must contain no captures');
    fixtures[name]={identity,descriptor,descriptorSHA256:hash(raw),homes:{}};
  }
  assert(!existsSync(out),'new output directory required');mkdirSync(out,{recursive:true});
  for(const [name,fx]of Object.entries(fixtures))for(const engine of ['go','rust']) {
    const home=join(out,'homes',name+'-'+engine);cpSync(fx.identity.home,home,{recursive:true});fx.homes[engine]=home;
  }
  const results=[],processes=[],diagnostics=[];
  const scope='Authenticated synthetic local creature display/model business. Full stable envelopes, JSONL frames, exact integer tokens, complete relations and capture bodies are compared. Resource counters and existing Rust declarations are audited separately. No real-client, CDN, full resource-accounting or performance acceptance.';
  function save() {
    writeFileSync(join(out,'report.json'),stringify({schema:'lycheedev.creature-public-parity.v1',scope,binarySHA256,
      inputs:Object.fromEntries(Object.entries(fixtures).map(([name,fx])=>[name,{descriptor:fx.descriptor,sha256:fx.descriptorSHA256,recipe:extra.fixtures[name]}])),
      selectedCases:cases.map(test=>test.id),baseCasesSHA256:hash(baseRaw),extraCasesSHA256:hash(extraRaw),
      passed:results.length===cases.length*2&&results.every(result=>result.passed),results,diagnostics,processes}));
  }
  function execute(engine,name,args,large=false) {
    const evidence=join(out,name+'-'+engine);mkdirSync(evidence);
    const output=join(evidence,'stdout.jsonl');
    const run=large?ownedSpawnSync(process.execPath,[self,'--capture-cli',binaries[engine],output,...args],{cwd:root,evidence,timeout:180000})
      :ownedSpawnSync(binaries[engine],args,{cwd:root,evidence,timeout:180000});
    const stdout=large?output:run.logs.find(path=>path.endsWith('.stdout.bin'));
    const stderr=run.logs.find(path=>path.endsWith('.stderr.bin'));
    processes.push({engine,name,binary:binaries[engine],args,exit:run.status,handlesReaped:run.handlesReaped,largeFileSink:large,stdout,stderr,proof:run.proof,error:run.error?.message});
    assert(!run.error&&run.handlesReaped,run.error?.message);assert.equal(run.stderr.toString(),'');
    if(large) {
      const sink=parse(run.stdout);assert.equal(sink.code,run.status);assert.equal(sink.signal,null);assert.equal(sink.binary,binaries[engine]);assert.deepEqual(sink.args,args);
    }
    return {exit:run.status,stdout:large?readFileSync(output).toString():run.stdout.toString(),stdoutPath:stdout};
  }
  function output(run,format) {
    if(format==='json')return {env:parse(run.stdout),frames:null};
    const frames=run.stdout.trimEnd().split(/\r?\n/).map(line=>parse(line)),env=frames.at(-1).envelope;
    assert.deepEqual(frames.map(frame=>frame.type),run.exit===0?['begin','record','end']:['begin','error']);
    assert.deepEqual(frames[0],{type:'begin',schema:env.schema});
    assert.deepEqual(frames.at(-1),{type:run.exit===0?'end':'error',envelope:env});
    if(run.exit===0)assert.deepEqual(frames[1],{type:'record',result:env.result});
    return {env,frames};
  }
  function audit(result,engine,name) {
    const limits={retainedBytes:512<<20,metadataBytes:128<<20,decodeWork:100000000,networkRequests:4096,networkBytes:1<<30};
    assert.deepEqual(Object.keys(result.resources).sort(),Object.keys(limits).sort());
    for(const [field,max]of Object.entries(limits))assert(Number.isSafeInteger(result.resources[field])&&result.resources[field]>=0&&result.resources[field]<=max);
    assert.equal(result.resources.networkRequests,0);assert.equal(result.resources.networkBytes,0);
    assert.deepEqual(result.tables.map(table=>table.name),['CreatureDisplayInfo','CreatureModelData','CreatureDisplayInfoGeosetData']);
    const diagnostic={engine,name,resources:result.resources};
    if(engine==='rust') {
      const proof={encodingFramingEKey:true,encodingRequestedPagesMD5:true,encodingWholeDecodedCKey:false,rootWholeDecodedCKey:true,contentWholeDecodedCKey:true};
      assert.deepEqual(result.tableVerification,[proof,proof,proof]);
      const accounting={wholeQueryAccountingComplete:false,strategy:'conservative cumulative admission',wdcMetadataAdmittedBytes:result.resourceAccounting?.wdcMetadataAdmittedBytes,
        remainingGaps:['uninterruptible single codec call']};
      assert.deepEqual(result.resourceAccounting,accounting);
      assert(Number.isSafeInteger(accounting.wdcMetadataAdmittedBytes)&&accounting.wdcMetadataAdmittedBytes>0&&accounting.wdcMetadataAdmittedBytes<=result.resources.metadataBytes);
      diagnostic.tableVerification=result.tableVerification;diagnostic.resourceAccounting=accounting;
      delete result.tableVerification;delete result.resourceAccounting;
    }else {assert(!Object.hasOwn(result,'tableVerification'));assert(!Object.hasOwn(result,'resourceAccounting'));}
    diagnostics.push(diagnostic);delete result.resources;
  }
  function verifyCapture(engine,name,env,fx,test) {
    assert.equal(env.captures.length,1);
    const capture=env.captures[0],home=fx.homes[engine],result=env.result;
    assert.match(capture.id,/^CAP-[0-9a-f]{64}$/);assert.match(capture.blob.sha256,/^[0-9a-f]{64}$/);assert(Number.isFinite(Date.parse(capture.capturedAt)));
    const ordered=(object,keys)=>Object.fromEntries(keys.filter(key=>Object.hasOwn(object,key)).map(key=>[key,object[key]]));
    const canonical={schema:capture.schema,id:'',blob:ordered(capture.blob,['sha256','bytes']),mediaType:capture.mediaType,
      provenance:ordered(capture.provenance,['kind','locator','snapshot','operationId','sourceCommit','baseSnapshot','baseSourceCommit','dataBuild','session']),
      complete:capture.complete,truncated:capture.truncated,capturedAt:capture.capturedAt};
    const encoded=JSON.stringify(canonical).replace(/[<>&\u2028\u2029]/g,char=>'\\u'+char.charCodeAt(0).toString(16).padStart(4,'0'));
    assert.deepEqual({...capture,id:''},canonical);assert.equal(capture.id,'CAP-'+hash(encoded));
    assert.equal(capture.mediaType,'application/json');assert.equal(capture.complete,result.complete);assert.equal(capture.truncated,result.truncated);
    const kind=test.args[2],flag=kind==='model'?'--file-data-id':test.args.includes('--display-id')?'--display-id':'--file-data-id';
    const id=String(Number(test.args[test.args.indexOf(flag)+1]));
    const locator='creature:'+kind+':'+(kind==='display'&&flag==='--file-data-id'?'file:':'')+id;
    assert.deepEqual(capture.provenance,{kind:'creature-'+kind,locator,snapshot:fx.identity.snapshot,operationId:'',dataBuild:fx.identity.pin.fullBuild});
    const path=join(home,'blobs',capture.blob.sha256.slice(0,2),capture.blob.sha256.slice(2)),bytes=readFileSync(path);
    assert.equal(hash(bytes),capture.blob.sha256);assert.equal(bytes.length,capture.blob.bytes);assert.deepEqual(parse(bytes),result);
    rawIntegers(bytes,'json',test.rawIntegers,1);
    const db=new DatabaseSync(join(home,'state/toolkit.sqlite'),{readOnly:true});
    try {const row=db.prepare('SELECT value FROM documents WHERE key=?').get('capture/'+capture.id);assert(row);assert.deepEqual(parse(Buffer.from(row.value)),capture);}finally{db.close();}
    const verify=execute(engine,name+'-capture',['evidence','verify',capture.id,'--home',home,'--format','json']);
    assert.equal(verify.exit,0);assert.deepEqual(parse(verify.stdout),{schema:'lycheedev.result.v1',ok:true,operationId:'',context:{},result:capture,captures:[],warnings:[],error:null});
    const identity={id:capture.id,blob:capture.blob,path,complete:capture.complete,truncated:capture.truncated,publicVerify:true,exactIntegerTokens:test.rawIntegers??[]};
    capture.id='<verified-capture>';capture.capturedAt='<time>';capture.blob={sha256:'<verified-result>',bytes:'<verified-result>'};return identity;
  }
  for(const test of cases)for(const format of ['json','jsonl']) {
    const name=test.id+'-'+format,fx=fixtures[test.fixture],runs={},errors={},contracts={};
    for(const engine of ['go','rust'])try {
      const home=fx.homes[engine],before=captureKeys(home);
      const args=[...test.args,'--snapshot',fx.identity.snapshot,'--installation',fx.identity.installation,'--home',home,'--format',format,'--offline'];
      const run=execute(engine,name,args,test.large&&format==='jsonl'),{env,frames}=output(run,format);
      runs[engine]={exit:run.exit,stdout:run.stdoutPath,ok:env.ok,error:env.error};
      assert.equal(run.exit,test.exit);assert.equal(env.ok,run.exit===0);assert.equal(env.schema,'lycheedev.result.v1');
      for(const [path,wanted]of Object.entries(test.expect??{}))assert.deepEqual(path.split('.').reduce((value,key)=>value?.[key],env),expected(wanted),path);
      if(env.ok) {
        boundary(env.result,test);rawIntegers(run.stdout,format,test.rawIntegers,format==='jsonl'?2:1);
        runs[engine].summary={displayID:env.result.displayID,fileDataID:env.result.fileDataID,displayCount:env.result.displays?.length,
          variationCount:env.result.variations?.length,relationCount:env.result.relations.length,complete:env.result.complete,partialReasons:env.result.partialReasons,truncationReason:env.result.truncationReason};
        runs[engine].capture=verifyCapture(engine,name,env,fx,test);audit(env.result,engine,name);
      }else {assert.deepEqual(env.captures,[]);assert.deepEqual(captureKeys(home),before,'error published a capture');}
      const normalizedFrames=frames?.map(frame=>frame.type==='record'?{...frame,result:env.result}:frame.envelope?{...frame,envelope:env}:frame)??null;
      contracts[engine]=normalized({exit:run.exit,envelope:env,frames:normalizedFrames},home);
    }catch(error){errors[engine]=error.stack;}
    const passed=Object.keys(errors).length===0&&equal(contracts.go,contracts.rust);
    results.push({name,fixture:test.fixture,format,passed,runs,errors,differences:Object.keys(errors).length?[]:differences(contracts.go,contracts.rust)});save();
    console.log(JSON.stringify({name,passed}));
  }
  for(const engine of ['go','rust'])assert.equal(hash(readFileSync(binaries[engine])),binarySHA256[engine]);
  assert.equal(hash(readFileSync(basePath)),hash(baseRaw));assert.equal(hash(readFileSync(extraPath)),hash(extraRaw));
  for(const fx of Object.values(fixtures))assert.equal(hash(readFileSync(fx.descriptor)),fx.descriptorSHA256);
  save();console.log(JSON.stringify({passed:results.every(result=>result.passed),profiles:results.length,processes:processes.length,failed:results.filter(result=>!result.passed).map(result=>result.name),report:join(out,'report.json')}));
  if(results.some(result=>!result.passed))process.exitCode=1;
}
