// Fresh independent Go expectations for the explicit local-client Data tests.
// Importing this module never launches a process. Only MAIN executes preparation.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import {performance} from 'node:perf_hooks';
import {beginScratch, contained} from '../../tools/rust-disk.mjs';
import {hashFile, ownedUserEnvironment} from '../../tools/rust-build.mjs';
import {ownedSpawnSync} from '../../tools/rust-owned-process.mjs';
import {parseAgentJson, isExactNumber} from '../../tools/rust-agent-client.mjs';
import {verifyGoPackage} from '../rust-performance-paired/inputs.mjs';

export const SCHEMA = 'lycheedev.local-data-go-oracle.v1';
export const GO_COMMIT = '29bdbdaff5f064ec212ff2064380b1ccac652eda';
export const OWNER = 'current-installed-data-go-oracle';
export const BUDGET = Object.freeze({milliseconds:600000, calls:64, stdoutAndStderr:128<<20, commandOutput:16<<20, pages:32, pageRows:200});
const json = bytes => parseAgentJson(bytes, {maximumBytes:BUDGET.commandOutput});
const ordinary = p => { const s=fs.lstatSync(p); assert(s.isFile()&&!s.isSymbolicLink(),p); return s; };
const record = p => ({path:p,bytes:ordinary(p).size,sha256:hashFile(p,128<<20)});
const save = (p,value) => fs.writeFileSync(p,JSON.stringify(value,null,2)+'\n',{flag:'wx'});
// Retain unsafe integers and signed zero in joined rows; original Go response
// bytes remain separately authoritative for every numeric token, including f32.
export function rowJSON(value) {
  if(isExactNumber(value))return value.exactNumber;
  if(typeof value==='number'){assert(Number.isFinite(value));return Object.is(value,-0)?'-0':JSON.stringify(value);}
  if(value===null||typeof value!=='object')return JSON.stringify(value);
  if(Array.isArray(value))return '['+value.map(rowJSON).join(',')+']';
  return '{'+Object.entries(value).map(([key,item])=>JSON.stringify(key)+':'+rowJSON(item)).join(',')+'}';
}
export function currentRetailRow(bytes) {
  const lines=bytes.toString('utf8').trim().split(/\r?\n/),names=lines.shift().split('|').map(name=>name.split('!')[0]);
  const matches=lines.map(line=>Object.fromEntries(line.split('|').map((value,index)=>[names[index],value])))
    .filter(row=>row.Active==='1'&&row.Product==='wow'&&row.Branch==='cn');
  assert.equal(matches.length,1,'one active installed cn Retail identity');
  const row=matches[0];assert.match(row.Version,/^\d+\.\d+\.\d+\.\d+$/);assert.match(row['Build Key'],/^[a-f0-9]{32}$/);assert.match(row['CDN Key'],/^[a-f0-9]{32}$/);return row;
}
export const SQL = Object.freeze({
  sql:'SELECT ID, Directory, MapName_lang FROM Map WHERE ID=:id',
  sqlAllIDs:'SELECT ID FROM Map WHERE ID>=0 ORDER BY ID',
  sqlRange:'SELECT ID, Directory FROM Map WHERE ID BETWEEN 0 AND 20 ORDER BY ID DESC LIMIT 5 OFFSET 1',
  sqlFlags:'SELECT ID, "Flags[0]" FROM Map WHERE ID=0',
  sqlExpression:'SELECT ID+1 AS NextID FROM Map WHERE ID IN (0,1,2) ORDER BY NextID DESC',
  sqlDistinct:'SELECT DISTINCT InstanceType FROM Map ORDER BY InstanceType',
  sqlText:'SELECT ID, MapName_lang FROM Map WHERE ID=0 AND LENGTH(MapName_lang)>0',
});
export function probesFor(rows) {
  const row=rows.find(row=>Number.isSafeInteger(row.ID)&&typeof row.Directory==='string'&&row.Directory.length>0);assert(row,'Map named record probe');
  const alphabet=[...new Set(rows.map(row=>row.Directory??'').join('').toLowerCase())].filter(c=>/^[a-z]$/.test(c));
  const query=alphabet.find(c=>rows.filter(row=>String(row.Directory??'').toLowerCase().includes(c)).length>2);assert(query,'actual truncated substring probe');
  return {recordId:row.ID,searchQuery:row.Directory.toUpperCase(),truncatedSearchQuery:query};
}
export function profileCommands(next,probes) {
  assert(Number.isSafeInteger(next)&&next>=0&&next<=0xffffffff);
  return [
    ['record',['data','db2','--table','Map','--id',String(probes.recordId)]],
    ['page',['data','db2','--table','Map','--limit','3']],
    ['following',['data','db2','--table','Map','--limit','3','--after-id',String(next)]],
    ['terminal',['data','db2','--table','Map','--after-id','4294967295']],
    ['search',['data','db2','search','Map','--field','Directory','--query',probes.searchQuery,'--limit','1']],
    ['truncatedSearch',['data','db2','search','Map','--field','Directory','--query',probes.truncatedSearchQuery,'--limit','2']],
    ['foreignKey',['data','db2','foreign-key','Map','--field','ID','--value',String(probes.recordId),'--limit','1']],
    ['invalidForeignKey',['data','db2','foreign-key','Map','--field','Directory','--value','0','--limit','1'],2],
  ];
}

