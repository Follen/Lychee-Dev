// Cross-check projected query results with immutable source bytes and graph facts.
import assert from 'node:assert/strict';
import {readFileSync,writeFileSync,existsSync} from 'node:fs';
import {join,dirname,resolve} from 'node:path';
import {createHash} from 'node:crypto';
import {hashFile} from '../../tools/rust-build.mjs';
const [inputName,outName]=process.argv.slice(2),input=resolve(inputName),out=resolve(outName);assert(!existsSync(out));
const r=JSON.parse(readFileSync(input));assert.equal(r.schema,'lycheedev.source-query-owned.v1');
const checks=[];function check(name,expected,actual){let passed=true,error;try{assert.deepEqual(actual,expected)}catch(e){passed=false;error=e.message}checks.push({name,expected,actual,passed,error});}
const hash=x=>createHash('sha256').update(x).digest('hex'),digest=x=>hash(JSON.stringify(x));
const sources=new Map();for(const row of r.artifact.contract.inputs){const path=join(dirname(r.artifact.sealedBuild.binary),'inputs',row.diskPath);check(`sealed ${row.file}`,row.sha256,hashFile(path));sources.set(row.file,readFileSync(path));}
function text(s,allowLoadAnchor=false){const b=sources.get(s.file);assert(b&&s.start>=0&&(s.end>s.start||(allowLoadAnchor&&s.start===0&&s.end===0))&&s.end<=b.length);return b.subarray(s.start,s.end).toString('utf8');}
const site=s=>({file:s.file,start:s.start,end:s.end});
const decision=d=>d.detail===undefined?{state:d.state}:{state:d.state,detail:d.detail};
check('owned root/tree/handles and original business gate pass',true,r.passed&&r.handlesReaped);
check('matching native EXE',r.artifact.binarySHA256,hashFile(r.artifact.binary));check('matching PDB',r.artifact.pdbSHA256,hashFile(r.artifact.pdb));
const scenarios=r.frames.filter(x=>x.event==='scenario'),graphs=new Map(scenarios.map(s=>[s.name,s.graph]));
for(const s of scenarios){const g=s.graph,p=s.name;check(`${p} snapshot bytes`,g.snapshot_key,hash(g.snapshot_inputs_json));const i=JSON.parse(g.snapshot_inputs_json);check(`${p} actual owned admission`,i.admissionSHA256,digest(i.admission));check(`${p} no source/config execution`,[false,false],[i.admission.sourceExecution,i.admission.configExecution]);
 const n=g.native_invocations;check(`${p} contiguous storage owners`,true,n.storages.every(x=>x.owner&&x.cell));check(`${p} storage identity unique`,n.storages.length,new Set(n.storages.map(x=>x.cell)).size);
 for(const capture of n.capture_facts){const closure=n.closures[capture.closure];check(`${p} capture ${capture.closure}:${capture.declaration.start} actual captured cell`,capture.cell,closure.captures[`${capture.declaration.file}:${capture.declaration.start}:${capture.declaration.end}`]);check(`${p} capture original name`,true,text(capture.declaration).length>0);check(`${p} capture has typed storage`,true,n.storages.some(x=>x.cell===capture.cell));}
 for(const f of g.uses){check(`${p} use ${f.id} original range`,true,text(f.site).length>0);check(`${p} use ${f.id} immutable proof`,digest([g.snapshot_key,f.id,site(f.site),f.context,decision(f.decision),f.writes,f.object_view??null]),f.proof_id);}
}
const roleGraph=graphs.get('query-roles');assert(roleGraph);
const vid=(g,id)=>digest([g.snapshot_key,'value',id]);
for(const row of r.frames.filter(x=>x.event==='definition'&&x.case==='roles')){
 const d=row.result;check(`${row.query} original use text`,true,text(d.site).length>0);
 const uses=roleGraph.uses.filter(f=>JSON.stringify(f.site)===JSON.stringify(d.site)&&f.context.startsWith('load:0:'));check(`${row.query} one selected occurrence`,1,uses.length);assert.equal(uses.length,1);const f=uses[0];
 if(d.selected.state==='known'){const v=roleGraph.values[f.decision.detail],fn=v.kind.kind==='function'?roleGraph.functions[v.kind.detail]:null;check(`${row.query} actual selected value`,vid(roleGraph,v.id),d.selected.value.value_id);check(`${row.query} actual original value`,v.origin,d.selected.value.origin);check(`${row.query} actual function declaration`,fn?.declaration??null,d.selected.value.function_declaration);check(`${row.query} actual occurrence proof`,f.proof_id,d.selected.value.proof);}
 else check(`${row.query} reasoned unavailable`,true,['unavailable','known_absent'].includes(f.decision.state));
 if(d.lexical.state==='known'){const declaration=d.lexical.value;check(`${row.query} original lexical declaration`,true,text(declaration.site).length>0);check(`${row.query} lexical identity`,digest([JSON.parse(roleGraph.snapshot_inputs_json).admissionSHA256,'declaration',site(declaration.site)]),declaration.id);}
}
const refs=r.frames.find(x=>x.event==='value_references').page.rows;
for(const ref of refs){const prefix=`reference ${ref.id}`;check(`${prefix} honest coverage`,'abstract_input_observation'===ref.coverage||'abstract_input_call'===ref.coverage,true);
 if(ref.roles.includes('read')){const f=roleGraph.uses.find(f=>digest([roleGraph.snapshot_key,'read',f.id])===ref.id);check(`${prefix} exact read fact`,true,!!f);if(f){check(`${prefix} exact source`,f.site,ref.site);check(`${prefix} exact proof`,f.proof_id,ref.proof);}}
 if(ref.roles.includes('call')){const c=roleGraph.calls.find(c=>digest([roleGraph.snapshot_key,'callee',c.id])===ref.id);check(`${prefix} exact call fact`,true,!!c);if(c){check(`${prefix} actual callee source`,roleGraph.uses[c.callee].site,ref.site);check(`${prefix} same callee proof`,roleGraph.uses[c.callee].proof_id,ref.proof);}}
 if(ref.storage){const storage=roleGraph.native_invocations.storages.find(x=>digest([roleGraph.snapshot_key,'storage',x.cell])===ref.storage);check(`${prefix} exact typed storage`,storage?.owner,ref.storage_owner);}
}
for(const call of r.frames.find(x=>x.event==='value_calls').page.rows){const actual=roleGraph.calls.find(c=>digest([roleGraph.snapshot_key,'call',c.id])===call.id);check(`call ${call.id} actual abstract event`,true,!!actual);check(`call ${call.id} no runtime claim`,'explicit_input_abstract_call',call.evidence_kind);if(actual){check(`call ${call.id} full source`,actual.site,call.site);check(`call ${call.id} exact callee`,roleGraph.uses[actual.callee].site,call.callee);}}
for(const frame of r.frames.filter(x=>x.event==='value_context'||x.event==='paging'))for(const row of (frame.page??frame.context).rows){check(`context ${row.id} exact original displayed bytes`,text(row.exhibited,row.kind==='load_occurrence'),row.snippet);check(`context ${row.id} original full range retained`,true,row.origin.start<=row.exhibited.start&&row.origin.end>=row.exhibited.end);if(row.kind==='load_occurrence')check(`context ${row.id} metadata anchor carries no source execution`,false,row.detail.sourceExecution);}
const result={schema:'lycheedev.source-query-independent.v1',passed:checks.every(x=>x.passed),checks,inputReport:input,inputReportSHA256:hashFile(input),scope:'fixed source/proof/declaration/value/storage/reference/call/snippet projection; not full Source'};writeFileSync(out,JSON.stringify(result,null,2));console.log(JSON.stringify({out,passed:result.passed,checks:checks.length,failed:checks.filter(x=>!x.passed).map(x=>x.name)}));if(!result.passed)process.exitCode=1;
