// New Agent observations have their own identity. Historical SQL reports never
// satisfy this contract. The fixed request fixture uses only safe integers and
// ASCII keys; this is not a generic serde JSON canonicalizer.
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {join} from 'node:path';
import {bounded, digest, parse, loadArchivedFixture, SAMPLE_KEY_ID, pathStyle, npmOfflineFlags, MISSING_KEYS, RESTORED_KEYS} from './agent-data-inputs.mjs';
import {assertEnvelope, assertCapture, archiveBytes, readArchived} from './data-assertions.mjs';
import {CURRENT_GO_CONTRACT, inventoryHash, pendingIncrementTasks} from './assertions.mjs';
import {assertDataSkillInventory, assertDataSkillBytes, dataSkillIdentity, loadDataSkillContract} from './data-skill-contract.mjs';
import {assertDiscoveryNoHome, assertHelp, assertCommandSchema, assertSearch} from './agent-discovery.mjs';
import {assertSafeContext, decodeAgentFrame, stringifyExactIntegerJson, isExactNumber, exactNumber} from '../../tools/rust-agent-client.mjs';
export {assertEnvelope, archiveBytes};
export const PHASES = ['catalog', 'partial', 'restored'];
export const COLUMNS = ['ID', 'Name_lang', 'ModelFileDataID'];
export const ROWS = [[10, '无模型🙂', 0], [20, '第二', 501], [30, '第三', 501]];
export function queryFor(phase) {
 const names = phase === 'catalog' ? COLUMNS : ['ID', 'Value'];
 const table = {name: phase === 'catalog' ? 'HouseDecor' : 'Sample', mode: 'static'};
 return {kind:'plan', plan:{op:'sort', input:{op:'project', input:{op:'scan', table}, columns:names.map(name=>({name,expression:{kind:'column',name}}))},keys:[{expression:{kind:'column',name:'ID'},descending:false,nulls_first:false}]}};
}
export function expectedRows(phase) { return phase === 'catalog' ? ROWS : phase === 'partial' ? [[1,42]] : [[1,42],[2,99]]; }
export function expectedColumns(phase) { return phase === 'catalog' ? COLUMNS : ['ID','Value']; }
export function assertPage(page, phase) {
 assert.deepEqual(page.columns,expectedColumns(phase)); assert.deepEqual(page.rows,expectedRows(phase));
 assert.equal(page.offset,0); assert.equal(page.totalRows,expectedRows(phase).length); assert.equal(page.complete,phase!=='partial');
}
export function assertCSV(result,phase) {
 assert.equal(result.format,'csv'); assertPage(result.page,phase);
 const quoted=value=>'\"'+String(value??'').replaceAll('\"','\"\"')+'\"';
 const text=[expectedColumns(phase),...expectedRows(phase)].map(row=>row.map(quoted).join(',')).join('\r\n')+'\r\n';
 assert.equal(result.text,text);
}
export function assertCaptureBody(body,phase,query,provenance) {
 assert.equal(body.schema,'lycheedev.data-agent-result.v1');assert.deepEqual(body.columns,expectedColumns(phase));assert.deepEqual(body.rows,expectedRows(phase));
 assert.equal(body.rowCount,expectedRows(phase).length);assert.equal(body.complete,phase!=='partial');assert.deepEqual(body.provenance,provenance);assert.deepEqual(body.provenance.request,query);
}
function safeRequest(value) {
 assertSafeContext(value,'fixed request');
 if (typeof value==='number') assert(Number.isSafeInteger(value)&&!Object.is(value,-0));
 if (typeof value==='string') assert(/^[\x20-\x7e]*$/.test(value));
 if(value&&typeof value==='object') for(const [key,item] of Object.entries(value)){assert(/^[\x20-\x7e]*$/.test(key));safeRequest(item);}
}
export function assertAgentCapture(capture,raw,metadata,snapshot,query) {
 assert.equal(metadata.database,'state/data/evidence-v2.sqlite');assertCapture(capture,raw,metadata);
 const body=parse(raw);assert.equal(capture.complete,body.complete);safeRequest(query);safeRequest(body.provenance.request);assert.deepEqual(body.provenance.request,query);
 assert.equal(capture.mediaType,'application/json');assert.equal(capture.provenance.kind,'data-agent-query');assert.equal(capture.provenance.snapshot,snapshot);
 // Preserve the authenticated CAP object's actual serialized field order.
 assert.equal(capture.provenance.locator,'typed:sha256:'+digest(Buffer.from(JSON.stringify(body.provenance.request))));
 assert.deepEqual(parse(Buffer.from(metadata.valueBase64,'base64')),capture);
}
export function assertAnalyzePage(page) {
 assert.deepEqual(page.columns,['plan_type','plan']);assert.equal(page.offset,0);assert.equal(page.complete,true);
 assert.equal(page.totalRows,4);assert.equal(page.rows.length,4);assert(page.rows.every(row=>Array.isArray(row)&&row.length===2&&row.every(cell=>typeof cell==='string')));const rows=new Map(page.rows);assert.equal(rows.size,page.rows.length);assert.deepEqual([...rows.keys()],['Plan with Metrics','Plan with Full Metrics','Output Rows','Duration']);
 for(const label of ['Plan with Metrics','Plan with Full Metrics','Output Rows','Duration'])assert(rows.has(label));
 assert.equal(rows.get('Output Rows'),'3');assert.equal(typeof rows.get('Duration'),'string');assert(rows.get('Duration').length>0);
 assert(rows.get('Plan with Metrics').length>0);assert(rows.get('Plan with Full Metrics').length>0);
}
export function assertAgentResources(provenance){
 const r=provenance.resources;assert(r);
 assert.equal(r.scope,'result_return_before_transport_and_later_handle_operations');
 assert.deepEqual({...r.limits,requestRetainedBytes:0},{sessionRetainedBytes:536870912,requestRetainedBytes:0,queryWork:100000000,operatorPoolBytes:134217728,resultBytes:134217728,pageBytes:8388608});
 assert(Number.isSafeInteger(r.limits.requestRetainedBytes)&&r.limits.requestRetainedBytes>0&&r.limits.requestRetainedBytes<=r.limits.sessionRetainedBytes);
 for(const ledger of [r.requestSafetyLedger,r.requestLogicalLedger]){
  assert.deepEqual(Object.keys(ledger).sort(),['decodeWork','metadataBytes','networkBytes','networkRequests','retainedBytes']);
  for(const n of Object.values(ledger))assert(Number.isSafeInteger(n)&&n>=0);
  assert(ledger.decodeWork<=r.limits.queryWork);assert(ledger.retainedBytes<=r.limits.requestRetainedBytes);assert.equal(ledger.networkBytes,0);assert.equal(ledger.networkRequests,0);
 }
 assert(r.requestSafetyLedger.decodeWork>0);
 assert(Number.isSafeInteger(r.sessionRetainedBytes)&&r.sessionRetainedBytes>0&&r.sessionRetainedBytes<=r.limits.sessionRetainedBytes);
 assert.equal(r.retainedSemantics,'outstanding_admitted_bytes_not_rss_or_os_peak');
 assert.equal(r.workSemantics,'admitted_work_not_cpu_or_operator_cost');
 assert.equal(r.resourceBoundaryScope,'in_process_logical_and_operator_pool');
 assert.equal(r.requiredExternalProtection,'formal_session_worker_supervisor');
 assert.equal(r.externalSupervisorVerifiedByEngine,false);assert.equal(r.wholeNativeAllocatorAccountingComplete,false);
}
export function assertAgentAnalyze(provenance,query){
 assert.deepEqual(provenance.request,{kind:'explain',query,verbose:true,analyze:true});
 assertAgentResources(provenance);const a=provenance.analysis;assert(a);
 assert.equal(a.analysisScope,'complete_native_root');
 assert.equal(a.requestTimingScope,'execute_entry_to_result_return');
 assert.deepEqual(a.excluded,['transport_serialization','subsequent_handle_operations']);
 assert.equal(a.workLedgerSemantics,'admitted_work_not_cpu_or_operator_cost');assert.equal(a.wholeNativeWorkAccountingComplete,false);
 assert.equal(a.nativePreflightAndPrerequisites.executed,true);assert.equal(a.nativePreflightAndPrerequisites.hasPrerequisites,false);assert.equal(a.prerequisiteNativeMetricsIncluded,true);
 assert.equal(a.sourcesScope,'authenticated_request_sources_listed_in_provenance');
 assert.deepEqual(a.requestWorkLedger,provenance.resources.requestSafetyLedger);
 const parts=[a.nativePreflightAndPrerequisites,a.rootExecutionAndOutputCompaction];
 assert(Number.isFinite(a.requestElapsedMs)&&a.requestElapsedMs>=0);
 for(const p of parts){assert(Number.isFinite(p.elapsedMs)&&p.elapsedMs>=0&&p.elapsedMs<=a.requestElapsedMs);assert(Number.isSafeInteger(p.decodeWork)&&p.decodeWork>=0&&p.decodeWork<=a.requestWorkLedger.decodeWork);}
 assert(parts.reduce((n,p)=>n+p.elapsedMs,0)<=a.requestElapsedMs);
}