export function prepare(root, request, {execute=ownedSpawnSync}={}) {
  assert.equal(process.platform,'win32');assert.equal(process.arch,'x64');root=fs.realpathSync(root);
  assert.deepEqual(Object.keys(request).sort(),['goArtifact','installation','luaLSArchive']);
  const artifactPath=contained(root,path.resolve(request.goArtifact)),artifact=json(fs.readFileSync(artifactPath));
  assert.equal(artifact.schema,'lycheedev.fixed-go-performance-artifact.v1');assert.equal(artifact.commit,GO_COMMIT);
  const go=verifyGoPackage(artifact.package.root,path.resolve(request.luaLSArchive));
  assert.equal(go.binarySHA256,artifact.package.binarySHA256);assert.equal(go.packageSHA256,artifact.package.packageSHA256);
  const installation=fs.realpathSync(request.installation);
  const buildInfoPath=path.join(path.dirname(installation),'.build.info'),buildInfo=record(buildInfoPath),row=currentRetailRow(fs.readFileSync(buildInfoPath));
  const inputFiles=[import.meta.filename,'tools/rust-disk.mjs','tools/rust-build.mjs','tools/rust-owned-process.mjs','tools/rust-owned-process.ps1','tools/rust-agent-client.mjs','tests/rust-performance-paired/inputs.mjs','tests/rust-performance-paired/contract.mjs'].map(p=>record(path.resolve(root,p)));
  const job=beginScratch(root,OWNER,{command:process.argv,inputs:{artifact:record(artifactPath),go,installation,buildInfo,row,inputFiles},retention:'evidence',admissionOptions:{requiredBytes:256<<20,scratchReserveBytes:128<<20}});
  const env=ownedUserEnvironment(job.directory),home=path.join(job.directory,'home'),fixture=path.join(job.directory,'fixture');
  fs.mkdirSync(fixture);fs.mkdirSync(path.join(fixture,'reports'));fs.mkdirSync(path.join(fixture,'captures'));
  const buildInfoSnapshot=path.join(fixture,'build-info.txt');fs.copyFileSync(buildInfoPath,buildInfoSnapshot,fs.constants.COPYFILE_EXCL);
  const reportPath=path.join(job.evidence,'preparation.json'),report={schema:'lycheedev.local-data-go-preparation.v1',state:'running',owner:job.receipt,directory:job.directory,fixture,go,buildInfo,row,calls:[],startedAt:new Date().toISOString(),budget:BUDGET};
  const logs=new Set([reportPath,buildInfoSnapshot]),expected={},captures=new Map(),deadline=performance.now()+BUDGET.milliseconds;let state='failed',handlesReaped=true,outputBytes=0;
  const persist=()=>fs.writeFileSync(reportPath,JSON.stringify(report,null,2)+'\n');
  const file=name=>path.join(fixture,name);
  const descriptor=p=>({...record(p),path:path.relative(fixture,p).split(path.sep).join('/')});
  function blob(ref,destination) {
    assert.match(ref.sha256,/^[a-f0-9]{64}$/);const source=path.join(home,'blobs',ref.sha256.slice(0,2),ref.sha256.slice(2));
    assert.equal(ordinary(source).size,ref.bytes);assert.equal(hashFile(source,128<<20),ref.sha256);
    fs.copyFileSync(source,destination,fs.constants.COPYFILE_EXCL);logs.add(destination);return descriptor(destination);
  }
  function call(id,args,{workspace=true,status=0,pinned=false,offline=true}={}) {
    assert(report.calls.length<BUDGET.calls);const remaining=Math.floor(deadline-performance.now());assert(remaining>=1000,'shared preparation 600s deadline');
    const command=[...args,...(pinned?['--snapshot',report.pin.id,'--installation',installation,...(offline?['--offline']:[])]:[]),...(workspace?['--home',home]:[]),'--format','json'];
    const observation={id,args:command};report.calls.push(observation);persist();handlesReaped=false;
    const run=execute(go.binary,command,{cwd:job.directory,env,evidence:job.evidence,timeout:remaining});handlesReaped=run.handlesReaped===true;
    for(const p of run.logs??[])logs.add(p);
    outputBytes+=(run.stdout?.length??0)+(run.stderr?.length??0);assert(outputBytes<=BUDGET.stdoutAndStderr,'original output budget');
    Object.assign(observation,{exit:run.status,error:run.error?.message,handlesReaped,proof:run.proof,logs:run.logs});persist();
    assert(handlesReaped&&!run.error,'original owned process must retire');assert.equal(run.status,status,`${id}: ${run.stderr?.toString().slice(0,1200)}`);
    const stdout=run.stdout??Buffer.alloc(0);assert(stdout.length<=BUDGET.commandOutput);const result=json(stdout);
    assert.equal(result.schema,'lycheedev.result.v1');assert.equal(result.ok,status===0);
    if(pinned&&status===0)assert.equal(result.context.snapshot,report.pin.id);
    const output=file(`reports/${id}.json`);fs.writeFileSync(output,stdout,{flag:'wx'});logs.add(output);expected[id]=descriptor(output);observation.response=expected[id];
    for(const capture of result.captures??[])if(!captures.has(capture.id)) {
      assert.match(capture.id,/^CAP-[a-f0-9]+$/);const body=blob(capture.blob,file(`captures/${capture.id}.bin`));captures.set(capture.id,{metadata:capture,body});
    }
    return result;
  }
  persist();
  try {
    for(const [id,args]of [['version',['version']],['help',['--help']],['describe',['describe']]]) {
      assert(!fs.existsSync(home));const result=call(id,args,{workspace:false});assert(!fs.existsSync(home));
      if(id==='version')assert.equal(result.result.version,'3.2.4');
    }
    call('init',['init']);
    const resolved=call('target',['target','resolve','--installation',installation,'--product','retail','--definitions','refs/heads/master','--region','cn','--locale','zhCN']);
    report.pin=resolved.result;const pin=report.pin;assert.match(pin.id,/^PIN-/);assert.equal(pin.data.fullBuild,row.Version);assert.equal(pin.data.buildConfig,row['Build Key']);assert.equal(pin.data.cdnConfig,row['CDN Key']);assert.equal(pin.data.product,'retail');assert.equal(pin.data.region,'cn');assert.equal(pin.data.language,'zhCN');assert.match(pin.data.definitionCommit,/^[a-f0-9]{40}$/);persist();
    const schema=call('schema',['data','db2','schema','Map'],{pinned:true,offline:false});
    assert.equal(schema.result.complete,true);assert.equal(schema.result.truncated,false);
    const ids=[],rows=[],pages=[];let after=null,first;
    for(let page=0;;page++) {
      assert(page<BUDGET.pages,'complete Map bounded page budget');const id=`allRows-${page}`;
      const current=call(id,['data','db2','--table','Map','--limit',String(BUDGET.pageRows),...(after===null?[]:['--after-id',String(after)])],{pinned:true});
      first??=current;const value=current.result;assert.equal(value.file.contentVerified,true);assert.equal(value.sources.commit,pin.data.definitionCommit);
      const p=value.page;assert.equal(p.ids.length,p.rows.length);for(let i=0;i<p.ids.length;i++){const n=p.ids[i];assert(Number.isSafeInteger(n)&&(!ids.length||n>ids.at(-1)));assert.equal(p.rows[i].ID,n);ids.push(n);rows.push(p.rows[i]);}pages.push(expected[id]);
      if(!p.more){assert.equal(p.next,null);break;}assert(p.ids.length>0&&p.next===p.ids.at(-1));after=p.next;
    }
    assert.equal(rows.length,schema.result.rowCount,'all logical Map rows, not a sample');
    const probes=probesFor(rows),rowPath=file('Map.rows.json');fs.writeFileSync(rowPath,rowJSON({ids,rows})+'\n',{flag:'wx'});logs.add(rowPath);
    const definitions={commit:pin.data.definitionCommit,manifest:blob(first.result.sources.manifest,file('manifest.json')),map:blob(first.result.sources.definition,file('Map.dbd'))};
    const page=call('page',['data','db2','--table','Map','--limit','3'],{pinned:true});
    for(const [id,args,status]of profileCommands(page.result.page.next,probes).filter(([id])=>id!=='page'))call(id,args,{pinned:true,status:status??0});
    const installedSqlQueries=[];
    for(const [id,sql]of Object.entries(SQL)) {
      const query=call(id,['data','sql','--sql',sql,...(id==='sql'?['--param',`id=${probes.recordId}`]:[])],{pinned:true});assert.equal(query.result.complete,true);
      if(id!=='sql')installedSqlQueries.push({sql,response:expected[id]});
    }
    const point=`SELECT ID, Directory, MapName_lang FROM Map WHERE ID=${probes.recordId}`;
    const query=call('sqlPoint',['data','sql','--sql',point],{pinned:true});assert.equal(query.result.complete,true);installedSqlQueries.push({sql:point,response:expected.sqlPoint});
    expected.installedSqlQueries=installedSqlQueries;
    const fileId=String(first.result.sources.identity.db2FileDataId);
    const asset=call('asset',['asset','inspect','--file-id',fileId],{pinned:true});assert.equal(asset.result.contentVerified,true);
    const rawPath=file('Map.decoded.db2');const exported=call('assetExport',['asset','export','--file-id',fileId,'--encoding','raw','--output',rawPath],{pinned:true});logs.add(rawPath);
    const raw=descriptor(rawPath);assert.equal(raw.sha256,asset.result.content.sha256);assert.equal(raw.bytes,asset.result.content.bytes);assert.equal(exported.result.content.sha256,raw.sha256);assert.equal(raw.sha256,first.result.file.content.sha256);
    assert.deepEqual(record(buildInfoPath),buildInfo,'installation changed during this fixture');
    for(const input of inputFiles)assert.deepEqual(record(input.path),input);assert.equal(hashFile(go.binary,128<<20),go.binarySHA256);
    const manifest={schema:SCHEMA,oracleCommit:GO_COMMIT,installation,pin,buildInfo:{...descriptor(buildInfoSnapshot),sourcePath:buildInfoPath,row},definitions,map:{raw,rows:{...descriptor(rowPath),count:rows.length},schema:expected.schema,pages},probes,expected,captures:[...captures.values()],oracle:{commit:GO_COMMIT,artifact:record(artifactPath),binarySHA256:go.binarySHA256,packageSHA256:go.packageSHA256},preparation:{owner:job.receipt,report:reportPath}};
    const manifestPath=file('fixture.json');save(manifestPath,manifest);logs.add(manifestPath);report.fixtureManifest=record(manifestPath);report.state='prepared-current-installed-go-expectations';state='succeeded';
  } catch(error) {report.state='failed-stopped';report.error=error.stack;process.exitCode=1;}
  finally {report.handlesReaped=handlesReaped;report.outputBytes=outputBytes;report.endedAt=new Date().toISOString();persist();job.seal({state:handlesReaped?state:'interrupted',handlesReaped,logs:[...logs]});}
  return {state:report.state,report:reportPath,owner:job.receipt,fixture:report.fixtureManifest};
}
if(process.argv[1]&&path.resolve(process.argv[1])===path.resolve(import.meta.filename)) {
  assert.equal(process.argv.length,4,'usage: node tests/rust-data-local-current/prepare.mjs --execute-main request.json');assert.equal(process.argv[2],'--execute-main');
  console.log(JSON.stringify(prepare(path.resolve('.'),json(fs.readFileSync(process.argv[3]))),null,2));
}
