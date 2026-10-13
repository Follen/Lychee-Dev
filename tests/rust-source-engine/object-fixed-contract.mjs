// Source-based expectations for finite static fields and shallow copy versions.
import assert from 'node:assert/strict';
import {existsSync,readFileSync,writeFileSync} from 'node:fs';
import {join,resolve,sep} from 'node:path';
import {hashFile} from '../../tools/rust-build.mjs';
const root=resolve('.'),out=resolve(process.argv[2]);assert(out.startsWith(root+sep)&&!existsSync(out));
const base=JSON.parse(readFileSync(join(root,'tests/rust-source-engine/context-native-contract.json')));
const file='object-versions.lua',diskFile=`native/${file}`,path=join(root,'tests/rust-source-engine/wow',diskFile),source=readFileSync(path);
assert.equal(source.length,1293);assert.equal(hashFile(path),'d084dc1d72825b47337cbf369abbe30a847a9a9dcfbd46e6f69af6b9a496666f');
const inputs=[...base.inputs,{file,diskFile,bytes:source.length,sha256:hashFile(path)}];
const specs=[
 {label:'source_old',value:'number',run:'generic_function',added:false,compatible:true},
 {label:'source_new',value:'string',run:'string_function',added:true,compatible:false},
 {label:'copied_old',value:'number',run:'generic_function',added:false,compatible:true},
 {label:'copied_new',value:'string',run:'string_function',added:true,compatible:false},
 {label:'twin',value:'string',run:'string_function',added:true,compatible:false},
 {label:'dynamic',value:'unknown',run:'unknown',added:true,compatible:false,dynamic:true},
];
function members(spec){
 const result={keys:spec.added?['added','run','value']:['run','value'],fields:{}};
 for(const [field,label] of [['value',spec.value],['run',spec.run],['added',spec.added?'boolean':null]])
  result.fields[field]={memberInfo:label,withKey:label?[label]:[],withKeyAll:label?[label]:[],memberType:label};
 return result;
}
const assertions=[];function add(name,expected){assertions.push({name,expected});}
for(const [step,index] of [0,1,0,2,3,2,4,5].entries()){
 const spec=specs[index],prefix=`object_red:${spec.label}:step:${step}`;
 add(`${prefix}:frozen_object_view`,'recorded');
 if(spec.dynamic){add(`${prefix}:members_not_known`,true);add(`${prefix}:foreign_members_not_known`,true);}
 else{add(`${prefix}:member_api_matrix`,members(spec));add(`${prefix}:foreign_member_api_matrix`,members(spec));}
 add(`${prefix}:direct_index`,{value:spec.value,run:spec.run,added:spec.dynamic?'unknown':spec.added?'boolean':'nil'});
 add(`${prefix}:public_structure_boolean`,spec.compatible);add(`${prefix}:foreign_structure_boolean`,spec.compatible);
 add(`${prefix}:public_structure_coverage`,spec.dynamic?'unavailable':'native_checked');
 assertions.push({name:`${prefix}:foreign_context`,expectedRef:{kind:'snapshotKey',scenario:'object-versions'}});
}
for(const [label,same] of [['source_changed',true],['copied_changed',true],['source_vs_copy',false],['same_shape_twin',false],['dynamic_changed',true]]){
 add(`object_red:${label}:object_identity`,same);if(same)add(`object_red:${label}:epoch_changed`,true);
 add(`object_red:${label}:native_type_identity_distinct`,true);
}
add('object_red:same_shape_distinct_reference_evidence',true);
for(const [label,index] of [['generic_old',0],['generic_new',1],['generic_copy',3],['generic_dynamic',5]]){
 const spec=specs[index];add(`object_red:${label}:version_preserved`,true);
 if(spec.dynamic){add(`object_red:${label}:members_not_known`,true);add(`object_red:${label}:foreign_members_not_known`,true);}
 else{add(`object_red:${label}:member_api_matrix`,members(spec));add(`object_red:${label}:foreign_member_api_matrix`,members(spec));}
 add(`object_red:${label}:public_structure_boolean`,spec.compatible);
}
function site(marker,token){const mark=Buffer.from(marker),name=Buffer.from(token),at=source.indexOf(mark),offset=mark.indexOf(name);assert(at>=0&&source.indexOf(mark,at+1)<0&&offset>=0);return{start:at+offset,end:at+offset+name.length};}
const bad=site('local bad = NeedNumber(Source)','Source'),copy=site('local copiedBad = NeedNumber(Copied)','Copied');
assert.deepEqual(bad,{start:583,end:589});assert.deepEqual(copy,{start:888,end:894});
add('object_red:exact_two_structural_argument_diagnostics',[{code:'param-type-mismatch',...bad},{code:'param-type-mismatch',...copy}]);
for(const index of [0,1,2,3,5])add(`object_red:${specs[index].label}:native_argument_coverage`,specs[index].dynamic?'unavailable':'native_checked');
add('object_red:explicit_five_inputs',5);assert.equal(assertions.length,101);assert.equal(new Set(assertions.map(row=>row.name)).size,101);
const contract={schema:base.schema,command:'object-red',rulesVersion:base.rulesVersion,capabilityProfile:base.capabilityProfile,inputs,scenarios:['object-versions'],assertions,
 scope:'complete finite static-field/shallow-copy observation and structural API expectations; first failure must be preserved, not accepted as business completion'};
writeFileSync(out,JSON.stringify(contract,null,2)+'\n',{flag:'wx'});console.log(JSON.stringify({path:out,sha256:hashFile(out),assertions:101}));
