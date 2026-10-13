// Explicit, offline public-CLI differential acceptance. Never builds a binary.
import {readFileSync,writeFileSync,mkdirSync,existsSync,lstatSync,readdirSync} from 'node:fs';
import {resolve,join,dirname,relative,isAbsolute} from 'node:path';
import {createHash} from 'node:crypto';
import {DatabaseSync} from 'node:sqlite';
import {isDeepStrictEqual as equal} from 'node:util';
import {fileURLToPath} from 'node:url';
import {ownedSpawnSync} from '../../tools/rust-owned-process.mjs';
import {verifyArtifact,sourceDigest,hashFile} from '../../tools/rust-build.mjs';

const root=resolve(dirname(fileURLToPath(import.meta.url)),'../..');
const [artifactArg,fixtureArg,outputArg,caseArg]=process.argv.slice(2);
if(!artifactArg||!fixtureArg||!outputArg)throw Error('usage: node tests/rust-source-data/catalog-public.mjs <current-artifact.json> <suite-fixtures.json> <NEW-evidence-directory>');
const artifactPath=resolve(artifactArg),fixturePath=resolve(fixtureArg),out=resolve(outputArg);
const assert=(condition,message)=>{if(!condition)throw Error(message);};
assert(!existsSync(out),'catalog.new_evidence_directory_required');
const hash=raw=>createHash('sha256').update(raw).digest('hex');
const read=(path,max=16<<20)=>{const stat=lstatSync(path);assert(stat.isFile()&&!stat.isSymbolicLink()&&stat.size<=max,`catalog.file_budget: ${path}`);return readFileSync(path);};
const parse=raw=>JSON.parse(raw.toString(),(_key,value,context)=>{
  if(typeof value==='number'&&context?.source&&/^-?\d+$/.test(context.source)&&!Number.isSafeInteger(value))return BigInt(context.source);
  return value;
});
const stringify=value=>JSON.stringify(value,(_key,item)=>typeof item==='bigint'?{$exactInteger:item.toString()}:item,2)+'\n';
const save=(path,value)=>writeFileSync(path,stringify(value));
const goJSON=value=>JSON.stringify(value).replace(/[<>&\u2028\u2029]/g,char=>'\\u'+char.charCodeAt(0).toString(16).padStart(4,'0'));
const ordered=(object,keys)=>Object.fromEntries(keys.filter(key=>Object.hasOwn(object,key)).map(key=>[key,object[key]]));
const artifact=verifyArtifact(root,artifactPath),sourceBefore=sourceDigest(root);
assert(artifact.inputHashes.sourceTree===sourceBefore,'catalog.current_source_seal_required');
const binaries={go:join(root,'.tmp/lycheedev-oracle.exe'),rust:artifact.sourceBinary};
const binarySHA256=Object.fromEntries(Object.entries(binaries).map(([engine,path])=>[engine,hashFile(path,128<<20)]));
assert(binarySHA256.go==='5738418756ee57c536518ff71223f2e7843ac3495a846b4f063d3104d83aae87','catalog.frozen_go_oracle_required');
const configRaw=read(fixturePath),config=parse(configRaw);
assert(config.schema==='lycheedev.catalog-public-fixtures.v1','catalog.fixture_suite_schema');
const casePath=caseArg?resolve(caseArg):join(root,'tests/rust-source-data/catalog-public-cases.json'),caseRaw=read(casePath),matrix=parse(caseRaw);
assert(matrix.schema==='lycheedev.catalog-public-cases.v1'&&equal(matrix.formats,['json','jsonl']),'catalog.case_schema');
const allowedCommands=new Set(['data item get','data decor list','data decor get','data creature display','data creature model','data encounter get']);
assert(Array.isArray(matrix.cases)&&matrix.cases.length>0&&matrix.cases.length<=256&&matrix.cases.every(row=>Array.isArray(row.args)&&row.args.every(arg=>typeof arg==='string')&&allowedCommands.has(row.args.slice(0,3).join(' '))),'catalog.offline_domain_commands_only');
mkdirSync(out,{recursive:true});mkdirSync(join(out,'inputs'));mkdirSync(join(out,'runs'));mkdirSync(join(out,'homes'));
const processes=[],cases=[],fixtures={},diagnostics=[];
let sequence=0;

