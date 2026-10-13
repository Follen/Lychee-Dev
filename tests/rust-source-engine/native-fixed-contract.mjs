// Root expectations are derived from these fixed source bytes, not analyzer
// results. Context hash expectations are resolved from independently checked
// snapshot inputs, rather than accepting the probe's assertion.expected field.
import assert from 'node:assert/strict';
import {existsSync,readFileSync,writeFileSync} from 'node:fs';
import {join,resolve,sep} from 'node:path';
import {hashFile} from '../../tools/rust-build.mjs';
import {originalV7Contract} from './wow-v7-contract.mjs';

const root=resolve('.'),out=resolve(process.argv[2]);assert(out.startsWith(root+sep)&&!existsSync(out));
const rows=[
 ['generic-defs.lua','20c76a693bf6c1b007659e2f9523027c4f3a6cc8e67511c6e0035bced2b273a1'],
 ['generic-calls.lua','bb1d4a042e543d3521149e3713052e17ac648fe0b02da1e627c3d210dbf3d660'],
 ['generic-replace.lua','cf62e412269815ddb9e61afdc1144116849d6bda7686d1be1e7db639fb50ec62'],
];
const profile=originalV7Contract(root).fixtures.find(row=>row.file==='profile.lua');
const inputs=[{file:'profile.lua',diskFile:'profile.lua',bytes:profile.bytes,sha256:profile.sha256}];
for(const [file,sha256] of rows){const diskFile=`native/${file}`,path=join(root,'tests/rust-source-engine/wow',diskFile);assert.equal(hashFile(path),sha256);inputs.push({file,diskFile,bytes:readFileSync(path).length,sha256});}
const bodies=new Map(inputs.map(row=>[row.file,readFileSync(join(root,'tests/rust-source-engine/wow',row.diskFile))]));
function site(file,marker,token){
 const source=bodies.get(file),name=Buffer.from(token);let mark=Buffer.from(marker),at=source.indexOf(mark);
 if(at<0){mark=Buffer.from(marker.replace(/\r?\n/g,'\r\n'));at=source.indexOf(mark);}
 assert(at>=0&&source.indexOf(mark,at+1)<0);const delta=mark.indexOf(name);assert(delta>=0);
 return {file,start:at+delta,end:at+delta+name.length};
}
const queries=[
 ['identity_number','local numberResult = Identity(7)','Identity(7)','number','Identity'],
 ['identity_string','local stringResult = Identity("seven")','Identity("seven")','string','Identity'],
 ['copied_number','local copiedNumber = Copied.echo(7)','Copied.echo(7)','number','Identity'],
 ['replaced_string','local replacedString = Source.echo("seven")','Source.echo("seven")','string','Replacement'],
 ['colon_number','local colonNumber = Methods:Echo("label", 7)','Methods:Echo("label", 7)','number','Echo'],
 ['dot_string','local dotString = Methods.Echo(Methods, "label", "seven")','Methods.Echo(Methods, "label", "seven")','string','Echo'],
 ['copied_return','function WrongCopiedReturn()\n    return Copied.echo(7)','Copied.echo(7)','number','Identity'],
 ['method_return','function WrongMethodReturn()\n    return Methods:Echo("label", 7)','Methods:Echo("label", 7)','number','Echo'],
 ['late_return','function WrongLateReturn()\n    return Identity(7)','Identity(7)','string','Replacement'],
];
const scenarios=['after-a','unknown-a','after-b'],interleave=[0,1,2,0,2,1],assertions=[],queryExpectations=[];
for(const [label,marker,expression,baseType,baseFunction] of queries){
 const sourceSite=site('generic-calls.lua',marker,expression);
 for(const [step,index] of interleave.entries()){
  const scenario=scenarios[index],unknown=index===1&&label.endsWith('return');
  const expected=unknown?'unknown':index===2&&label==='late_return'?'number':baseType;
  const selectedFunction=unknown?null:index===2&&label==='late_return'?'Identity':baseFunction;
  assertions.push({name:`${scenario}:${label}:interleave:${step}:native_type`,expected},
   {name:`${scenario}:${label}:interleave:${step}:model_context`,expectedRef:{kind:'snapshotKey',scenario}},
   {name:`${scenario}:${label}:interleave:${step}:managed_candidate_count`,expected:unknown?0:1});
  queryExpectations.push({scenario,query:label,interleaveStep:step,sourceSite,nativeFamily:expected,selectedFunction,typeCoverage:unknown?'unavailable':'native_projected',candidateCount:unknown?0:1});
 }
}
const diagnosticExpectations=[];
for(const [index,scenario] of scenarios.entries()){
 assertions.push({name:`${scenario}:foreign_model_inherits_context`,expectedRef:{kind:'snapshotKey',scenario}},
  {name:`${scenario}:foreign_formal_parameter_identity`,expected:true});
 const rows=[
  ['copied_wrong_return',queries[6][1],queries[6][2],'return-type-mismatch',index!==1],
  ['method_wrong_return',queries[7][1],queries[7][2],'return-type-mismatch',index!==1],
  ['late_wrong_return',queries[8][1],queries[8][2],'return-type-mismatch',index===0],
  ['wrong_label','local wrongLabel = Methods:Echo(99, 7)','99','param-type-mismatch',true],
  ['late_wrong_argument',queries[8][1],'7','param-type-mismatch',index===0],
 ];
 for(const [name,marker,token,code,enabled] of rows){
  const sourceSite=site('generic-calls.lua',marker,token);
  assertions.push({name:`${scenario}:${name}:native_diagnostic_original_range`,expected:enabled?1:0});
  diagnosticExpectations.push({scenario,name,sourceSite,code,expectedCount:enabled?1:0});
 }
 assertions.push({name:`${scenario}:native_diagnostic_total`,expected:[5,1,3][index]},
  {name:`${scenario}:late_return_check_coverage`,expected:index===1?'unavailable':'native_checked'},
  {name:`${scenario}:copied_value_reference_overlay`,expected:true});
}
assertions.push({name:'same_database_phase_order_context_keys_distinct',expected:3});
assert.equal(assertions.length,193);assert.equal(new Set(assertions.map(row=>row.name)).size,193);
const contract={schema:'lycheedev.emmy-native-context-contract.v1',rulesVersion:'wow-native-binding-context-v1',capabilityProfile:'typed-call-v1',inputs,scenarios,interleave,assertions,queryExpectations,diagnosticExpectations,
 scope:'fixed annotated pure-return signatures, copy identity and native argument/return diagnostics; no general effects/flow or complete Source acceptance'};
writeFileSync(out,JSON.stringify(contract,null,2)+'\n',{flag:'wx'});console.log(JSON.stringify({path:out,sha256:hashFile(out),assertions:assertions.length,queries:queryExpectations.length}));
