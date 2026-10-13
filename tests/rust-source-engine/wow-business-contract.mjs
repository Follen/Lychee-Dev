// Independent expectations, expressed from fixed fixture bytes rather than the
// analyzer's symbol resolver or its own assertion names.
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {join} from 'node:path';
import {createHash} from 'node:crypto';

export function fixedBusinessContract(root,{regressions=false}={}){
 const inputs=new Map();
 const files=['functions.lua','future-a.lua','future-b.lua','mixins.lua','profile.lua','shadow.lua','unsupported.lua'];
 if(regressions)files.push('effect-mutation.lua','parallel-assignment.lua','colon-call.lua','conditional-call.lua');
 for(const file of files){
  const bytes=readFileSync(join(root,'tests/rust-source-engine/wow',file));
  inputs.set(file,{file,bytes:bytes.length,sha256:createHash('sha256').update(bytes).digest('hex')});
 }
 function site(file,needle,token=needle){
  const path=join(root,'tests/rust-source-engine/wow',file);
  const bytes=readFileSync(path),marker=Buffer.from(needle),name=Buffer.from(token);
  const at=bytes.indexOf(marker);assert(at>=0&&bytes.indexOf(marker,at+1)<0,`unique fixture marker ${file}: ${needle}`);
  const delta=marker.indexOf(name);assert(delta>=0);
  inputs.set(file,{file,bytes:bytes.length,sha256:createHash('sha256').update(bytes).digest('hex')});
  return {file,range:[at+delta,at+delta+name.length],text:token};
 }
 const f1=site('functions.lua','function F1(n)','F1');
 const f2=site('functions.lua','function F2(s)','F2');
 const shadow=site('shadow.lua','local function Mixin(left, right)','Mixin');
 const call=(id,file,marker,token,target,argType,returnType,parameterMismatch)=>({id,site:site(file,marker,token),target,argumentTypes:[argType],returnType,parameterMismatch,checkCoverage:'checked'});
 const calls=[
  call('first-copy-number','mixins.lua','local first = x.run(7)','x.run',f1,'number','number',false),
  call('first-copy-wrong-argument','mixins.lua','local wrongFirst = x.run("bad")','x.run',f1,'string','number',true),
  call('second-copy-string','mixins.lua','local second = x.run("ok")','x.run',f2,'string','string',false),
  call('second-copy-wrong-argument','mixins.lua','local wrongSecond = x.run(7)','x.run',f2,'number','string',true),
  call('captured-function-identity','mixins.lua','return copied(7)','copied',f1,'number','number',false),
  {...call('shadowed-helper-ordinary-call','shadow.lua','local plain = Mixin(1, 2)','Mixin',shadow,'number','number',false),argumentTypes:['number','number']},
  {...call('shadowed-helper-wrong-arguments','shadow.lua','local wrong = Mixin(target, source)','Mixin',shadow,'table','number',true),argumentTypes:['table','table']},
 ];
 const deferred=[
  {site:site('future-a.lua','function Deferred()\n    return Later("ok")','Later'),afterLoadTarget:f2,returnType:'string',declaredReturnType:'string',returnMismatch:false},
  {site:site('future-a.lua','function WrongDeferredReturn()\n    return Later("ok")','Later'),afterLoadTarget:f2,returnType:'string',declaredReturnType:'number',returnMismatch:true},
 ];
 for(const item of deferred)item.unknownPhase={target:null,type:'unavailable',checkCoverage:'unavailable',parameterMismatch:'undetermined',returnMismatch:'undetermined',reasonClass:'unknown-execution-phase'};
 const reads=[
  {id:'no-future-binding',site:site('future-a.lua','local before = Later','Later'),target:null,type:'nil',basis:'fixture declares closed initial environment'},
  {id:'shadow-does-not-copy',site:site('shadow.lua','local absent = target.run','target.run'),target:null,type:'nil',basis:'ordinary helper must not create intrinsic copy edge'},
 ];
 const unavailable=[
  {site:site('unsupported.lua','local dynamic = object[key]','object[key]'),reasonClass:'dynamic-member'},
  {site:site('unsupported.lua','local uncertain = object.run','object.run'),reasonClass:'metatable'},
  {site:site('unsupported.lua','local branchValue = conditional.run','conditional.run'),reasonClass:'conditional-write'},
 ];
 const returnChecks=[{site:site('mixins.lua','return copied(7)','copied(7)'),declared:'string',actual:'number',mismatch:true}];
 const sameSite=(left,right)=>left.file===right.file&&JSON.stringify(left.range)===JSON.stringify(right.range);
 const inverseCalleeReferences=[f1,f2,shadow].map(target=>({target,context:'explicit_after_load',sites:[...calls.filter(call=>sameSite(call.target,target)).map(call=>call.site),...deferred.filter(item=>sameSite(item.afterLoadTarget,target)).map(item=>item.site)].sort((a,b)=>a.file.localeCompare(b.file)||a.range[0]-b.range[0]),scope:'exact known callee set for the seven calls and two deferred calls in this independent contract; ordinary value reads remain separately represented'}));
 const identityChecks=[{change:'execution-phase',sourceBytes:'equal',fixedSourceAndLoadInput:'equal',graphIdentity:'different',affectedSites:deferred.map(item=>item.site)},{change:'swap-future-a-and-future-b-load-order',sourceBytes:'equal',fixedSourceAndLoadInput:'different',graphIdentity:'different',requiredChangedFact:{site:reads[0].site,originalTarget:null,swappedTarget:f2,swappedType:'function'},preservedAfterLoadFacts:deferred.map(item=>({site:item.site,target:f2,returnType:'string'}))}];
 const regressionCases=regressions?[
  {id:'effect-mutation',scenario:'effect-mutation-afterload',site:site('effect-mutation.lua','local after = t.run(7)','t.run'),target:f2,returnType:'string',argumentMismatch:true},
  {id:'parallel-assignment',scenario:'parallel-assignment-afterload',oldCall:site('parallel-assignment.lua','local oldCall = old.run(7)','old.run'),oldTarget:f1,oldReturnType:'number',newRead:site('parallel-assignment.lua','local newRead = a.run','a.run'),newFieldType:'nil'},
  {id:'colon-call',scenario:'colon-call-afterload',site:site('colon-call.lua','local got = t:run(7)','t:run'),expression:site('colon-call.lua','local got = t:run(7)','t:run(7)'),checkCoverage:'unavailable',expressionTypeState:'unavailable',expressionReason:'colon_call_not_supported'},
  {id:'conditional-call',scenario:'conditional-call-afterload',site:site('conditional-call.lua','local after = t.run(7)','t.run'),bindingState:'unavailable',typeState:'unavailable',checkCoverage:'unavailable',conditionBasis:'unknown chunk vararg, not an absent closed-environment global'},
 ]:[];
 return {schema:'lycheedev.wow-business-contract.v1',inputs:[...inputs.values()].sort((a,b)=>a.file.localeCompare(b.file)),requiredPartialOrder:[['profile.lua','mixins.lua'],['profile.lua','unsupported.lua'],['functions.lua','future-a.lua'],['future-a.lua','future-b.lua'],['functions.lua','mixins.lua'],['functions.lua','shadow.lua'],['functions.lua','unsupported.lua']],executionContexts:['unknown_deferred','explicit_after_load'],calls,deferred,reads,unavailable,returnChecks,inverseCalleeReferences,identityChecks,regressionCases,invariants:['all views derive from same selected value and call proof','unknown execution phase does not default to after-load','captured value remains F1 after second copy','local helper has zero intrinsic copy edges','changing execution phase or TOC changes scenario identity','inverse references are exact site partitions, not spelling matches'],scope:'explicit primitive signatures and fixed closed fixture load/value graph; not full Source or native Mixin implementation proof'};
}