function inventory(directory){
  const files=[];let total=0;
  function visit(path){
    const stat=lstatSync(path);assert(!stat.isSymbolicLink(),'catalog.fixture_reparse');
    if(stat.isDirectory())for(const name of readdirSync(path).sort())visit(join(path,name));
    else {const raw=read(path);total+=raw.length;assert(files.length<128&&total<=16<<20,'catalog.fixture_inventory_budget');files.push({path:relative(directory,path).replaceAll('\\','/'),bytes:raw.length,sha256:hash(raw)});}
  }
  visit(directory);return files;
}
function contained(base,path){const suffix=relative(base,resolve(path));assert(suffix&&!suffix.startsWith('..')&&!isAbsolute(suffix),'catalog.fixture_path_outside_descriptor');}
function loadFixture(name,spec,retain){
  assert(/^[a-zA-Z][a-zA-Z0-9]*$/.test(name),'catalog.fixture_name');
  const descriptorPath=resolve(root,spec.identity),raw=read(descriptorPath),identity=parse(raw),base=dirname(descriptorPath);
  assert(hash(raw)===spec.sha256&&identity.schema==='lycheedev.catalog-navigation-fixture.v1','catalog.fixture_descriptor_identity');
  contained(base,identity.home);contained(base,identity.installation);
  const game=join(base,'game'),files=inventory(game),documents=[],blobs=[];
  const database=new DatabaseSync(join(identity.home,'state/toolkit.sqlite'),{readOnly:true});
  function document(key){const row=database.prepare('SELECT value FROM documents WHERE key=?').get(key);assert(row,'catalog.fixture_document_missing');const bytes=Buffer.from(row.value);assert(bytes.length<=1<<20,'catalog.document_budget');documents.push({key,bytes,sha256:hash(bytes)});return parse(bytes);}
  let pin,manifest;
  try{
    pin=document(`pin/${identity.snapshot}`);
    assert(pin.schema==='lycheedev.selection.v1'&&pin.id===identity.snapshot&&equal(pin.data,identity.pin),'catalog.fixture_pin_identity');
    const pinCanonical={schema:pin.schema,id:'',data:ordered(pin.data,['product','region','fullBuild','buildConfig','cdnConfig','language','definitionCommit'])};
    assert(equal({...pin,id:''},pinCanonical)&&'PIN-'+hash(goJSON(pinCanonical))===identity.snapshot,'catalog.fixture_pin_digest');
    const prefix=`https://raw.githubusercontent.com/wowdev/WoWDBDefs/${identity.pin.definitionCommit}/`;
    for(const suffix of ['manifest.json',...identity.tables.map(table=>`definitions/${table.name}.dbd`)]){
      const url=prefix+suffix,doc=document('definition/'+hash(url));
      assert(doc.url===url&&/^[0-9a-f]{64}$/.test(doc.blob?.sha256),'catalog.definition_identity');
      const bytes=read(join(identity.home,'blobs',doc.blob.sha256.slice(0,2),doc.blob.sha256.slice(2)));
      assert(bytes.length===doc.blob.bytes&&hash(bytes)===doc.blob.sha256,'catalog.definition_blob');
      blobs.push({suffix,...doc.blob,raw:bytes});
      if(suffix==='manifest.json')manifest=parse(bytes);
      else assert(identity.tables.find(table=>suffix===`definitions/${table.name}.dbd`).dbdSHA256===doc.blob.sha256,'catalog.dbd_identity');
    }
  }finally{database.close();}
  const idx=read(join(game,'Data/data/0000000001.idx')),archive=read(join(game,'Data/data/data.000'));
  assert(idx.length>=40&&(idx.length-40)%18===0,'catalog.index_layout');
  const rawTables=[];
  for(const table of identity.tables){
    const binding=manifest.find(row=>row.tableName===table.name);
    assert(binding?.db2FileDataID===table.fileDataID,'catalog.manifest_binding');
    let found;
    for(let at=40;at<idx.length;at+=18){
      const offset=idx.readUInt32BE(at+10),size=idx.readUInt32LE(at+14);
      assert(size>=30&&offset+size<=archive.length,'catalog.archive_span');
      const frame=archive.subarray(offset+30,offset+size);
      if(frame.length!==table.bytes+9||!frame.subarray(0,9).equals(Buffer.from('424c5445000000004e','hex')))continue;
      const wdc=frame.subarray(9);if(hash(wdc)!==table.sha256)continue;
      assert(wdc.subarray(0,4).toString()==='WDC3'&&wdc.readUInt32LE(24)===table.layout&&wdc.readUInt32LE(20)===Number.parseInt(binding.tableHash,16),'catalog.wdc_identity');
      found={...table,logicalRows:wdc.readUInt32LE(4),raw:wdc};break;
    }
    assert(found,`catalog.authenticated_table_absent: ${table.name}`);rawTables.push(found);
  }
  const receipt={descriptorPath,descriptorSHA256:hash(raw),identity,files,documents:documents.map(({bytes,...doc})=>({...doc,bytes:bytes.length})),blobs:blobs.map(({raw,...blob})=>blob),tables:rawTables.map(({raw,...table})=>table)};
  if(retain){
    const destination=join(out,'inputs',name);mkdirSync(destination);writeFileSync(join(destination,'input-identity.json'),raw);
    for(const [n,doc]of documents.entries())writeFileSync(join(destination,`document-${n}.json`),doc.bytes);
    for(const blob of blobs)writeFileSync(join(destination,blob.sha256+'.blob'),blob.raw);
    for(const table of rawTables)writeFileSync(join(destination,table.name+'.db2'),table.raw);
    save(join(destination,'verified-inputs.json'),receipt);
  }
  return {receipt,identity,documents,blobs,homes:{}};
}
function run(engine,args,label,cwd=root){
  const prefix=join(out,'runs',`${String(++sequence).padStart(4,'0')}-${label}-${engine}`);
  const execution=ownedSpawnSync(binaries[engine],args,{cwd,env:process.env,evidence:out,timeout:180000});
  const stdout=execution.stdout?.toString('utf8')??'',stderr=execution.stderr?.toString('utf8')??'';
  writeFileSync(prefix+'.stdout',stdout);writeFileSync(prefix+'.stderr',stderr);
  processes.push({engine,args,cwd,exit:execution.status,handlesReaped:execution.handlesReaped,error:execution.error?.message,proof:execution.proof,logs:execution.logs,stdout:prefix+'.stdout',stderr:prefix+'.stderr'});
  assert(!execution.error&&execution.handlesReaped,execution.error?.message??'catalog.process_reap_unverified');
  return {exit:execution.status,stdout,stderr};
}
function seed(name,fixture,engine){
  const home=join(out,'homes',`${name}-${engine}`);assert(!existsSync(home),'catalog.new_home_required');fixture.homes[engine]=home;
  const init=run(engine,['init','--home',home,'--format','json'],`${name}-init`);assert(init.exit===0&&parse(init.stdout).ok===true,'catalog.init_failed');
  const database=new DatabaseSync(join(home,'state/toolkit.sqlite'));
  try{
    database.exec('PRAGMA synchronous=FULL;BEGIN IMMEDIATE;CREATE TABLE IF NOT EXISTS documents(key TEXT PRIMARY KEY NOT NULL,generation INTEGER NOT NULL CHECK(generation>0),value BLOB NOT NULL CHECK(length(value)<=1048576)) WITHOUT ROWID;PRAGMA user_version=1');
    const insert=database.prepare('INSERT INTO documents(key,generation,value) VALUES(?,1,?)');
    for(const doc of fixture.documents)insert.run(doc.key,doc.bytes);
    const source={schema:'lycheedev.selection.v1',id:'',source:{repository:'wow-ui-source',product:'retail',requestedRef:'fixture',exactCommit:'b'.repeat(40),parserRevision:'fixture-v1'}};
    source.id='PIN-'+hash(goJSON(source));fixture.sourceSnapshot=source.id;
    insert.run('pin/'+source.id,Buffer.from(goJSON(source)));
    for(const blob of fixture.blobs){const parent=join(home,'blobs',blob.sha256.slice(0,2));mkdirSync(parent,{recursive:true});writeFileSync(join(parent,blob.sha256.slice(2)),blob.raw,{flag:'wx'});}
    database.exec('COMMIT');
  }finally{database.close();}
  save(join(home,'import-identity.json'),{schema:'lycheedev.catalog-public-import.v1',engine,source:fixture.receipt.descriptorSHA256,documents:fixture.receipt.documents,blobs:fixture.receipt.blobs});
  for(const kind of ['locked','unlocked','missing']){
    const directory=join(home,'projects',kind);mkdirSync(join(directory,'nested'),{recursive:true});
    if(kind!=='missing')save(join(directory,'lycheedev.json'),{schema:'lycheedev.project.v1',product:fixture.identity.pin.product});
    if(kind==='locked')save(join(directory,'lycheedev.lock.json'),{schema:'lycheedev.project-lock.v1',selection:parse(fixture.documents.find(doc=>doc.key==='pin/'+fixture.identity.snapshot).bytes)});
  }
}
function parseOutput(stdout,format,exit){
  if(format==='json')return {envelope:parse(stdout),frames:null};
  const frames=stdout.trimEnd().split(/\r?\n/).map(parse),expected=exit===0?['begin','record','end']:['begin','error'];
  assert(equal(frames.map(frame=>frame.type),expected),'catalog.jsonl_terminal_shape');
  const envelope=frames.at(-1).envelope;
  assert(equal(frames[0],{type:'begin',schema:envelope.schema}),'catalog.jsonl_begin');
  if(exit===0)assert(equal(frames[1],{type:'record',result:envelope.result}),'catalog.jsonl_record');
  return {envelope,frames};
}
function auditedResult(result,engine,label){
  if(result===undefined)return result;
  const copy=structuredClone(result),limits={retainedBytes:512<<20,metadataBytes:128<<20,decodeWork:100000000,networkRequests:4096,networkBytes:1<<30};
  assert(equal(Object.keys(result.resources??{}).sort(),Object.keys(limits).sort()),'catalog.resources_shape');
  for(const [key,limit]of Object.entries(limits))assert(Number.isSafeInteger(result.resources[key])&&result.resources[key]>=0&&result.resources[key]<=limit,`catalog.resources_${key}`);
  assert(result.resources.networkRequests===0&&result.resources.networkBytes===0,'catalog.offline_network_usage');
  const audit={label,engine,resources:result.resources};copy.resources='<independently-audited-resource-counters>';
  if(engine==='rust'){
    const proof={encodingFramingEKey:true,encodingRequestedPagesMD5:true,encodingWholeDecodedCKey:false,rootWholeDecodedCKey:true,contentWholeDecodedCKey:true};
    assert(Array.isArray(result.tableVerification)&&result.tableVerification.length===result.tables.length&&result.tableVerification.every(value=>equal(value,proof)),'catalog.table_verification');
    const accounting={wholeQueryAccountingComplete:false,strategy:'conservative cumulative admission',wdcMetadataAdmittedBytes:result.resourceAccounting?.wdcMetadataAdmittedBytes,remainingGaps:['uninterruptible single codec call']};
    assert(equal(result.resourceAccounting,accounting)&&Number.isSafeInteger(accounting.wdcMetadataAdmittedBytes)&&accounting.wdcMetadataAdmittedBytes>0&&accounting.wdcMetadataAdmittedBytes<=result.resources.metadataBytes,'catalog.resource_accounting');
    audit.tableVerification=result.tableVerification;audit.resourceAccounting=result.resourceAccounting;delete copy.tableVerification;delete copy.resourceAccounting;
  }else assert(!Object.hasOwn(result,'tableVerification')&&!Object.hasOwn(result,'resourceAccounting'),'catalog.go_diagnostics_changed');
  diagnostics.push(audit);return copy;
}
function normalizeHome(value,home){
  if(typeof value==='string')return value.split(home).join('<isolated-home>');
  if(Array.isArray(value))return value.map(item=>normalizeHome(item,home));
  if(value&&typeof value==='object')return Object.fromEntries(Object.entries(value).map(([key,item])=>[key,normalizeHome(item,home)]));
  return value;
}
function captureIdentity(capture){
  return {schema:capture.schema,id:'',blob:ordered(capture.blob,['sha256','bytes']),mediaType:capture.mediaType,provenance:ordered(capture.provenance,['kind','locator','snapshot','operationId','sourceCommit','baseSnapshot','baseSourceCommit','dataBuild','session']),complete:capture.complete,truncated:capture.truncated,capturedAt:capture.capturedAt};
}
function validateRun(execution,format,engine,fixture,label){
  const {envelope,frames}=parseOutput(execution.stdout,format,execution.exit),home=fixture.homes[engine];
  assert(envelope.schema==='lycheedev.result.v1'&&envelope.ok===(execution.exit===0),'catalog.envelope_status');
  const result=envelope.ok?auditedResult(envelope.result,engine,label):envelope.result;
  const normalized=structuredClone(envelope);if(envelope.ok)normalized.result=result;
  const captureProof=[];
  assert(!envelope.ok||Array.isArray(envelope.captures)&&envelope.captures.length>0,'catalog.success_capture_required');
  const encounter=envelope.ok&&Object.hasOwn(envelope.result,'journalEncounterID');
  if(encounter)assert(envelope.captures.length===2,'catalog.encounter_two_captures');
  for(const [index,capture]of (envelope.captures??[]).entries()){
    assert(/^CAP-[0-9a-f]{64}$/.test(capture.id)&&Number.isFinite(Date.parse(capture.capturedAt))&&/^[0-9a-f]{64}$/.test(capture.blob?.sha256),'catalog.capture_identity_shape');
    const canonical=captureIdentity(capture);
    assert(equal({...capture,id:''},canonical)&&capture.id==='CAP-'+hash(goJSON(canonical)),'catalog.capture_identity_digest');
    const raw=read(join(home,'blobs',capture.blob.sha256.slice(0,2),capture.blob.sha256.slice(2)));
    assert(hash(raw)===capture.blob.sha256&&raw.length===capture.blob.bytes&&capture.mediaType==='application/json','catalog.capture_blob');
    const body=encounter&&index===1?{operation:'encounter_get',root:`journal_encounter:${envelope.result.journalEncounterID}`,snapshot:envelope.result.snapshot,dataBuild:envelope.result.pin.fullBuild,relations:envelope.result.relations,relatedFiles:envelope.result.relatedFiles,complete:envelope.result.complete,truncated:envelope.result.truncated,truncationReason:envelope.result.truncationReason}:envelope.result;
    assert(equal(parse(raw),body),'catalog.capture_public_body');
    if(encounter){
      const kind=index===0?'encounter-get':'relation-manifest',locator=`encounter:${index===0?'get':'relations'}:${envelope.result.journalEncounterID}`;
      assert(capture.provenance.kind===kind&&capture.provenance.locator===locator&&capture.provenance.snapshot===envelope.result.snapshot&&capture.provenance.dataBuild===envelope.result.pin.fullBuild&&capture.complete===envelope.result.complete&&capture.truncated===envelope.result.truncated,'catalog.encounter_capture_contract');
    }
    const database=new DatabaseSync(join(home,'state/toolkit.sqlite'),{readOnly:true});
    try{const row=database.prepare('SELECT value FROM documents WHERE key=?').get('capture/'+capture.id);assert(row&&equal(parse(Buffer.from(row.value)),capture),'catalog.capture_metadata');}finally{database.close();}
    const check=run(engine,['evidence','verify',capture.id,'--home',home,'--format','json'],label+'-capture');
    const verification=parse(check.stdout),expectedVerification={schema:'lycheedev.result.v1',ok:true,operationId:'',context:{},result:capture,captures:[],warnings:[],error:null};
    assert(check.exit===0&&check.stderr===''&&equal(verification,expectedVerification),'catalog.public_evidence_verify');
    captureProof.push({id:capture.id,blob:capture.blob,capturedAt:capture.capturedAt,identityVerified:true,bodyContract:encounter&&index===1?'exact relation manifest derived from public result':'exact public result',metadataVerified:true,publicVerify:true,publicVerifyEnvelope:verification});
    normalized.captures[index]={...capture,id:`<verified-capture-${index}>`,capturedAt:'<verified-capture-time>',blob:{sha256:'<verified-result-blob>',bytes:'<verified-result-bytes>'}};
  }
  const normalizedFrames=frames?.map(frame=>frame.type==='record'?{...frame,result}:frame.envelope?{...frame,envelope:normalized}:frame)??null;
  return {rawEnvelope:envelope,captureProof,contract:normalizeHome({exit:execution.exit,stderr:execution.stderr.replaceAll('\r\n','\n'),envelope:normalized,frames:normalizedFrames},home)};
}
let failure;
try{
  for(const [name,spec]of Object.entries(config.fixtures)){fixtures[name]=loadFixture(name,spec,true);for(const engine of ['go','rust'])seed(name,fixtures[name],engine);}
  for(const test of matrix.cases)for(const format of matrix.formats){
    const fixture=fixtures[test.fixture];assert(fixture,'catalog.case_fixture_missing');const runs={},label=`${test.id}-${format}`;
    for(const engine of ['go','rust']){
      const home=fixture.homes[engine],selection=test.selection??'snapshot';
      assert(['snapshot','none','sourceOnly','projectLocked','projectNearest','projectUnlocked','projectMissing','snapshotWins'].includes(selection),'catalog.selection_mode');
      const explicit=['snapshot','sourceOnly','snapshotWins'].includes(selection)?['--snapshot',selection==='sourceOnly'?fixture.sourceSnapshot:fixture.identity.snapshot]:[];
      const kind=selection==='projectUnlocked'?'unlocked':selection==='projectLocked'||selection==='projectNearest'?'locked':'missing';
      const project=['projectLocked','projectUnlocked','projectMissing','snapshotWins'].includes(selection)?['--project',join(home,'projects',kind)]:[];
      const args=[...test.args,...explicit,...project,...(test.omitInstallation?[]:['--installation',fixture.identity.installation]),'--home',home,'--format',format,'--offline'];
      const cwd=selection==='projectNearest'?join(home,'projects','locked','nested'):home;
      const execution=run(engine,args,label,cwd),validated=validateRun(execution,format,engine,fixture,label);
      const assertions=[{path:'exit',expected:test.exit,actual:execution.exit,passed:execution.exit===test.exit}];
      for(const [path,expected]of Object.entries(test.expect??{})){const actual=path.split('.').reduce((value,key)=>value?.[key],validated.rawEnvelope);assertions.push({path,expected,actual,passed:equal(actual,expected)});}
      runs[engine]={...validated,assertions};delete runs[engine].rawEnvelope;
    }
    const passed=equal(runs.go.contract,runs.rust.contract)&&Object.values(runs).every(run=>run.assertions.every(check=>check.passed));
    cases.push({id:test.id,fixture:test.fixture,format,args:test.args,passed,...runs});save(join(out,'cases-so-far.json'),cases);
  }
}catch(error){failure={message:error.message,stack:error.stack};}
let stability;
try{
  verifyArtifact(root,artifactPath);
  stability={source:sourceDigest(root)===sourceBefore,config:hash(read(fixturePath))===hash(configRaw),cases:hash(read(casePath))===hash(caseRaw),binaries:Object.entries(binaries).every(([engine,path])=>hashFile(path,128<<20)===binarySHA256[engine]),fixtures:Object.entries(fixtures).every(([name,fixture])=>equal(fixture.receipt,loadFixture(name,config.fixtures[name],false).receipt))};
}catch(error){stability={error:error.message};}
const passed=!failure&&cases.length===matrix.cases.length*matrix.formats.length&&cases.every(test=>test.passed)&&Object.values(stability).every(value=>value===true);
const report={schema:'lycheedev.catalog-public-parity.v1',scope:'Synthetic authenticated local item/decor inputs; offline public JSON and JSONL parity, independent homes, full envelopes and independently verified captures. Explicit resource-counter differences and additive Rust proof/accounting declarations are separately audited; not real-client, remote/CDN, whole-query accounting or performance acceptance.',artifactPath,artifact,sourceSHA256:sourceBefore,binarySHA256,configSHA256:hash(configRaw),caseMatrixSHA256:hash(caseRaw),inputs:Object.fromEntries(Object.entries(fixtures).map(([name,fixture])=>[name,fixture.receipt])),stability,failure,diagnostics,cases,passed};
save(join(out,'report.json'),report);save(join(out,'process-proof.json'),{schema:'lycheedev.catalog-public-process-proof.v1',runs:processes});
console.log(stringify({passed,cases:cases.length,failedCases:cases.filter(test=>!test.passed).map(test=>`${test.id}/${test.format}`),failure,report:join(out,'report.json')}));
if(!passed)process.exitCode=1;
