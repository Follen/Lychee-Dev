import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {existsSync,readFileSync,writeFileSync} from 'node:fs';
import {dirname,join,resolve} from 'node:path';
import {hashFile} from '../../tools/rust-build.mjs';
const reportPath=resolve(process.argv[2]),out=resolve(process.argv[3]);assert(!existsSync(out));
const report=JSON.parse(readFileSync(reportPath)),checks=[];
function check(name,expected,actual){let passed=true,error;try{assert.deepEqual(actual,expected);}catch(e){passed=false;error=e.message;}checks.push({name,expected,actual,passed,error});}
const queries=report.frames.filter(row=>row.event==='native_query'),diagnostics=report.frames.filter(row=>row.event==='native_diagnostics'),scenarios=report.frames.filter(row=>row.event==='scenario');
check('one exact query',1,queries.length);check('one exact diagnostic set',1,diagnostics.length);check('one exact scenario',1,scenarios.length);
const query=queries[0],diagnostic=diagnostics[0],scenario=scenarios[0],graph=scenario.graph;
const fixture=report.artifact.fixtures.find(row=>row.file==='coverage-unknown-argument.lua');
const source=readFileSync(join(dirname(report.artifact.sealedBuild.binary),'inputs/tests/rust-source-engine/wow',fixture.diskFile));
check('fixed source length',127,source.length);check('fixed source SHA','e406a9390c3cb083f3e91ced5bef97ec85a2938c0b1eb3c7289a8ba85f5f424f',createHash('sha256').update(source).digest('hex'));
check('actual sealed syntax and custody',true,report.transportComplete&&report.syntaxComplete&&report.handlesReaped);
check('exact call spelling','Replacement(object[key])',source.subarray(98,122).toString());check('exact argument spelling','object[key]',source.subarray(110,121).toString());
check('query source range',{file:fixture.file,start:98,end:122},query.sourceSite);
check('actual native result','String',query.nativeLuaType);check('result coverage is independent','native_projected',query.typeCoverage.state);
check('result coverage operation','call_result',query.typeCoverage.operation);check('known result reason',null,query.typeCoverage.reason);
check('native declaration has one candidate',1,query.nativeCandidates.length);check('fixed native declaration return','String',query.nativeCandidates[0].returnNativeLuaType);
check('fixed native declaration has no templates',[],query.nativeCandidates[0].genericParameters);check('unknown argument binding','unavailable',query.argumentDecision.state);
const call=graph.calls[query.callFactId],callee=graph.uses[call.callee],result=graph.uses[call.resultFactId],argument=graph.uses[call.arguments[0]];
check('call graph range',query.sourceSite,call.site);check('graph result identity',result.id,query.resultFactId);check('graph argument identity',argument.id,query.argumentFactId);
check('callee proof',callee.proof_id,query.calleeProofId);check('result proof',result.proof_id,query.resultProofId);check('argument proof',argument.proof_id,query.argumentProofId);
check('native projection owns result proof',result.proof_id,query.typeCoverage.proofId);check('dependency proof pair',[callee.proof_id,result.proof_id],query.typeCoverage.proofIds);
check('dynamic graph argument remains unavailable','unavailable',argument.decision.state);
const value=graph.values[callee.decision.detail],selected=graph.functions[value.kind.detail];
check('bound callee source function','Replacement',selected.name);check('exact selected closure',selected.closure,query.calleeDecision.selectedValue.closure);
check('exact result-vs-argument diagnostic',[{code:'return-type-mismatch',sourceSite:{file:fixture.file,start:98,end:122}}],diagnostic.diagnostics.map(row=>({code:row.code,sourceSite:row.sourceSite})));
for(const [operation,state] of [['call_result','native_projected'],['argument_check','unavailable'],['return_check','native_checked']]){
 const rows=diagnostic.coverage.filter(row=>row.operation===operation&&row.site.file===fixture.file&&row.site.start===98&&row.site.end===122);
 check(`${operation} exactly one consistent coverage`,1,rows.length);check(`${operation} status`,state,rows[0]?.state);check(`${operation} result proof`,result.proof_id,rows[0]?.proofId);
}
check('same query/diagnostic context',scenario.snapshotKey,diagnostic.snapshotKey);check('same query snapshot',scenario.snapshotKey,query.snapshotKey);check('same model context',scenario.snapshotKey,query.contextIdentity);
check('exact snapshot digest',createHash('sha256').update(graph.snapshot_inputs_json).digest('hex'),scenario.snapshotKey);
check('coverage scope identity','fixed-return-unknown-argument-v1',graph.snapshot_inputs.validationScope);
check('snapshot contains all five inputs',report.artifact.fixtures.map(row=>({file:row.file,bytes:row.bytes,sha256:row.sha256})),graph.snapshot_inputs.files);
const failed=checks.filter(row=>!row.passed),resultReport={schema:'lycheedev.emmy-coverage-independent.v1',passed:failed.length===0,checks,failed:failed.length,sourceReportSHA256:hashFile(reportPath),scope:'fixed return and unknown-argument projection, native diagnostic and graph proof consistency; not general effects/flow or full Source'};
writeFileSync(out,JSON.stringify(resultReport,null,2)+'\n');console.log(JSON.stringify({report:out,passed:resultReport.passed,checks:checks.length,failed:failed.length}));if(!resultReport.passed)process.exitCode=1;
