import assert from 'node:assert/strict';
import {resolve,join} from 'node:path';
import {readFileSync,writeFileSync,mkdirSync,existsSync,copyFileSync} from 'node:fs';
import {spawnSync} from 'node:child_process';
import {randomUUID} from 'node:crypto';
import {hashFile} from '../../tools/rust-build.mjs';
const root=resolve('.'),out=resolve(process.argv[2]),scenario=process.argv[3],sealed=resolve(process.argv[4]??'.tmp/rust-emmy-xml-02');
assert(['retail','line'].includes(scenario));
const identity=JSON.parse(readFileSync(join(sealed,'artifact.json'))),exe=join(out,'emmy_xml_program.exe');
const luac=join(root,'.tmp/rust-luals-suspended-release-01/payload/tool/luac/luac.exe');
assert.equal(hashFile(luac),identity.runtime.sha256);
if(!process.argv.includes('--worker')){
 assert(out.startsWith(root+'\\')&&!existsSync(out));mkdirSync(out);assert.equal(hashFile(identity.binary),identity.binarySHA256);assert.equal(hashFile(identity.pdb),identity.pdbSHA256);
 copyFileSync(identity.binary,exe);copyFileSync(identity.pdb,exe.replace(/\.exe$/,'.pdb'));
 const template=readFileSync(join(root,'tools/rust-owned-process.ps1'),'utf8'),anchor='var limits=new LIMITS();limits.basic.flags=0x2000;Set(job,9,limits);';assert.equal(template.split(anchor).length,2);
 const wrapper=join(out,'owned-boundary-2gib.ps1');writeFileSync(wrapper,template.replace(anchor,'var limits=new LIMITS();limits.basic.flags=0x2000|0x200;limits.jobMemory=new UIntPtr(2147483648UL);Set(job,9,limits);'));
 writeFileSync(join(out,'artifact.json'),JSON.stringify({...identity,binary:exe,pdb:exe.replace(/\.exe$/,'.pdb'),scenario,runnerSHA256:hashFile(import.meta.filename),wrapperSHA256:hashFile(wrapper)},null,2));writeFileSync(join(out,'runner-snapshot.mjs'),readFileSync(import.meta.filename));
 const base=join(out,'process-'+randomUUID()),spec={command:process.execPath,args:[import.meta.filename,out,scenario,sealed,'--worker'],cwd:root,timeoutMs:scenario==='retail'?90000:15000,orphanMs:5000,cargoBuild:false,stdout:base+'.stdout.txt',stderr:base+'.stderr.txt',receipt:base+'.receipt.json'};
 writeFileSync(base+'.request.json',JSON.stringify(spec,null,2));const run=spawnSync('powershell.exe',['-NoProfile','-NonInteractive','-File',wrapper,'-Request',base+'.request.json'],{cwd:root,windowsHide:true,timeout:spec.timeoutMs+35000,maxBuffer:1<<20});
 let proof,worker,error;try{proof=JSON.parse(readFileSync(spec.receipt));worker=JSON.parse(readFileSync(join(out,'worker.json')));}catch(e){error=String(e);}
 const reaped=run.status===0&&proof?.rootReaped&&proof.treeReaped&&proof.handlesClosed&&proof.activeProcesses===0;
 const report={schema:'lycheedev.xml-boundary-owned.v1',scenario,passed:reaped&&!error&&proof.exitCode===0&&worker?.passed===true,worker,proof,error,handlesReaped:reaped,scope:scenario==='retail'?'all original manifest bytes verified; all XML attempted; extraction and syntax facts only, no implicit parameters or semantic bindings':'actual Lua5.1 entity-CR diagnostic mapped through the existing extractor method; no source execution'};
 assert.equal(hashFile(exe),identity.binarySHA256);assert.equal(hashFile(luac),identity.runtime.sha256);
 writeFileSync(join(out,'report.json'),JSON.stringify(report,null,2));console.log(JSON.stringify({report:join(out,'report.json'),passed:report.passed,handlesReaped:reaped,summary:worker?.summary,error}));if(!report.passed)process.exitCode=1;
}else{
 function run(args,input=null,command=exe,timeout=5000){const result=spawnSync(command,args,{cwd:root,windowsHide:true,input,timeout,maxBuffer:8<<20});return {exit:result.status,error:result.error?.message,stdout:result.stdout?.toString(),stderr:result.stderr?.toString()};}
 let report;
 if(scenario==='retail'){
  const result=run(['retail',join(root,'.tmp/rust-source-independent-retail-01/input/manifest.json'),join(root,'.tmp/gob01/source/v1/worktrees/wow-ui-source/31c7f7b9cc79e56c986b365c06a6afbcf3c9177b')],null,exe,85000);
  writeFileSync(join(out,'retail.stdout.json'),result.stdout??'');writeFileSync(join(out,'retail.stderr.txt'),result.stderr??'');let value,error;try{value=JSON.parse(result.stdout);}catch(e){error=String(e);}
  const diagnostics=(value?.files??[]).flatMap(file=>(file.summary?.profileDiagnostics??[]).map(d=>({path:file.path,...d})));
  const commentator=value?.files?.find(file=>file.path==='Interface/AddOns/Blizzard_Commentator/Bindings.xml');
  const counts=value?.counts;
  const fullExtraction=value?.extractionState==='passed'&&value.extractionComplete===true&&value.documentsVerified===4036&&value.luaFilesVerified===2551&&value.luaBytesVerified===23354176&&value.xmlFilesAttempted===1091&&value.xmlFilesExtracted===1091&&value.xmlFilesEmmyParsed===1091&&value.xmlFilesUnsupported===0&&value.xmlFilesFailed===0&&value.xmlFilesExcluded===0&&value.xmlFilesNotRun===0;
  const bindingCountsExact=counts?.bindingFiles===8&&counts.bindingRawBytes===184481&&counts.bindingDeclarations===1033&&counts.bindingScripts===1025&&counts.modifiedClicks===77;
  const diagnosticExact=diagnostics.length===1&&diagnostics[0].path===commentator?.path&&diagnostics[0].code==='binding_root_text'&&diagnostics[0].severity==='warning'&&diagnostics[0].validationState==='unresolved'&&diagnostics[0].nativeRuntimeBehavior==='unknown'&&diagnostics[0].raw?.start===1132&&diagnostics[0].raw?.end===1133&&commentator?.rawSHA256==='0044e6a9881e3a151b4def1e746e930d80352cfc48752c75fa19bd7378bb7b2c'&&commentator.summary.unexpectedProfileText.some(t=>t.sourceText==='>\n\n\t'&&t.raw.start===1132&&t.raw.end===1136);
  const profileStatusExact=value?.state==='extracted_with_profile_diagnostics'&&value.profileValidationPassed===false&&value.profileValidationState==='unresolved';
  report={passed:!error&&!result.error&&result.exit===0&&fullExtraction&&bindingCountsExact&&diagnosticExact&&profileStatusExact,result,value,error,fullExtraction,bindingCountsExact,diagnosticExact,profileStatusExact,profileValidationPassed:value?.profileValidationPassed,diagnostics,summary:value?{state:value.state,documentsVerified:value.documentsVerified,xmlFilesAttempted:value.xmlFilesAttempted,xmlFilesExtracted:value.xmlFilesExtracted,xmlFilesFailed:value.xmlFilesFailed,counts,profileValidationPassed:value.profileValidationPassed}:null};
 }else{
  const records=[];
  for(const name of ['xml-entity-cr-top.xml','xml-entity-cr-handler.xml']){
   const file=join(root,'tests/rust-source-engine',name),extraction=run(['extract',file]);const value=JSON.parse(extraction.stdout),script=value.scripts[0];
   const input=Buffer.from(script.compiler.text);writeFileSync(join(out,name+'.stdin.lua'),input);
   const compiler=run(['-p','-'],input,luac),line=Number(compiler.stderr?.match(/stdin:(\d+):/)?.[1]);assert(line>0);
   const expectedLine=name.includes('top')?2:3;const mapped=run(['line',file,'0',String(line)]);writeFileSync(join(out,name+'.mapping.json'),mapped.stdout??'');const mapping=JSON.parse(mapped.stdout);
   const invalidText=Buffer.from('local = 2'),original=readFileSync(file),start=original.indexOf(invalidText),end=start+invalidText.length;assert(start>=0);
   const spans=mapping.mapping?.sourceMap?.segments??[];
   const containsRaw=spans.some(segment=>segment.raw&&segment.raw.start<=start&&segment.raw.end>=end);
   records.push({name,inputSHA256:hashFile(file),compiler,expectedCompilerLine:expectedLine,actualCompilerLine:line,mapped,mapping,expectedOriginalRange:{start,end},passed:!extraction.error&&extraction.exit===0&&!compiler.error&&compiler.exit===1&&line===expectedLine&&!mapped.error&&mapped.exit===0&&mapping.mapping?.columnKnown===false&&containsRaw});
  }
  report={records,passed:records.every(r=>r.passed),summary:{cases:records.length,failed:records.filter(r=>!r.passed).length}};
 }
 writeFileSync(join(out,'worker.json'),JSON.stringify(report,null,2));if(!report.passed)process.exitCode=1;
}
