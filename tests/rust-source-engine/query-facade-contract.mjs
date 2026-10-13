// Source-derived expectations: no parser, engine output or source execution.
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {join} from 'node:path';
import {createHash} from 'node:crypto';
const sha=bytes=>createHash('sha256').update(bytes).digest('hex');
export function sourcePoint(text,file,marker,token,{last=false}={}) {
 const source=Buffer.from(text),needle=Buffer.from(marker),start=source.indexOf(needle);
 assert(start>=0&&source.indexOf(needle,start+1)<0,`unique exact marker: ${file} ${marker}`);
 const at=last?needle.lastIndexOf(Buffer.from(token)):needle.indexOf(Buffer.from(token));assert(at>=0);
 return {file,start:start+at,end:start+at+Buffer.byteLength(token)};
}
export function queryFacadeContract(root) {
 const names=['query-roles.lua','query-captures.lua','query-load-a.lua','query-load-b.lua'];
 const inputs=names.map(file=>{const bytes=readFileSync(join(root,'tests/rust-source-engine/wow/native',file));return {file,diskFile:`native/${file}`,bytes:bytes.length,sha256:sha(bytes),text:bytes.toString('utf8')};});
 const byName=new Map(inputs.map(row=>[row.file,row.text]));
 const at=(file,marker,token,options)=>sourcePoint(byName.get(file),file,marker,token,options);
 const roles='query-roles.lua';
 const original=at(roles,'local function Original(v)','Original');
 const replacement=at(roles,'local function Replacement(v)','Replacement');
 const alias=at(roles,'local alias = Original','alias');
 const saved=at(roles,'local saved = alias','saved');
 const shadow=at(roles,'    local saved = Replacement','saved');
 // The selected function and lexical storage declaration are deliberately
 // separate. A copied value retains its source after the original cell changes.
 const definitions=[
  ['alias_original','local alias = Original','Original',original,original,['read']],
  ['saved_from_alias','local saved = alias','alias',alias,original,['read']],
  ['function_value','local functionValueRead = saved','saved',saved,original,['read']],
  ['saved_call','local firstCall = saved(1)','saved',saved,original,['read','call']],
  ['alias_after_write','local secondCall = alias("two")','alias',alias,replacement,['read','call']],
  ['block_shadow','local shadowCall = saved(3)','saved',shadow,replacement,['read','call']],
  ['member_saved','local memberSaved = object.run','object.run',null,original,['read']],
  ['saved_member_call','local savedMemberCall = memberSaved(4)','memberSaved',at(roles,'local memberSaved = object.run','memberSaved'),original,['read','call']],
  ['current_member_call','local currentMemberCall = object.run(5)','object.run',null,replacement,['read','call']],
  ['other_object_call','local otherMemberCall = other.run(6)','other.run',null,replacement,['read','call']],
 ].map(([label,marker,token,lexicalDeclaration,functionDeclaration,roles])=>({label,site:at('query-roles.lua',marker,token),lexicalDeclaration,selected:{state:'known',kind:'function',functionDeclaration},roles}));
 definitions.push({label:'dynamic_member',site:at(roles,'local dynamicMember = object[unknownKey]','object[unknownKey]'),lexicalDeclaration:null,selected:{state:'unavailable'},roles:['read']});
 const originalReads=[
  ['local alias = Original','Original',['read']],
  ['local saved = alias','alias',['read']],
  ['local functionValueRead = saved','saved',['read']],
  ['local firstCall = saved(1)','saved',['read','call']],
  ['local object = { run = Original }','Original',['read']],
  ['local memberSaved = object.run','object.run',['read']],
  ['local savedMemberCall = memberSaved(4)','memberSaved',['read','call']],
 ].map(([marker,token,roles])=>({site:at('query-roles.lua',marker,token),roles}));
 const captures='query-captures.lua';
 const sharedDeclaration=at(captures,'local value = 1','value');
 const sharedUse=at(captures,'local function Read() return value end','value');
 const captureCases=[
  {label:'shared_first',call:at(captures,'local first = Read()','Read()'),body:sharedUse,lexicalDeclaration:sharedDeclaration,valueOrigin:at(captures,'local value = 1','1')},
  {label:'shared_second',call:at(captures,'local second = Read()','Read()'),body:sharedUse,lexicalDeclaration:sharedDeclaration,valueOrigin:at(captures,'value = "later"','"later"')},
 ];
 return {schema:'lycheedev.source-query-facade-contract.v1',state:'fixed_expectations_not_execution_evidence',inputs:inputs.map(({text,...row})=>row),definitions,originalReads,captureCases,
  requiredControls:['same-input stable page order','no duplicate/omitted page rows','cursor foreign-owner rejection','cursor changed target/filter/selector rejection','single-row byte-budget failure','cancelled original control','expired original control'],
  requiredMultiplicity:['two factory closures and captured cells remain distinct','load A-before-B never resolves a future Shared definition','B-before-A resolves declared Shared','repeated A requires explicit load selector','same source function body needs explicit invocation selector'],
  scope:'common-fact query facade subset of G01-G05; XML/Flow/full Source remain separate mandatory gates'};
}
export function queryProbeContract(root) {
 const base=queryFacadeContract(root);
 const extras=[['scope.lua','golden/scope.lua'],['writes-redeclaration.lua','golden/writes-redeclaration.lua'],['lexical-negative.lua','wow/native/query-lexical-negative.lua'],['query-loop-scopes.lua','wow/native/query-loop-scopes.lua']].map(([file,path])=>{
  const bytes=readFileSync(join(root,'tests/rust-source-engine',path));return{file,diskPath:`tests/rust-source-engine/${path}`,bytes:bytes.length,sha256:sha(bytes),text:bytes.toString('utf8')};
 });
 const text=new Map(extras.map(row=>[row.file,row.text]));
 const at=(file,marker,token,options)=>sourcePoint(text.get(file),file,marker,token,options);
 const scopeOuter=at('scope.lua','local value = 1','value'),scopeInner=at('scope.lua','        local value = 2','value');
 const first=at('writes-redeclaration.lua','local reused = 0','reused'),second=at('writes-redeclaration.lua','local reused = 2','reused');
 const lexical=[['captured_initializer',scopeOuter],['inner_read',scopeInner],['outer_return',scopeOuter],['first_write',first],['old_value',first],['new_write',second],['new_return',second],['loop_local',at('lexical-negative.lua','for value=1,2 do','value')],['parameter_return',at('lexical-negative.lua','local function f(value)','value')],['initializer_outer',at('lexical-negative.lua','local value=1','value')]];
 const assertions=[];const add=(name,expected)=>assertions.push({name,expected});
 for(const[label,site]of lexical){add(`query:lexical:${label}:exact_binding`,site);add(`query:lexical:${label}:no_execution_invented`,true);}
 add('query:lexical:comments_strings_not_calls',[]);
 const loop='query-loop-scopes.lua',outer=at(loop,'local value = 10','value'),numeric=at(loop,'for value = value, value + 2 do','value'),generic=at(loop,'for value in Iterator(value) do','value'),nestedOuter=at(loop,'for value = 1, 2 do','value'),nestedInner=at(loop,'    for value = value, value + 1 do','value'),recursive=at(loop,'local function Recursive(n)','Recursive');
 for(const[label,site]of[['numeric_header_start',outer],['numeric_header_limit',outer],['numeric_body',numeric],['numeric_after',outer],['generic_header',outer],['generic_body',generic],['generic_after',outer],['nested_header_start',nestedOuter],['nested_header_limit',nestedOuter],['nested_body',nestedInner],['nested_after_inner',nestedOuter],['nested_after_outer',outer],['local_function_recursion',recursive],['recursive_upvalue',outer]])add(`query:loop_scope:${label}:exact_binding`,site);
 for(const row of base.definitions.filter(row=>row.label!=='dynamic_member')){add(`query:roles:${row.label}:lexical`,row.lexicalDeclaration);add(`query:roles:${row.label}:selected`,row.selected.functionDeclaration);}
 add('query:roles:dynamic_member:unavailable',true);
 add('query:roles:original_exact_inverse_read_call_set',base.originalReads.map(row=>({...row,roles:[...row.roles].sort()})));
 add('query:roles:original_reference_page_complete',false);
 const roleText=readFileSync(join(root,'tests/rust-source-engine/wow/native/query-roles.lua'),'utf8');
 add('query:roles:original_exact_calls',[sourcePoint(roleText,'query-roles.lua','local firstCall = saved(1)','saved(1)'),sourcePoint(roleText,'query-roles.lua','local savedMemberCall = memberSaved(4)','memberSaved(4)')]);
 add('query:captures:body_needs_selector',true);
 for(const row of base.captureCases){const label=row.label==='shared_first'?'first':'second';add(`query:captures:${label}:origin`,row.valueOrigin);add(`query:captures:${label}:lexical`,row.lexicalDeclaration);}
 add('query:captures:shared_cell_distinct_write',true);add('query:captures:factory_cells_are_distinct',2);
 const shared=sourcePoint(readFileSync(join(root,'tests/rust-source-engine/wow/native/query-load-b.lua'),'utf8'),'query-load-b.lua','function Shared(value)','Shared');
 for(const[label,known]of[['before',false],['after',true],['repeated_before',false],['repeated_after',true]]){add(`query:loads:${label}:selected`,known?shared:null);if(!known)add(`query:loads:${label}:known_absent`,true);if(label==='repeated_after')add('query:loads:repeated_requires_selector',true);}
 // The complete ordered ID set is checked through an independent relation:
 // page union/repeated page must equal the separately returned full page.
 for(const name of ['exact_ordered_page_union','no_duplicate_records','repeat_first_page_stable'])add(`query:paging:${name}`,{relation:'reported_expected_equals_actual_and_independent_full_page'});
 for(const name of ['changed_filter_rejected','changed_limit_rejected','changed_target_rejected','changed_selector_rejected','foreign_owner_rejected'])add(`query:paging:${name}`,4);
 add('query:paging:single_row_budget_explicit','source.native_query_record_too_large');add('query:paging:context_snippets_exact_original_bytes',true);add('query:paging:cancel_during_same_operation',{code:'command.cancelled',exit:7});
 return{schema:'lycheedev.source-query-probe-contract.v1',inputs:[...base.inputs.map(({diskFile,...row})=>({...row,diskPath:`tests/rust-source-engine/wow/${diskFile}`})),...extras.map(({text,...row})=>row)],assertions,sourceExpectations:base,
  scope:'fixed common-fact query facade; independent source sites and full reference/call sets; no full Source acceptance'};
}
