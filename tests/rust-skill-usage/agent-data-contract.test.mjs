// Declarations only until the unified test gate. These checker negatives never
// represent a product observation or replace the owned installed workflow.
import test from 'node:test';
import assert from 'node:assert/strict';
import {queryFor,assertPage,assertCSV,assertAnalyzePage,assertAgentDataReport} from './agent-data-assertions.mjs';
import {assertHelp,assertDiscoveryNoHome} from './agent-discovery.mjs';
test('typed task preserves exact catalog rows and rejects missing coverage',()=>{
 const page={columns:['ID','Name_lang','ModelFileDataID'],rows:[[10,'无模型🙂',0],[20,'第二',501],[30,'第三',501]],offset:0,totalRows:3,complete:true};
 assertPage(page,'catalog');assertCSV({format:'csv',page,text:'"ID","Name_lang","ModelFileDataID"\r\n"10","无模型🙂","0"\r\n"20","第二","501"\r\n"30","第三","501"\r\n'},'catalog');
 for(const changed of [{...page,complete:false},{...page,totalRows:2},{...page,rows:page.rows.slice(1)},{...page,offset:1}])assert.throws(()=>assertPage(changed,'catalog'));
 assert.equal(queryFor('catalog').plan.input.input.op,'scan');
});
test('Analyze requires actual native output rather than provenance echo',()=>{
 const page={columns:['plan_type','plan'],offset:0,totalRows:4,complete:true,rows:[['Plan with Metrics','exec rows=3'],['Plan with Full Metrics','exec rows=3 elapsed_compute=1'],['Output Rows','3'],['Duration','1ms']]};assertAnalyzePage(page);
 assert.throws(()=>assertAnalyzePage({...page,rows:page.rows.filter(([name])=>name!=='Output Rows')}));assert.throws(()=>assertAnalyzePage({...page,rows:page.rows.map(([k,v])=>[k,k==='Output Rows'?'2':v])}));
});
test('metadata discovery proves no new home on each call',()=>{
 const paths=['explicit','default'],rows=[{id:'help',paths,before:[false,false],after:[false,false]}];assertDiscoveryNoHome(rows,['help'],paths);
 assert.throws(()=>assertDiscoveryNoHome([{...rows[0],after:[false,true]}],['help'],paths));assert.throws(()=>assertHelp({schema:'lycheedev.cli-help.v1',kind:'group',command:'',children:Array(33).fill({}),limits:{children:32}}));
});
test('historical SQL report never qualifies as Agent workflow',()=>{assert.throws(()=>assertAgentDataReport({schema:'lycheedev.skill-data-usage.v2',state:'passed_selected_data_workflow'},{}));});

test('CAP completeness remains bound even after valid identity recomputation',async()=>{
 const {assertAgentCapture}=await import('./agent-data-assertions.mjs');
 const {captureCanonical}=await import('./data-assertions.mjs');
 const {digest,goJSON}=await import('./agent-data-inputs.mjs');
 const query=queryFor('partial'),body={schema:'lycheedev.data-agent-result.v1',columns:['ID','Value'],rows:[[1,42]],rowCount:1,complete:false,provenance:{request:query}};
 const raw=Buffer.from(JSON.stringify(body)),capture={schema:'lycheedev.capture.v1',id:'',blob:{sha256:digest(raw),bytes:raw.length},mediaType:'application/json',provenance:{kind:'data-agent-query',locator:'typed:sha256:'+digest(Buffer.from(JSON.stringify(query))),snapshot:'PIN-'+ 'a'.repeat(64)},complete:false,truncated:false,capturedAt:'2026-10-20T00:00:00Z'};
 const seal=()=>{capture.id='CAP-'+digest(goJSON(captureCanonical(capture)));return{database:'state/data/evidence-v2.sqlite',key:'capture/'+capture.id,generation:1,valueBase64:Buffer.from(JSON.stringify(capture)).toString('base64')};};
 let metadata=seal();assertAgentCapture(capture,raw,metadata,capture.provenance.snapshot,query);capture.complete=true;metadata=seal();assert.throws(()=>assertAgentCapture(capture,raw,metadata,capture.provenance.snapshot,query));
});

test('fixed WDC3 layout oracle binds every metadata region and wide partition ID',async()=>{
 const {fixtureWdc3Layout}=await import('./agent-data-assertions.mjs');
 const raw=Buffer.alloc(224);raw.write('WDC3');
 for(const [p,v]of [[4,2],[8,2],[12,8],[44,2],[56,48],[68,2],[80,208],[84,1],[120,216],[124,1]])raw.writeUInt32LE(v,p);
 raw.writeBigUInt64LE(0x1234567890abcdefn,112);raw.writeUInt16LE(4,158);
 const expected=fixtureWdc3Layout(raw);assert.equal(expected.metadataEnd,208);assert.equal(expected.partitions[1].keyId.exactNumber,'1311768467294899695');assert.equal(expected.placements[1].byteOffset,4);
 for(const mutate of [v=>v.partitions[0].offset++,v=>v.placements[1].byteOffset++,v=>v.fields[0].bitOffset++,v=>v.metadataEnd++]){const changed=structuredClone(expected);mutate(changed);assert.throws(()=>assert.deepEqual(changed,fixtureWdc3Layout(raw)));}
 const truncated=raw.subarray(0,200);assert.throws(()=>fixtureWdc3Layout(truncated));
});
test('wrapper-only PowerShell variables do not alter actual child environment identity',async()=>{
 const {agentEnvironment,environmentHash}=await import('./agent-environment.mjs');
 const config={prefix:'prefix',scratch:'scratch'},base={PATH:'tools',SystemRoot:'Windows',KEEP:'value'};
 const parent=agentEnvironment(config,base),child=agentEnvironment(config,{...base,PSModulePath:'wrapper modules',PSExecutionPolicyPreference:'Bypass'});
 assert.deepEqual(child,parent);assert.equal(environmentHash(child),environmentHash(parent));
 assert.notEqual(environmentHash(agentEnvironment(config,{...base,SystemRoot:'different'})),environmentHash(parent));
});
