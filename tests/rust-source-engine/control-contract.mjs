// Fixed control expectations from Lua source, not from candidate graph output.
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {join} from 'node:path';
import {createHash} from 'node:crypto';
import {sourcePoint} from './query-facade-contract.mjs';
const hash=b=>createHash('sha256').update(b).digest('hex');
export function controlContract(root) {
 const specs=[
  {name:'numeric',file:'control-numeric.lua',completion:'complete',freezeAccepted:false,freezeReason:'ambiguous execution occurrence:',calls:[
   {marker:'local result = Mark(i)',call:'Mark(i)',count:2,arguments:[1,2]},
   {marker:'local descending = Mark(i)',call:'Mark(i)',count:3,arguments:[3,2,1]},
   {marker:'Mark("zero_iterations")',call:'Mark("zero_iterations")',count:0}],
   selectedReturns:[{marker:'local first = previous()',call:'previous()',number:1},{marker:'local second = current()',call:'current()',number:2}],
   invariants:['per-iteration captured locals have distinct cells','first closure retains iteration1 after iteration2','scope activation and occurrence are source-backed']},
  {name:'repeat',file:'control-repeat.lua',completion:'complete',calls:[
   {marker:'Mark("once")',call:'Mark("once")',count:1},
   {marker:'Mark("break")',call:'Mark("break")',count:1},
   {marker:'Mark("after")',call:'Mark("after")',count:1}],
   invariants:['repeat body local done visible in condition','break exits exact loop, not function or whole file']},
  {name:'break',file:'control-break.lua',completion:'complete',calls:[
   {marker:'    Mark(i)',call:'Mark(i)',count:1,arguments:[1]},
   {marker:'Mark("after_break")',call:'Mark("after_break")',count:1}]},
  {name:'return',file:'control-return.lua',completion:'complete',calls:[
   {marker:'return Mark("early")',call:'Mark("early")',count:2,occurrenceKinds:['known-true-entry','unknown-true-path']},
   {marker:'Mark("continued")',call:'Mark("continued")',count:2,occurrenceKinds:['known-false-entry','unknown-false-path']},
   {marker:'return Mark(i)',call:'Mark(i)',count:1,arguments:[1]},
   {marker:'Mark("unreachable")',call:'Mark("unreachable")',count:0}],
   invariants:['unknown early-return paths remain separate','return paths have no following statement visits','mixed completion keeps both possible returns without fabricating one value']},
  {name:'while',file:'control-while.lua',completion:'unresolved',calls:[
   {marker:'Mark("reachable")',call:'Mark("reachable")',count:1,prefixOnly:true},
   {marker:'Mark("after_nontermination")',call:'Mark("after_nontermination")',count:0}],
   boundary:'no_proved_termination',truncated:false},
  {name:'generic',file:'control-generic.lua',completion:'unresolved',calls:[
   {marker:'    Mark(value)',call:'Mark(value)',count:0},
   {marker:'Mark("after_unknown_iterator")',call:'Mark("after_unknown_iterator")',count:0}],
   boundary:'unknown_iterator',truncated:false},
  {name:'budget',file:'control-budget.lua',completion:'budget',calls:[
   {marker:'    Mark(i)',call:'Mark(i)',count:64,arguments:'exact1through64'},
   {marker:'Mark("after_budget")',call:'Mark("after_budget")',count:0}],
   boundary:'loop_iteration_budget',truncated:true},
  {name:'zero-step',file:'control-zero-step.lua',completion:'unresolved',calls:[
   {marker:'Mark("after_zero_step")',call:'Mark("after_zero_step")',count:0}],
   boundary:'unsupported_zero_step',truncated:false,
   note:'Explicit prototype refusal/boundary; do not infer nontermination from another Lua version.'},
  {name:'function-budget',file:'control-function-budget.lua',completion:'budget',calls:[
   {marker:'        Mark(i)',call:'Mark(i)',count:64,arguments:'exact1through64'},
   {marker:'Mark("after_function_loop")',call:'Mark("after_function_loop")',count:0},
   {marker:'Mark("after_stopped_call")',call:'Mark("after_stopped_call")',count:0}],
   stoppedCaller:{marker:'local stopped = Limited()',call:'Limited()'},boundary:'loop_iteration_budget',truncated:true},
  {name:'function-generic',file:'control-function-generic.lua',completion:'unresolved',calls:[
   {marker:'        Mark(value)',call:'Mark(value)',count:0},
   {marker:'Mark("after_function_iterator")',call:'Mark("after_function_iterator")',count:0},
   {marker:'Mark("after_stopped_call")',call:'Mark("after_stopped_call")',count:0}],
   stoppedCaller:{marker:'local stopped = Unknown(...)',call:'Unknown(...)'},boundary:'unknown_iterator',truncated:false},
 ];
 const inputs=[];for(const spec of specs){const bytes=readFileSync(join(root,'tests/rust-source-engine/wow/native',spec.file));const text=bytes.toString('utf8');inputs.push({file:spec.file,diskPath:`tests/rust-source-engine/wow/native/${spec.file}`,bytes:bytes.length,sha256:hash(bytes)});for(const call of spec.calls){call.site=sourcePoint(text,spec.file,call.marker,call.call);assert(call.count===undefined||call.count>=0);}for(const item of spec.selectedReturns??[])item.site=sourcePoint(text,spec.file,item.marker,item.call);if(spec.stoppedCaller)spec.stoppedCaller.site=sourcePoint(text,spec.file,spec.stoppedCaller.marker,spec.stoppedCaller.call);}
 return{schema:'lycheedev.source-control-contract.v1',state:'fixed_spec_not_execution_evidence',inputs,cases:specs,
  concreteCallMeaning:'selected source closure invoked by the explicit abstract input; exclude source-template observations, never claim actual game execution',
  requiredEvidence:['original statement/condition/exit ranges','structured occurrence including loop iteration','typed fresh scope activation/cell','exact argument/return-slot source identities','control edges for return/break/continuation','prefix boundary and incomplete graph distinction','same admission deadline and shared steps','cancel classification and native owner/handle retirement'],
  scope:'common control facts prerequisite; no API host, secrecy transfer or 36-flow acceptance inferred'};
}