const ordered=(value,keys)=>{assert.deepEqual(Object.keys(value).sort(),[...keys].sort());return Object.fromEntries(keys.map(key=>[key,value[key]]));};
function canonicalSourceIdentity(value) {
 const x=ordered(value,['schema','sourceKind','data','table','definition','definitionManifest','definitionObject','layout','source','missing','unavailablePartitions','complete']);
 x.data=ordered(x.data,['product','region','fullBuild','buildConfig','cdnConfig','language','definitionCommit']);
 x.table=ordered(x.table,['name','hash','db2FileDataId','dbcFileDataId']);
 x.definition=ordered(x.definition,['sha256','match','build','layout','fields']);
 x.definition.fields=x.definition.fields.map(field=>ordered(field,['name','kind','foreignTable','foreignColumn','verified','bits','signed','elements','array','identity','inline','relation']));
 x.layout=ordered(x.layout,['version','schemaRevision','schemaBuild','rows','columns','stride','tableHash','layoutHash','firstId','lastId','locale','flags','identityColumn','packedOffset','lookupColumns','fields','placements','partitions','paletteOffset','paletteBytes','commonOffset','commonBytes','metadataEnd']);
 x.layout.fields=x.layout.fields.map(field=>ordered(field,['bitOffset','bitWidth','extraBytes','codec','parameters']));
 x.layout.placements=x.layout.placements.map(field=>ordered(field,['widthDelta','byteOffset']));
 x.layout.partitions=x.layout.partitions.map(part=>ordered(part,['keyId','offset','rows','stringBytes','identityBytes','relationBytes','copies','sparseOffset','sparseCount','encryptedIds','idsComplete']));
 x.source=ordered(x.source,['origin','contentKey','payloadEncodingKey','content','partialContent','keySource']);
 x.missing=x.missing.map(row=>ordered(row,['offset','bytes','keyID','chunk']));return x;
}
// Independent oracle only for the authenticated fixed WDC3 fixtures. It does
// not call the product decoder or accept a layout supplied by the report.
export function fixtureWdc3Layout(raw) {
 assert(Buffer.isBuffer(raw)&&raw.length>=72&&raw.length<=64<<20);
 assert.equal(raw.subarray(0,4).toString('ascii'),'WDC3');
 const u32=at=>raw.readUInt32LE(at),u16=at=>raw.readUInt16LE(at);
 const totalColumns=u32(44),storageBytes=u32(56),commonBytes=u32(60),paletteBytes=u32(64),count=u32(68);
 assert(totalColumns>0&&totalColumns<=8&&count>0&&count<=2);
 assert.equal(storageBytes,totalColumns*24);assert.equal(commonBytes,0);assert.equal(paletteBytes,0);
 const storageStart=72+count*40+totalColumns*4,metadataEnd=storageStart+storageBytes;
 assert(metadataEnd<=raw.length);
 const partitions=Array.from({length:count},(_,i)=>{const p=72+i*40,key=raw.readBigUInt64LE(p);return{
  keyId:key<=BigInt(Number.MAX_SAFE_INTEGER)?Number(key):exactNumber(key.toString()),
  offset:u32(p+8),rows:u32(p+12),stringBytes:u32(p+16),identityBytes:u32(p+24),
  relationBytes:u32(p+28),copies:u32(p+36),sparseOffset:u32(p+20),sparseCount:u32(p+32),
  encryptedIds:[],idsComplete:false};});
 const placements=Array.from({length:totalColumns},(_,i)=>{const p=72+count*40+i*4;return{widthDelta:raw.readInt16LE(p),byteOffset:u16(p+2)};});
 const fields=Array.from({length:totalColumns},(_,i)=>{const p=storageStart+i*24;const field={bitOffset:u16(p),bitWidth:u16(p+2),extraBytes:u32(p+4),codec:u32(p+8),parameters:[u32(p+12),u32(p+16),u32(p+20)]};assert.equal(field.codec,0);assert.equal(field.extraBytes,0);return field;});
 const result={version:3,schemaRevision:0,schemaBuild:'',rows:u32(4),columns:u32(8),stride:u32(12),tableHash:u32(20),layoutHash:u32(24),firstId:u32(28),lastId:u32(32),locale:u32(36),flags:u16(40),identityColumn:u16(42),packedOffset:u32(48),lookupColumns:u32(52),fields,placements,partitions,paletteOffset:metadataEnd,paletteBytes,commonOffset:metadataEnd,commonBytes,metadataEnd};
 assert.equal(result.flags&1,0);assert.equal(result.columns,totalColumns);
 assert.equal(partitions.reduce((n,p)=>n+p.rows,0),result.rows);
 let end=metadataEnd;for(const p of partitions){assert(p.offset>=end);end=p.offset+p.rows*result.stride+p.stringBytes+p.identityBytes+p.relationBytes+p.copies*8+p.sparseCount*10;assert(end<=raw.length);}
 return result;
}
export function assertSourceIdentity(source,phase,fixture) {
 const partial=phase==='partial', name=phase==='catalog'?'HouseDecor':'Sample', table=fixture.identity.tables.find(row=>row.name===name);assert(table);
 const identity=source.sourceIdentity, file=source.sourceReading,raw=fixture.rawObjects[table.sha256];assert(raw);
 assert.equal(source.sourceSha256,digest(Buffer.from(stringifyExactIntegerJson(canonicalSourceIdentity(identity)))));
 assert.deepEqual(identity.layout,fixtureWdc3Layout(raw),'full layout must match authenticated raw WDC3 bytes');
 const base=`https://raw.githubusercontent.com/wowdev/WoWDBDefs/${fixture.identity.pin.definitionCommit}/`;
 const manifest=fixture.documents.map(row=>parse(row.bytes)).find(row=>row.url===base+'manifest.json'),definition=fixture.documents.map(row=>parse(row.bytes)).find(row=>row.url===base+`definitions/${name}.dbd`);assert(manifest&&definition);
 assert.deepEqual(identity.definitionManifest,manifest.blob);assert.deepEqual(identity.definitionObject,definition.blob);assert.equal(identity.definition.sha256,table.dbdSHA256);
 assert.equal(identity.definition.layout,raw.readUInt32LE(24).toString(16).toUpperCase().padStart(8,'0'));assert.equal(table.layout,raw.readUInt32LE(24));assert.equal(identity.table.hash,raw.readUInt32LE(20));
 for(const [key,offset]of [['rows',4],['columns',8],['stride',12],['tableHash',20],['layoutHash',24],['firstId',28],['lastId',32],['locale',36],['packedOffset',48],['lookupColumns',52]])assert.equal(identity.layout[key],raw.readUInt32LE(offset));
 for(const [key,offset]of [['flags',40],['identityColumn',42]])assert.equal(identity.layout[key],raw.readUInt16LE(offset));
 assert.equal(identity.source.origin,'installation');assert.equal(file.source,'installation');
 if(phase!=='catalog'){const id=identity.layout.partitions[1].keyId;assert(isExactNumber(id));assert.equal(id.exactNumber,raw.readBigUInt64LE(112).toString());}
 assert.equal(source.sourceMode,'static');assert.equal(source.complete,!partial);assert.equal(source.truncated,false);
 assert.equal(identity.schema,'lycheedev.data-static-generation-identity.v1');assert.deepEqual(identity.data,fixture.identity.pin);
 assert.equal(identity.sourceKind,'static');assert.equal(identity.table.dbcFileDataId,0);assert.equal(identity.layout.version,3);assert.equal(identity.layout.schemaRevision,0);assert.equal(identity.layout.schemaBuild,'');assert.equal(identity.definition.build,fixture.identity.pin.fullBuild);assert.equal(identity.definition.match,'layout');
 const fieldNames=phase==='catalog'?['ID','Name_lang','ModelFileDataID','ThumbnailFileDataID','ItemID','GameObjectID','Type','ModelType']:['ID','Value'];assert.deepEqual(identity.definition.fields,fieldNames.map(name=>({name,kind:name==='Name_lang'?'locstring':'int',foreignTable:'',foreignColumn:'',verified:true,bits:name==='Name_lang'?0:32,signed:['Name_lang','Type','ModelType'].includes(name),elements:1,array:false,identity:name==='ID',inline:true,relation:false})));
 const keySource=phase==='catalog'?{kind:'public',state:'not_needed',commit:'71b75360752840dd62a412013a38b5a972524a50',sha256:'6d83241ed776f9e74e27ed65d1ec52f1a1464a4e9b16bed497258fdd61020b52'}:{kind:'file',state:'loaded',sha256:digest(phase==='partial'?MISSING_KEYS:RESTORED_KEYS)};assert.deepEqual(file.keySource,keySource);assert.deepEqual(identity.source.keySource,keySource);
 assert.equal(identity.table.name,name);assert.equal(identity.table.db2FileDataId,table.fileDataID);
 assert.deepEqual(source.sourceDefinition,identity.definition);assert.deepEqual(source.sourceLayout,identity.layout);
 assert.equal(identity.source.contentKey,fixture.objects[table.sha256].ckey);assert.equal(identity.source.payloadEncodingKey,fixture.objects[table.sha256].ekey);
 assert.equal(file.entry.fileDataID,table.fileDataID);assert.equal(file.entry.contentKey,fixture.objects[table.sha256].ckey);assert.equal(file.payloadEncodingKey,fixture.objects[table.sha256].ekey);
 assert.deepEqual(file.pin,fixture.identity.pin);assert.equal(file.catalogSHA256,digest(fixture.catalog));assert.deepEqual(file.root,fixture.rootRef);
 assert.deepEqual(file.buildConfiguration,{sha256:fixture.configs[0].sha256,bytes:fixture.configs[0].bytes});assert.deepEqual(file.cdnConfiguration,{sha256:fixture.configs[1].sha256,bytes:fixture.configs[1].bytes});
 assert.deepEqual(file.verification,{encodingFramingEKey:true,encodingRequestedPagesMD5:true,encodingWholeDecodedCKey:false,rootWholeDecodedCKey:true,contentWholeDecodedCKey:!partial});assert.deepEqual(source.sourceVerification,file.verification);
 assert.equal(file.contentVerified,!partial);assert.equal(identity.complete,!partial);
 if(partial){assert.deepEqual(file.partialContent,fixture.partial.partialContent);assert.equal(file.missing.length,1);assert.equal(file.missing[0].keyID,SAMPLE_KEY_ID);assert.equal(file.missing[0].bytes,8);assert.equal(file.missing[0].offset,fixture.partial.rawBytes-8);assert.equal(file.missing[0].chunk,1);assert.deepEqual(identity.missing,[file.missing[0]]);assert.deepEqual(identity.unavailablePartitions,[1]);assert.deepEqual(identity.source.partialContent,fixture.partial.partialContent);assert.deepEqual(identity.source.content,{sha256:'',bytes:0});}
 else{assert.deepEqual(file.content,{sha256:table.sha256,bytes:table.bytes});assert.deepEqual(identity.missing,[]);assert.deepEqual(identity.unavailablePartitions,[]);assert.deepEqual(identity.source.content,{sha256:table.sha256,bytes:table.bytes});assert.equal(identity.source.partialContent,null);assert(!Object.hasOwn(file,'partialContent'));assert(!Object.hasOwn(file,'missing'));}
}
export function pendingDataTasks(workflow){return pendingIncrementTasks(workflow).filter(row=>row.taskId!=='g323-data-query');}
export function assertAgentDataReport(report,workflow,{evidenceDirectory=report?.config?.evidence}={}) {
 assert.equal(report.schema,'lycheedev.skill-agent-data-usage.v1');assert.equal(report.state,'passed_selected_agent_data_workflow');assert.equal(report.completeSkillAcceptance,false);
 assert.deepEqual(report.checks.map(row=>row.caseId),['export-catalog','export-partial','npm-installed-package']);assert(report.checks.every(row=>row.state==='passed'));assert.equal(report.goContract,CURRENT_GO_CONTRACT);assert.deepEqual(report.pendingTasks,pendingDataTasks(workflow));
 const config=report.config,p=pathStyle(config.scratch);assert.equal(config.platform,'win32');assert.equal(config.arch,'x64');assert.deepEqual(config.skillIdentity,dataSkillIdentity(loadDataSkillContract(config.skillIdentity)));assert.equal(report.skillTree,config.skillIdentity.candidateTree);
 assert.equal(config.launcher,p.join(config.prefix,'node_modules/lycheedev/payload/skill/scripts/lycheedev.mjs'));assert.equal(config.prefix,p.join(config.scratch,'npm-prefix'));
 const fixtures=Object.fromEntries(['catalog','partial'].map(kind=>[kind,loadArchivedFixture(evidenceDirectory,report.fixtures[kind],kind)]));
 const envelopes={},seen=new Set();let bytes=0;
 for(const command of report.commands){assert(!seen.has(command.id));seen.add(command.id);assert.equal(command.exit,0);assert.equal(command.signal,null);assert.equal(command.error,undefined);const raw=readArchived(evidenceDirectory,command.stdout);bytes+=raw.length+readArchived(evidenceDirectory,command.stderr).length;if(command.kind==='public'){const envelope=parse(raw);assertEnvelope(envelope);envelopes[command.id]=envelope;}}
 assert(bytes<=128<<20);assert(report.commands.length<=80);
 const expected=[];const add=(id,executable,args,cwd=config.scratch,kind='public',environment={})=>expected.push({id,executable,args,cwd,kind,environment});
 for(const [i,kind] of ['catalog','partial'].entries())add(`export-${kind}`,config.exporters.executables[i].path,[kind==='catalog'?'prepare_catalog_public_fixture':'prepare_skill_partial_fixture','--exact','--ignored','--nocapture'],config.root,'exporter',kind==='catalog'?{LYCHEEDEV_CATALOG_FIXTURE:p.join(config.scratch,`${kind}-fixture`)}:{LYCHEEDEV_SKILL_PARTIAL_FIXTURE:p.join(config.scratch,`${kind}-fixture`),LYCHEEDEV_SKILL_OWNER_RECEIPT:p.join(config.evidence,'receipt.json')});
 add('npm-pack',config.node,[config.npm.path,'pack',config.sourcePackage,'--json','--pack-destination',config.scratch,...npmOfflineFlags(config.scratch)],config.scratch,'npm');
 add('npm-install',config.node,[config.npm.path,'install','--global','--prefix',config.prefix,config.tgz,...npmOfflineFlags(config.scratch)],config.scratch,'npm');
 const publicCall=(id,args,kind)=>add(id,config.node,[config.launcher,...args,...(['cli','schema'].includes(args[0])?[]:['--home',p.join(config.scratch,`${kind}-home`)]),'--format','json'],config.scratch,'public',{LYCHEEDEV_HOME:p.join(config.scratch,`${kind}-home`)});
 const discovery=[['version',['version']],['describe',['describe']],['help',['--help']],['agent-schema',['data','agent','schema']],['agent-help',['data','agent','serve','--help']],['agent-command-schema',['schema','data','agent','serve']],['agent-search',['cli','search','data agent serve']],['empty-search',['cli','search','zz-no-command-7b95']]];
 for(const [id,args]of discovery)publicCall(id,args,'discovery');
 for(const kind of ['catalog','partial']){publicCall(`${kind}-init`,['init'],kind);add(`${kind}-import`,config.exporters.executables.find(row=>row.name==='agent_fixture_import').path,['import_owned_agent_fixture','--exact','--ignored','--nocapture'],config.root,'importer',{LYCHEEDEV_AGENT_IMPORT:p.join(config.scratch,`${kind}-import.json`)});}
 assertDiscoveryNoHome(report.discovery,discovery.map(([id])=>id),[p.join(config.scratch,'discovery-home'),p.join(config.scratch,'user','.lycheedev')]);
 assert.equal(envelopes.version.result.platform,'windows/amd64');assert.equal(envelopes.version.result.version,config.version);assert.equal(typeof envelopes.version.result.rust,'string');assert(envelopes.version.result.rust.length>0);const commands=envelopes.describe.result.commands;assert(Array.isArray(commands));for(const path of ['data agent serve','data agent schema','data cache prepare','source query','schema','cli search','evidence verify'])assert(commands.some(row=>row.path===path));assert(!commands.some(row=>row.path==='data sql'||row.path.startsWith('data db2')||/^data (item|spell|decor|creature|encounter) /.test(row.path)));assertHelp(envelopes.help.result);assertHelp(envelopes['agent-help'].result,'data agent serve');assertCommandSchema(envelopes['agent-command-schema'].result,'data agent serve');assertSearch(envelopes['agent-search'].result,'data agent serve','data agent serve');assertSearch(envelopes['empty-search'].result,'zz-no-command-7b95');assert.equal(envelopes['agent-schema'].result.protocol,'lycheedev.data-agent.v1');
 const pack=parse(readArchived(evidenceDirectory,report.commands.find(row=>row.id==='npm-pack').stdout));assert.equal(pack.length,1);assert.equal(pack[0].filename,p.basename(config.tgz));assert.equal(pack[0].name,'lycheedev');assert.equal(pack[0].version,config.version);
 const installed=parse(readArchived(evidenceDirectory,report.installedInventory));assert.deepEqual(installed,config.packageFiles);assert.equal(inventoryHash(installed),config.packageSHA256);const skill=assertDataSkillInventory(installed,config.skillIdentity);
 const {files,...identity}=report.skillProof;assert.deepEqual(identity,config.skillIdentity);assert.deepEqual(files.map(row=>row.path),skill.files.map(row=>row.path));for(const [i,row]of skill.files.entries())assertDataSkillBytes(readArchived(evidenceDirectory,files[i].archive),row);
 assert(readArchived(evidenceDirectory,report.npmShim).toString().includes('lycheedev.mjs'));
 for(const kind of ['catalog','partial']){assert.equal(fixtures[kind].directory,p.join(config.scratch,`${kind}-fixture`));const imported=parse(readArchived(evidenceDirectory,report.imports[kind]));assert.deepEqual(imported,{schema:'lycheedev.paired-agent-seed.recovered.v1',owner_receipt:p.join(config.evidence,'receipt.json'),home:p.join(config.scratch,`${kind}-home`),fixture_home:fixtures[kind].identity.home,snapshot:fixtures[kind].identity.snapshot,pin:fixtures[kind].identity.pin,documents:fixtures[kind].documents.filter(row=>row.key.startsWith('definition/')).map(row=>{const value=parse(row.bytes);return{key:row.key,value,blob_path:p.join(fixtures[kind].identity.home,'blobs',value.blob.sha256.slice(0,2),value.blob.sha256.slice(2))};})});assert.deepEqual(report.commands.find(row=>row.id===`${kind}-import`).environment,{LYCHEEDEV_AGENT_IMPORT:p.join(config.scratch,`${kind}-import.json`)});const seeded=parse(readArchived(evidenceDirectory,report.seeded[kind]));assert.deepEqual(seeded,fixtures[kind].documents.map(row=>({key:row.key,generation:1,valueBase64:row.bytes.toString('base64')})).sort((a,b)=>a.key.localeCompare(b.key)));assert.deepEqual(parse(readArchived(evidenceDirectory,report.workspaces[kind])),envelopes[`${kind}-init`].result.identity);}
 assert.deepEqual(report.sessions.map(row=>row.phase),PHASES);const captures=[];
 for(const session of report.sessions){const phase=session.phase,kind=phase==='catalog'?'catalog':'partial',fixture=fixtures[kind],query=queryFor(phase);assert.equal(session.executable,config.node);assert.match(config.environmentSHA256,/^[a-f0-9]{64}$/);assert.equal(session.environmentSHA256,config.environmentSHA256);assert.equal(session.cwd,config.scratch);const args=[config.launcher,'data','agent','serve','--home',p.join(config.scratch,`${kind}-home`),'--snapshot',fixture.identity.snapshot,'--installation',fixture.identity.installation,'--format','jsonl'];if(kind==='partial')args.push('--key-file',p.join(config.scratch,'sample.keys'));assert.deepEqual(session.args,args);assert.deepEqual(session.process,{code:0,signal:null});
 const frames=session.frames.map((row,i)=>{assert.equal(row.request.protocol,'lycheedev.data-agent.v1');assert.equal(row.request.id,i+1);assert(Number.isSafeInteger(row.request.timeout_ms)&&row.request.timeout_ms>0&&row.request.timeout_ms<=30000);const frame=decodeAgentFrame(readArchived(evidenceDirectory,row.frame),i+1);assert.equal(frame.ok,true);return{request:row.request,value:frame.result};});
 assert.deepEqual(frames.map(row=>row.request.task),phase==='catalog'?['discover','query','page','export','capture','release','explain','page','release','close']:['discover','query','page','export','capture','release','close']);
 const req=row=>{const {protocol,id,timeout_ms,...task}=row.request;return task;};assert.deepEqual(req(frames[0]),{task:'discover',table:{name:kind==='catalog'?'HouseDecor':'Sample',mode:'static'}});assert.equal(frames[0].value.rowCount,expectedRows(phase).length);assertSourceIdentity(frames[0].value.provenance,phase,fixture);
 assert.deepEqual(req(frames[1]),{task:'query',query});const result=frames[1].value,handle=result.result.handle;assert.equal(result.complete,phase!=='partial');assert.equal(result.provenance.engine,'datafusion');assert.equal(result.provenance.complete,phase!=='partial');assert.equal(result.result.rows,expectedRows(phase).length);assertAgentResources(result.provenance);assert.equal(result.provenance.sources.length,1);assertSourceIdentity(result.provenance.sources[0],phase,fixture);
 for(const index of [2,3,4,5])assert.equal(frames[index].request.handle,handle);assert.deepEqual(req(frames[2]),{task:'page',handle,offset:0,limit:100});assertPage(frames[2].value,phase);assert.deepEqual(req(frames[3]),{task:'export',handle,offset:0,limit:100,format:'csv'});assertCSV(frames[3].value,phase);assert.deepEqual(req(frames[4]),{task:'capture',handle});assert.deepEqual(req(frames[5]),{task:'release',handle});assert.deepEqual(frames[5].value,{released:handle});
 const capture=frames[4].value,raw=readArchived(evidenceDirectory,session.captureBody);assertCaptureBody(parse(raw),phase,query,result.provenance);assertAgentCapture(capture,raw,parse(readArchived(evidenceDirectory,session.captureMetadata)),fixture.identity.snapshot,query);captures.push(capture.id);publicCall(`agent-${phase}-verify`,['evidence','verify',capture.id],kind);assert.deepEqual(envelopes[`agent-${phase}-verify`].result,capture);
 if(phase==='catalog'){assert.deepEqual(req(frames[6]),{task:'explain',query,verbose:true,analyze:true});assertAgentAnalyze(frames[6].value.provenance,query);assert.deepEqual(req(frames[7]),{task:'page',handle:frames[6].value.result.handle,offset:0,limit:100});assertAnalyzePage(frames[7].value);assert.deepEqual(req(frames[8]),{task:'release',handle:frames[6].value.result.handle});assert.deepEqual(frames[8].value,{released:frames[6].value.result.handle});}
 assert.deepEqual(req(frames.at(-1)),{task:'close'});assert.deepEqual(frames.at(-1).value,{closed:true,handlesReleased:true});
 }
 assert.equal(new Set(captures).size,3);assert.deepEqual(report.commands.map(({id,executable,args,cwd,kind,environment})=>({id,executable,args,cwd,kind,environment})),expected);
 assert.equal(readArchived(evidenceDirectory,report.keys.missing).toString(),MISSING_KEYS);assert.equal(readArchived(evidenceDirectory,report.keys.restored).toString(),RESTORED_KEYS);
 assert.deepEqual(report.completedTasks,[{taskId:'g323-data-query',runtimeVerified:true,basis:'actual_owned_npm_installed_agent_and_independent_full_capture_replay',captures:3}]);return true;
}
