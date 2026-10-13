// Fixed old inputs plus explicit supplementary test-host assumptions.
// This module does not invoke a Lua interpreter, analyzer or old flow binder.
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {join} from 'node:path';
import {createHash} from 'node:crypto';
const sha=value=>createHash('sha256').update(value).digest('hex');
export function commonFlowContract(root) {
 const matrixPath=join(root,'tests/rust-source-engine/source-business-matrix.json');
 const frozenPath=join(root,'crates/lycheedev-source/src/flow/fixtures/frozen-go.json');
 const matrixBytes=readFileSync(matrixPath),frozenBytes=readFileSync(frozenPath);
 const matrix=JSON.parse(matrixBytes),frozen=JSON.parse(frozenBytes);
 assert.equal(matrix.flowCases.length,36);assert.equal(frozen.cases.length,36);
 assert.deepEqual(matrix.flowRules,frozen.rules,'flow rule bytes/shape are original frozen data');
 const host={schema:'lycheedev.fixed-flow-host.v1',scope:'explicit toy test environment only; not production WoW API effect acceptance',
  rules:{sha256:sha(Buffer.from(JSON.stringify(matrix.flowRules))),evidence:'complete frozen rules JSON and JSON pointer; path/line are metadata labels, no api.lua source bytes supplied'},
  callables:Object.keys(matrix.flowRules.calls).sort().map(name=>({name,identity:`fixed-flow-api:${name}`,effect:'no_lua_heap_write',retainsArguments:false,sourceExecution:false})),
  predicates:[{name:'issecretvalue',identity:'fixed-flow-guard:issecretvalue',argument:1,meaning:'true/false branch constraints on exact selected value and write observation',effect:'no_lua_heap_write',retainsArguments:false}],
  initialObjects:[{name:'Box',identity:'fixed-flow-object:Box',fields:[],purpose:'original F05 method fixture requires preexisting receiver'}],
  unresolvedGlobals:'unavailable_external_binding; never infer a tested API solely from source spelling',
  sourceRisk:'secret return labels mark a potentially secret candidate, not proof that a guard always returns true',
  operationRules:'missing arithmetic/comparison/concat/index rules remain explicit unknown',
  sourceExecution:false,configExecution:false};
 const cases=matrix.flowCases.map((row,index)=>{
  const old=frozen.cases[index];assert.equal(row.name,old.name);assert.equal(row.input.fixturePointer,`/cases/${index}`);
  assert.deepEqual(row.frozenExpected,old.expected,'immutable old output remains independent of business correctness');
  const originals=row.input.files.map(file=>{const bytes=Buffer.from(file.text);assert.equal(bytes.length,file.bytes);assert.equal(sha(bytes),file.sha256);return{path:file.path,text:file.text,bytes:file.bytes,sha256:file.sha256};});
  // Frozen fixtures name a requested function scope but do not establish that
  // the actual game called it. The prototype explores an explicit abstract entry.
  const entry=row.input.target.symbol?{kind:'abstract_function_entry',path:row.input.target.path,symbol:row.input.target.symbol,arguments:'symbolic'}:
   row.id==='F36'?{kind:'ambiguous_line_target',path:row.input.target.path,line:row.input.target.line}:{kind:'ordered_top_level',path:row.input.target.path};
  const load=row.id==='F03'?{state:'explicit',ordered:['wrapper.lua','main.lua'],reason:'unique wrapper is established before the original main input'}:
   row.id==='F12'?{state:'unknown',candidates:[['a.lua','b.lua','main.lua'],['b.lua','a.lua','main.lua']],reason:'conflicting global Wrap definitions; no last-file choice is authorized'}:
   {state:'explicit',ordered:originals.map(file=>file.path)};
  return{id:row.id,name:row.name,originals,target:row.input.target,budget:row.input.budget,entry,load,hostSHA256:sha(Buffer.from(JSON.stringify(host))),
   expectedBusiness:row.requiredBusiness,frozenExpected:row.frozenExpected,additionalProfiles:row.id==='F36'?[{kind:'exact_symbol',symbol:'A',secretUseExecutionOccurrences:1}]:
    row.id==='F30'?[{kind:'trusted_builtin_pairs_empty_table',requires:'separate explicit builtin identity/effect material',secretUseExecutionOccurrences:0}]:[]};
 });
 return{schema:'lycheedev.common-flow-contract.v1',state:'fixed_spec_not_execution_evidence',matrixSHA256:sha(matrixBytes),frozenSHA256:sha(frozenBytes),host,hostSHA256:sha(Buffer.from(JSON.stringify(host))),rules:matrix.flowRules,cases,
  requiredOutput:['original source/rule evidence distinction','call/argument/return-slot/value/write identities','ordered proof path','guard dominance on exact value','abstract occurrence and multiplicity','binding uncertainty and its dependencies','coverage/truncation/pagination independently'],
  scope:'all36 frozen Source flow inputs; common facts executor and full business acceptance still required'};
}
