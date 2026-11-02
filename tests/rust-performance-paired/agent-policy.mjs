import assert from 'node:assert/strict';
import {SQL,dataCases} from './data-contract.mjs';
import {CSV_SQL} from './agent-fixture-inputs.mjs';
import {queryFor} from './agent-audit.mjs';
export {agentEnvironment} from './agent-environment.mjs';
import {statistics,REPETITIONS,TARGET_RATIO,elapsed} from './contract.mjs';

// Direct CLI calls already have the business lifecycle as their owned interval.
// Agent calls retain that original controller interval beside the complete
// spawn-through-close interval measured by the byte-bound Agent controller.
export function businessElapsed(proof,agentPacket=null){
 const controllerTiming=elapsed(proof);
 if(agentPacket===null)return{controllableMillis:controllerTiming.controllableMillis,controllerTiming,timingBoundary:'before_cli_create_through_tree_reap_and_output_drain'};
 const t=agentPacket.businessTiming;
 assert.equal(t?.schema,'lycheedev.agent-business-timing.v1','Agent lifecycle timing required');
 assert.deepEqual(agentPacket.child,{code:0,signal:null});
 for(const key of ['startNs','endNs'])assert.match(t[key],/^[1-9]\d*$/);
 const start=BigInt(t.startNs),end=BigInt(t.endNs);
 assert(end>start,'ordered Agent lifecycle required');
 const controllableMillis=Number(end-start)/1e6;
 assert(Number.isFinite(controllableMillis)&&controllableMillis<=controllerTiming.controllableMillis,'Agent lifecycle must fit the complete controller interval');
 return{controllableMillis,controllerTiming,timingBoundary:'before_agent_spawn_through_child_close_and_output_drain',businessTiming:t};
}
export function requiredFixtureKinds(c){
 assert(['data','generation','csv'].includes(c.mode));
 if(c.mode!=='data')return ['numeric'];
 const base=dataCases().find(e=>e.id===c.caseId);
 if(base)return [base.fixture];
 assert(/^B04-(palette|bgra8|bc1|bc2|bc3)-(png|webp)$/.test(c.caseId),'one explicit current Data case per bounded group');
 return ['images'];
}
export function selection(c,fixtures){assert(['data','generation','csv'].includes(c.mode));if(c.mode!=='data')return{id:'agent-'+c.mode,kind:'sql',fixture:'numeric',performance:c.mode==='generation',args:[]};const cases=dataCases(fixtures.images?.images);const selected=cases.find(e=>e.id===c.caseId);assert(selected,'one explicit current Data case per bounded group');return selected;}
export function phases(c,engine){return c.mode==='generation'?['fresh-wdc',...(engine==='rust'?['prepare']:[]),'first-after-prepare','subsequent-hit']:['cold','warm'];}
export const fixtureKind=e=>e.fixture==='catalog'?'catalog':'numeric';
export function productArgs(c,e,fixture,engine,phase,home,output){const command=phase==='prepare'?['data','cache','prepare','NumericSample']:e.kind==='sql'?['data','sql','--sql',e.fixture==='catalog'?SQL:CSV_SQL]:[...e.args,...(e.kind==='image'?['--output',output]:[])];assert(!(engine==='rust'&&e.kind==='sql'&&phase!=='prepare'),'Rust SQL must use Agent helper');return[...command,'--snapshot',fixture.identity.snapshot,'--installation',fixture.identity.installation,'--home',home,'--format','json','--offline'];}
export function helperRequest(c,e,fixture,home,output){return{schema:'lycheedev.paired-agent-request.recovered.v1',binary:c.rust.sourceBinary,home,snapshot:fixture.identity.snapshot,installation:fixture.identity.installation,root:c.root,query:queryFor(fixtureKind(e)),output:output??null};}
export function ownerInputs(c,e,fixture){return{rust:c.rust.binarySHA256,go:c.go.binarySHA256,files:c.inputs,fixture:fixture.receipt.inventory.sha256,case:e.id};}
export function groups(rows,c,e){const count=c.acceptance?REPETITIONS:1;assert.equal(rows.length,count);const keys=c.mode==='generation'?['fresh-wdc','prepare-inclusive-first','first-after-prepare','subsequent-hit']:['cold','warm'];return keys.map(phase=>{const timings=Object.fromEntries(['go','rust'].map(engine=>[engine,statistics(rows.map(r=>phase==='prepare-inclusive-first'?r[engine]['first-after-prepare'].controllableMillis+(engine==='rust'?r.rust.prepare.controllableMillis:0):r[engine][phase].controllableMillis))]));const evaluated=c.acceptance&&e.performance&&phase!=='first-after-prepare';const ratio=timings.rust.median/timings.go.median;return{case:e.id,phase,count,timings,ratio:c.mode==='csv'?null:ratio,timingComparability:c.mode==='csv'?'different-output-workloads-correctness-only':'paired-selected-workload',performanceAcceptanceEvaluated:evaluated,state:evaluated?(ratio<=TARGET_RATIO?'passed_selected_path':'failed_performance'):'equivalent_correctness_only',fullB04Acceptance:false};});}



export function comparison(c,e){const timing={timingBasis:'complete_business_process_lifecycle',originalControllerTiming:'retained per sample without subtraction'};return c.mode==='csv'?{...timing,baseline:'authenticated Go324 SQL JSON and complete query CAP',candidate:'Agent query plus every CSV Export page and full query CAP',csv:'all fields quoted, doubled embedded quotes, CRLF, complete concatenated output',performanceAcceptanceEligible:false,reason:'JSON and CSV perform different output work; timings are not a performance comparison'}:{...timing,baseline:'authenticated Go324 production workload',candidate:'Agent typed query or unchanged asset workload',performanceAcceptanceEligible:e?.performance===true,reason:e?.performance?'selected bounded workload only; not full architecture or B04 acceptance':'selected catalog correctness fixture; no performance acceptance'};}
