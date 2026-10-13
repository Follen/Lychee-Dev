// Read-only root verification of already executed facts against fixture byte sites.
import assert from 'node:assert/strict';
import {existsSync,readFileSync,writeFileSync} from 'node:fs';
import {dirname,join,resolve} from 'node:path';
import {createHash} from 'node:crypto';
import {hashFile} from '../../tools/rust-build.mjs';

const reportPath=resolve(process.argv[2]),out=resolve(process.argv[3]);assert(!existsSync(out));
const expectedRulesVersion=process.argv[4]??'wow-binding-value-graph-v2';assert(/^[a-z0-9-]{1,128}$/.test(expectedRulesVersion));
const report=JSON.parse(readFileSync(reportPath));
const artifact=report.artifact,contract=JSON.parse(readFileSync(join(dirname(artifact.sealedBuild.binary),'contract.json')));
const checks=[];
function check(name,expected,actual){let passed=true,error;try{assert.deepEqual(actual,expected);}catch(e){passed=false;error=e.message;}checks.push({name,expected,actual,passed,error});}
const scenarios=report.frames.filter(f=>f.event==='scenario'),scenario=name=>{const row=scenarios.find(f=>f.name===name);assert(row,name);return row;};
const siteKey=site=>`${site.file}:${site.start??site.range[0]}:${site.end??site.range[1]}`;
const sameSite=(a,b)=>siteKey(a)===siteKey(b);
const bytes=file=>readFileSync(join(dirname(artifact.sealedBuild.binary),file));
const sha=text=>createHash('sha256').update(text).digest('hex');
function sealedSite(file,marker,token){const input=bytes(file),pattern=Buffer.from(marker),at=input.indexOf(pattern),offset=pattern.indexOf(Buffer.from(token));assert(at>=0&&input.indexOf(pattern,at+1)<0&&offset>=0);return{file,start:at+offset,end:at+offset+Buffer.byteLength(token)};}
const normalize=site=>({file:site.file,range:[site.start,site.end],text:bytes(site.file).subarray(site.start,site.end).toString()});
const factAt=(s,site)=>{const rows=s.graph.uses.filter(f=>sameSite(f.site,site));assert.equal(rows.length,1,siteKey(site));return rows[0];};
const callAt=(s,site)=>{const rows=s.views.calls.filter(c=>sameSite(c.binding.site,site));assert.equal(rows.length,1,siteKey(site));return rows[0];};
check('native custody and syntax actually passed',true,report.transportComplete&&report.handlesReaped&&report.syntaxComplete&&report.inputIdentityComplete);
for(const input of contract.inputs){const file=join(dirname(artifact.sealedBuild.binary),input.file);check(`sealed input ${input.file}`,input.sha256,hashFile(file));}
for(const s of scenarios){
 const input=s.graph.snapshot_inputs;
 check(`${s.name}: exact snapshot hash`,sha(s.graph.snapshot_inputs_json),s.snapshotKey);
 check(`${s.name}: snapshot object matches hash bytes`,JSON.parse(s.graph.snapshot_inputs_json),input);
 check(`${s.name}: fixed engine commit`,'08b0873962e83bb8307e9a81052e3f41578fbc78',input.engine.commit);
 check(`${s.name}: fixed engine archive`,'ae529a06b551dbb301744bea2b58ba94cb3e30340f63a8c83f9335be9086fdb7',input.engine.sourceArchiveSHA256);
 check(`${s.name}: both pinned library versions`,{emmylua_code_analysis:'0.30.0',emmylua_parser:'0.30.0'},input.engine.packages);
 check(`${s.name}: input rules version`,expectedRulesVersion,input.adapter.rulesVersion);
 check(`${s.name}: actual lock identity`,hashFile(join(resolve('.'),'Cargo.lock')),input.adapter.dependencyLockSHA256);
 check(`${s.name}: whole configuration object`,JSON.parse(input.effectiveEmmyrcJson),input.effectiveEmmyrc);
 check(`${s.name}: exact configuration hash`,sha(input.effectiveEmmyrcJson),input.effectiveEmmyrcSHA256);
 check(`${s.name}: actual Lua51 grammar`,'Lua5.1',input.effectiveEmmyrc.runtime.version);
 check(`${s.name}: original source-map identity`,'identity-original-utf8-no-transform-v1',input.sourceMapProfile);
 check(`${s.name}: execution phase participates`,s.graph.phase,input.executionPhase);
 check(`${s.name}: ordered load identities`,s.graph.script_load_occurrences,input.orderedScriptLoadOccurrences.map(row=>`${row.index}:${row.file}`));
 const loaded=[...new Set(input.orderedScriptLoadOccurrences.map(row=>row.file))].sort();
 check(`${s.name}: all loaded inputs authenticated`,loaded.map(file=>contract.inputs.find(row=>row.file===file)),input.files.map(row=>({file:row.file,bytes:row.bytes,sha256:row.sha256})));
 check(`${s.name}: explicitly closed fixture environment`,'closed-fixture-initial-globals-v1',input.initialEnvironmentProfile);
 check(`${s.name}: no implicit library/environment inputs`,[[],[]],[input.stdlibInputs,input.environmentInputs]);
 check(`${s.name}: no raw managed fallback`,0,s.views.managedFallbackCalls);
 for(const field of ['uses','values','writes','functions','calls','copies'])check(`${s.name}: ${field} IDs contiguous`,s.graph[field].map((_,i)=>i),s.graph[field].map(item=>item.id));
 for(const use of s.graph.uses){
  const v=s.views.uses[use.id];
  check(`${s.name}: use ${use.id} shared identity`,[use.id,use.proof_id,use.id,use.proof_id,use.id,use.id,use.id],[v.binding.factId,v.binding.proofId,v.type.factId,v.type.proofId,v.definitionFactId,v.referencesFactId,v.checkFactId]);
  const source=bytes(use.site.file);check(`${s.name}: use ${use.id} original range`,true,Number.isInteger(use.site.start)&&Number.isInteger(use.site.end)&&use.site.start>=0&&use.site.end>=use.site.start&&use.site.end<=source.length);
  if(use.decision.state==='bound'){
   const value=s.graph.values[use.decision.detail];check(`${s.name}: use ${use.id} value exists`,true,!!value);
   if(value?.kind.kind==='function'){
    const f=s.graph.functions[value.kind.detail];check(`${s.name}: use ${use.id} selected declaration`,f.declaration,v.binding.selectedDeclaration);check(`${s.name}: use ${use.id} selected value`,value.id,v.binding.selectedValueId);
   }
  }else check(`${s.name}: use ${use.id} no invented definition`,null,v.binding.selectedDeclaration);
 }
 // Every inverse row must be derived from this graph, retaining ordinary reads.
 const expectedRefs={},expectedCallees={};
 for(const use of s.graph.uses){const value=use.decision.state==='bound'?s.graph.values[use.decision.detail]:null;if(value?.kind.kind==='function')(expectedRefs[value.id]??=[]).push({factId:use.id,proofId:use.proof_id,site:use.site,context:use.context});}
 for(const call of s.graph.calls){const use=s.graph.uses[call.callee],value=use.decision.state==='bound'?s.graph.values[use.decision.detail]:null;if(value?.kind.kind==='function')(expectedCallees[value.id]??=[]).push({factId:use.id,proofId:use.proof_id,site:use.site,context:use.context,callId:call.id});}
 check(`${s.name}: complete graph inverse reads`,expectedRefs,s.views.inverseReferences);
 check(`${s.name}: complete graph inverse callees`,expectedCallees,s.views.inverseCalleeReferences);
 for(const call of s.graph.calls){
  const view=s.views.calls.find(v=>v.callId===call.id),result=s.graph.uses[call.resultFactId];
  check(`${s.name}: call ${call.id} result fact exists`,true,!!result);
  if(result){
   const type=s.views.uses[result.id].type;
   check(`${s.name}: call ${call.id} original result range`,call.site,result.site);
   check(`${s.name}: call ${call.id} result link`,[result.id,result.proof_id],[view.resultFactId,view.resultProofId]);
   check(`${s.name}: call ${call.id} shared result type`,type,view.resultTypeView);
   check(`${s.name}: call ${call.id} result type projection`,type.type??null,view.resultType??null);
   check(`${s.name}: call ${call.id} result coverage`,type.state,view.resultCoverage);
  }
  check(`${s.name}: call ${call.id} declared return alias`,view.declaredReturnType,view.returnType);
  check(`${s.name}: call ${call.id} argument coverage alias`,view.argumentCheckCoverage,view.checkCoverage);
 }
 for(const edge of s.graph.copies){
  const source=s.graph.writes[edge.source_field_write],destination=s.graph.writes[edge.destination_field_write];
  check(`${s.name}: copy ${edge.id} saves exact value`,source.decision,edge.selected_value);
  check(`${s.name}: copy ${edge.id} destination value`,source.decision,destination.decision);
  check(`${s.name}: copy ${edge.id} distinct source and destination cells`,true,source.cell!==destination.cell);
 }
}
const selected=[];
for(const wanted of contract.calls){
 const s=scenario(wanted.site.file==='shadow.lua'?'shadow-afterload':'mixins-afterload'),call=callAt(s,wanted.site),fact=factAt(s,wanted.site);
 check(`${wanted.id}: exact original source spelling`,wanted.site.text,bytes(wanted.site.file).subarray(...wanted.site.range).toString());
 check(`${wanted.id}: selected source declaration`,wanted.target.range,normalize(call.binding.selectedDeclaration).range);
 check(`${wanted.id}: selected declaration file`,wanted.target.file,call.binding.selectedDeclaration.file);
 check(`${wanted.id}: argument types`,wanted.argumentTypes,call.argumentTypes);
 check(`${wanted.id}: return type`,wanted.returnType,call.returnType);
 check(`${wanted.id}: expression result type`,wanted.returnType,call.resultType);
 check(`${wanted.id}: expression result coverage`,'known',call.resultCoverage);
 check(`${wanted.id}: checked`,wanted.checkCoverage,call.checkCoverage);
 check(`${wanted.id}: signature tied to selected value`,call.binding.signature,call.signature);
 check(`${wanted.id}: call shares callee proof`,[fact.id,fact.proof_id],[call.factId,call.proofId]);
 const diagnostics=call.diagnostics.filter(d=>d.code==='argument_type_mismatch');
 const expectedMismatchIndexes=wanted.argumentTypes.map((actual,i)=>actual!==call.signature.parameters[i][1]?i:null).filter(i=>i!==null);
 check(`${wanted.id}: exact wrong argument set`,expectedMismatchIndexes,diagnostics.map(d=>d.index));
 check(`${wanted.id}: no unrelated diagnostics`,diagnostics.length,call.diagnostics.length);
 check(`${wanted.id}: mismatch expected independently`,wanted.parameterMismatch,diagnostics.length>0);
 for(const diagnostic of diagnostics){const argument=s.graph.uses[call.argumentFactIds[diagnostic.index]];check(`${wanted.id}: diagnostic argument location ${diagnostic.index}`,argument.site,diagnostic.site);check(`${wanted.id}: diagnostic callee proof ${diagnostic.index}`,[fact.id,fact.proof_id],[diagnostic.factId,diagnostic.proofId]);}
 if(wanted.site.file==='shadow.lua')check(`${wanted.id}: ordinary helper`,null,call.intrinsic);
 selected.push({target:wanted.target,site:wanted.site,scenario:s.name,valueId:call.binding.selectedValueId});
}
for(const wanted of contract.deferred){
 for(const name of ['future-ab-afterload','future-ba-afterload']){
  const s=scenario(name),call=callAt(s,wanted.site),returns=s.views.returnChecks.filter(r=>r.expressionSite.file===wanted.site.file&&r.expressionSite.start===wanted.site.range[0]);assert.equal(returns.length,1);
  check(`${name}: deferred ${wanted.site.range[0]} definition`,wanted.afterLoadTarget,normalize(call.binding.selectedDeclaration));
  check(`${name}: deferred ${wanted.site.range[0]} return`,wanted.returnType,call.returnType);
  check(`${name}: deferred ${wanted.site.range[0]} return diagnostic`,wanted.returnMismatch,returns[0].mismatch);
  check(`${name}: deferred ${wanted.site.range[0]} return coverage`,'checked',returns[0].checkCoverage);
  check(`${name}: deferred ${wanted.site.range[0]} declared return`,wanted.declaredReturnType,returns[0].declaredReturnType);
  if(name==='future-ab-afterload')selected.push({target:wanted.afterLoadTarget,site:wanted.site,scenario:name,valueId:call.binding.selectedValueId});
 }
 const s=scenario('future-ab-unknown'),call=callAt(s,wanted.site),fact=factAt(s,wanted.site),ret=s.views.returnChecks.find(r=>r.expressionSite.file===wanted.site.file&&r.expressionSite.start===wanted.site.range[0]);
 check(`unknown deferred ${wanted.site.range[0]} no target`,null,call.binding.selectedDeclaration);
 check(`unknown deferred ${wanted.site.range[0]} reason`,'unknown_execution_phase',fact.decision.detail);
 check(`unknown deferred ${wanted.site.range[0]} parameter coverage`,'unavailable',call.checkCoverage);
 check(`unknown deferred ${wanted.site.range[0]} return coverage`,'unavailable',ret.checkCoverage);
 check(`unknown deferred ${wanted.site.range[0]} mismatch undetermined`,null,ret.mismatch);
}
for(const wanted of contract.reads){const s=scenario(wanted.site.file==='future-a.lua'?'future-ab-afterload':'shadow-afterload'),fact=factAt(s,wanted.site),view=s.views.uses[fact.id];check(`${wanted.id}: known absent`,'known_absent',fact.decision.state);check(`${wanted.id}: nil type`,wanted.type,view.type.type);check(`${wanted.id}: no target`,null,view.binding.selectedDeclaration);}
const ba=scenario('future-ba-afterload'),ab=scenario('future-ab-afterload'),unknown=scenario('future-ab-unknown');
const changed=factAt(ba,contract.reads[0].site);check('TOC-only change selects F2',contract.deferred[0].afterLoadTarget,normalize(ba.views.uses[changed.id].binding.selectedDeclaration));
check('TOC-only change is function','function',ba.views.uses[changed.id].type.type);
check('phase identity changes',true,ab.snapshotKey!==unknown.snapshotKey);check('TOC identity changes',true,ab.snapshotKey!==ba.snapshotKey);
const remove=(value,keys)=>Object.fromEntries(Object.entries(value).filter(([key])=>!keys.includes(key)));
check('phase changes only execution input',remove(ab.graph.snapshot_inputs,['executionPhase']),remove(unknown.graph.snapshot_inputs,['executionPhase']));
check('TOC changes only ordered input',remove(ab.graph.snapshot_inputs,['orderedScriptLoadOccurrences']),remove(ba.graph.snapshot_inputs,['orderedScriptLoadOccurrences']));
for(const expected of contract.inverseCalleeReferences){const actual=selected.filter(item=>sameSite(item.target,expected.target)).map(item=>item.site).sort((a,b)=>a.file.localeCompare(b.file)||a.range[0]-b.range[0]);check(`independent inverse ${siteKey(expected.target)}`,expected.sites,actual);}
for(const wanted of contract.unavailable){const s=scenario('unsupported-afterload'),fact=factAt(s,wanted.site);check(`unavailable ${siteKey(wanted.site)}`,'unavailable',fact.decision.state);check(`unavailable type ${siteKey(wanted.site)}`,'unavailable',s.views.uses[fact.id].type.state);const reasons={'dynamic-member':'dynamic_field_key',metatable:'metatable_behavior_unavailable','conditional-write':'conditional_write'};check(`unavailable reason ${siteKey(wanted.site)}`,reasons[wanted.reasonClass],fact.decision.detail);check(`unavailable no invented target ${siteKey(wanted.site)}`,null,s.views.uses[fact.id].binding.selectedDeclaration);}
for(const wanted of contract.returnChecks){const s=scenario('mixins-afterload'),r=s.views.returnChecks.find(r=>sameSite(r.expressionSite,wanted.site));assert(r);check('captured return actual',wanted.actual,r.actualReturnType);check('captured return declared',wanted.declared,r.declaredReturnType);check('captured return mismatch',wanted.mismatch,r.mismatch);check('captured return coverage','checked',r.checkCoverage);}
check('shadow helper no intrinsic copies',0,scenario('shadow-afterload').graph.copies.length);
check('Mixin exact two copies',2,scenario('mixins-afterload').graph.copies.length);
for(const wanted of contract.regressionCases??[]){
 const s=scenario(wanted.scenario);
 if(wanted.id==='effect-mutation'){
  const call=callAt(s,wanted.site);
  const expected={declaration:wanted.target,returnType:wanted.returnType,argumentMismatch:wanted.argumentMismatch};
  const actual={declaration:call.binding.selectedDeclaration?normalize(call.binding.selectedDeclaration):null,returnType:call.returnType,argumentMismatch:call.diagnostics.some(d=>d.code==='argument_type_mismatch')};
  check('regression effect-mutation',expected,actual);
  const mutator=s.views.calls.find(call=>sameSite(call.binding.site,sealedSite('effect-mutation.lua','mutate(t)','mutate')));assert(mutator);
  // These checks are applicable only after a real bounded summary is present.
  // Missing support remains the concrete F2 failure above, never an Unknown pass.
  if(mutator.effectSummary){
   const parameter=sealedSite('effect-mutation.lua','mutate(o)','o'),receiver=sealedSite('effect-mutation.lua','o.run = F2','o'),rhs=sealedSite('effect-mutation.lua','o.run = F2','F2'),target=sealedSite('effect-mutation.lua','o.run = F2','o.run'),argument=sealedSite('effect-mutation.lua','(t)','t');
   check('effect summary exact parameter',parameter,mutator.effectSummary.parameters[0].declaration);
   check('effect summary exact body target',target,mutator.effectSummary.writes[0].write_site);
   check('effect summary exact body receiver',receiver,mutator.effectSummary.writes[0].receiver_site);
   check('effect summary exact body RHS',rhs,mutator.effectSummary.writes[0].value.site);
   check('effect summary bounded call coverage','bounded_parameter_field_writes',mutator.effectCoverage);
   check('effect summary has one concrete write',1,mutator.effectWriteVersions.length);
   const binding=mutator.parameterBindings[0],arg=s.graph.uses[binding.argument_fact],object=s.graph.values[arg.decision.detail];
   check('effect summary actual argument location',argument,arg.site);
   check('effect summary actual argument is object','object',object.kind.kind);
   check('effect summary actual object alias',object.kind.detail,binding.object_id);
   check('effect summary actual argument value',arg.decision,binding.selected_value);
   const version=s.graph.writes[mutator.effectWriteVersions[0]],prior=s.graph.writes[version.previous];
   check('effect summary writes selected object cell',`object:${binding.object_id}:field:run`,version.cell);
   check('effect summary writes source body site',target,version.site);
   check('effect summary body replaces a previous field',version.cell,prior.cell);
   check('effect summary selected F2 reaches later call',[version.decision.detail,version.id],[call.binding.selectedValueId,call.binding.writeVersions[0]]);
   check('effect summary retains caller proof',true,version.inputs.includes(mutator.factId)&&version.inputs.includes(arg.id));
   check('effect summary retains RHS fact',true,version.inputs.some(id=>sameSite(s.graph.uses[id].site,rhs)&&s.graph.uses[id].decision.detail===call.binding.selectedValueId));
   check('effect summary retains parameter receiver fact',true,version.inputs.some(id=>sameSite(s.graph.uses[id].site,receiver)&&s.graph.uses[id].decision.detail===arg.decision.detail));
   check('effect summary single-value result projection','nil',mutator.resultType);
  }
 }else if(wanted.id==='parallel-assignment'){
  const call=callAt(s,wanted.oldCall),fresh=factAt(s,wanted.newRead);
  check('regression parallel-assignment',{oldTarget:wanted.oldTarget,oldReturnType:wanted.oldReturnType,newFieldType:wanted.newFieldType},{oldTarget:call.binding.selectedDeclaration?normalize(call.binding.selectedDeclaration):null,oldReturnType:call.returnType,newFieldType:s.views.uses[fresh.id].type.type??null});
 }else if(wanted.id==='colon-call'){
  const call=callAt(s,wanted.site),expr=factAt(s,wanted.expression),type=s.views.uses[expr.id].type;
  check('regression colon-call',{checkCoverage:wanted.checkCoverage,expressionTypeState:wanted.expressionTypeState,expressionReason:wanted.expressionReason},{checkCoverage:call.checkCoverage,expressionTypeState:type.state,expressionReason:type.reason??null});
 }else if(wanted.id==='conditional-call'){
  const call=callAt(s,wanted.site),fact=factAt(s,wanted.site);
  check('regression conditional-call',{bindingState:wanted.bindingState,typeState:wanted.typeState,checkCoverage:wanted.checkCoverage},{bindingState:fact.decision.state,typeState:s.views.uses[fact.id].type.state,checkCoverage:call.checkCoverage});
  if(s.graph.branches){
   const branch=s.graph.branches[0],condition=sealedSite('conditional-call.lua','if Flag then','Flag'),possibleSite=sealedSite('conditional-call.lua','Mixin(t, source)','Mixin(t, source)');
   check('conditional branch one bounded record',1,s.graph.branches.length);
   check('conditional branch original condition',[condition],branch.condition_sites);
   check('conditional branch condition evidence',[condition],branch.condition_facts.map(id=>s.graph.uses[id].site));
   const possible=s.views.calls.find(c=>sameSite(c.site,possibleSite));assert(possible);
   check('conditional call is possible, not executed','conditional_possible',possible.executionState);
   check('conditional call remains unavailable',['unavailable','unavailable','unavailable','unavailable'],[possible.argumentCheckCoverage,possible.invocationCoverage,possible.effectCoverage,possible.resultCoverage]);
   check('conditional branch call ownership',[possible.callId],branch.call_facts);
   check('conditional branch no executed Mixin copies',0,s.graph.copies.length);
   const footprint=branch.footprints[0],target=footprint.targets[0],write=s.graph.writes[target.unavailableWriteVersion],sourceWrite=s.graph.writes[target.sourceWriteVersions[0]],targetArg=s.graph.uses[possible.argumentFactIds[0]],sourceArg=s.graph.uses[possible.argumentFactIds[1]];
   const targetObject=s.graph.values[targetArg.decision.detail].kind.detail,sourceObject=s.graph.values[sourceArg.decision.detail].kind.detail;
   check('conditional footprint exact target field',`object:${targetObject}:field:run`,write.cell);
   check('conditional footprint exact source field',`object:${sourceObject}:field:run`,sourceWrite.cell);
   check('conditional footprint only affected target',[targetObject],footprint.targets.map(t=>t.objectId));
   check('conditional footprint retains source version',[sourceWrite.id],target.sourceWriteVersions);
   check('conditional write retains branch',branch.id,write.conditional_branch);
   check('conditional write retains condition/call/arguments',true,[possible.factId,...possible.argumentFactIds,...branch.condition_facts].every(id=>write.inputs.includes(id)));
   check('conditional unavailable field reaches later read',[write.id],fact.writes);
   check('conditional footprint no hidden coverage gap',[],branch.coverage_gaps);
  }
 }
}
const canonicalFactsSHA256=createHash('sha256').update(JSON.stringify(scenarios.map(s=>({name:s.name,graph:s.graph,views:s.views})))).digest('hex');
const failures=checks.filter(c=>!c.passed),expectedFailureNames=(artifact.expectedRedCases??['effect-mutation','parallel-assignment','colon-call','conditional-call']).map(id=>'regression '+id);
const independentlyReproducedRed=report.expectedRedVerified===true&&JSON.stringify(failures.map(f=>f.name))===JSON.stringify(expectedFailureNames);
const result={schema:'lycheedev.emmy-wow-independent.v1',passed:failures.length===0,independentlyReproducedRed,checks:checks.length,failed:failures.length,rows:checks,reportPath,reportSHA256:hashFile(reportPath),verifierSHA256:hashFile(import.meta.filename),expectedRulesVersion,build:artifact.sealedBuild,contract,canonicalFactsSHA256,scope:'independent exact fixture-site business expectations and coherent graph projections over actual native output; red reproduction does not pass business behavior; not full Source, native Mixin proof or engine selection'};
writeFileSync(out,JSON.stringify(result,null,2)+'\n');console.log(JSON.stringify({report:out,passed:result.passed,checks:result.checks,failed:result.failed}));if(!result.passed)process.exitCode=1;
