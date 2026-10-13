// Run the normal-feature query probe against the existing fixed query contract.
import assert from 'node:assert/strict';
import {copyFileSync, existsSync, mkdirSync, readFileSync, statSync, writeFileSync} from 'node:fs';
import {dirname, join, resolve} from 'node:path';
import {hashFile} from '../../tools/rust-build.mjs';
import {ownedSpawnSync} from '../../tools/rust-owned-process.mjs';
import {queryProbeContract, sourcePoint} from './query-facade-contract.mjs';

const root=resolve('.'), [producer, output, luacArg]=process.argv.slice(2);
assert(producer&&output,'usage: query-current-owned.mjs <producer-id> <new-output> [luac.exe]');
const out=resolve(output), evidence=join(root,'.tmp/rust-disk/evidence',producer);
assert(!existsSync(out));
const receipt=JSON.parse(readFileSync(join(evidence,'receipt.json')));
assert.equal(receipt.state,'retired');assert.equal(receipt.handlesReaped,true);
assert(!receipt.command.includes('--no-default-features'));
const artifactLog=join(evidence,'stdout.bin');
assert.equal(hashFile(artifactLog),receipt.sealedLogs.find(row=>resolve(row.path)===artifactLog).sha256);
const artifacts=readFileSync(artifactLog,'utf8').trim().split(/\r?\n/).flatMap(line=>{try{return[JSON.parse(line)]}catch{return[]}});
const artifact=artifacts.find(row=>row.reason==='compiler-artifact'&&row.target.name==='emmy_source_query_probe'&&!row.fresh);
assert(artifact,'producer must have emitted the actual fresh query probe');
const binary=artifact.filenames.find(path=>path.endsWith('.exe')),pdb=artifact.filenames.find(path=>path.endsWith('.pdb'));
const binarySHA256=hashFile(binary),pdbSHA256=hashFile(pdb),contract=queryProbeContract(root);
const extraPath='tests/rust-source-engine/wow/native/query-vararg-assignment.lua';
const extraInput={file:'query-vararg-assignment.lua',diskPath:extraPath,bytes:statSync(join(root,extraPath)).size,sha256:hashFile(join(root,extraPath))};
const emptyPath='tests/rust-source-engine/wow/native/query-empty-function.lua';
const emptyInput={file:'query-empty-function.lua',diskPath:emptyPath,bytes:statSync(join(root,emptyPath)).size,sha256:hashFile(join(root,emptyPath))};
const extraInputs=[extraInput,emptyInput],isExtra=file=>extraInputs.some(input=>input.file===file);
const inputs=[...contract.inputs,...extraInputs];
const luac=resolve(luacArg??'.tmp/rust-luals-suspended-release-01/payload/tool/luac/luac.exe');
const runtime=JSON.parse(readFileSync(join(root,'release/tools/luac.json'))).runtime;
assert.equal(runtime.version,'5.1.5');assert.equal(hashFile(luac),runtime.sha256);assert.equal(statSync(luac).size,runtime.bytes);
mkdirSync(out,{recursive:true});
for(const input of inputs){const source=join(root,input.diskPath),destination=join(out,'inputs',input.diskPath);assert.equal(hashFile(source),input.sha256);mkdirSync(dirname(destination),{recursive:true});copyFileSync(source,destination);}
writeFileSync(join(out,'fixed-contract.json'),JSON.stringify(contract,null,2));
const execution=ownedSpawnSync(binary,[],{cwd:root,evidence:out,timeout:30000,orphanMs:5000});
writeFileSync(join(out,'analysis.stdout.jsonl'),execution.stdout);writeFileSync(join(out,'analysis.stderr.txt'),execution.stderr);
let frames=[],error;
try{assert(execution.stdout.length>0&&execution.stdout.length<=2**21&&execution.stdout.at(-1)===10);frames=execution.stdout.toString('utf8').trimEnd().split('\n').map(JSON.parse);}catch(failure){error=String(failure);}
const syntax=[];
for(const input of inputs){const run=ownedSpawnSync(luac,['-p',join(out,'inputs',input.diskPath)],{cwd:root,evidence:out,timeout:5000,orphanMs:5000});syntax.push({...input,exit:run.status,error:run.error?.message,handlesReaped:run.handlesReaped,proof:run.proof,sourceExecuted:false,passed:run.status===0&&!run.error&&run.handlesReaped});}
const checks=[],additionalChecks=[];
function check(list,name,expected,actual){let passed=true,message;try{assert.deepEqual(actual,expected)}catch(failure){passed=false;message=failure.message}list.push({name,expected,actual,passed,error:message});}
const original=(name,expected,actual)=>check(checks,name,expected,actual);
const additions=(name,expected,actual)=>check(additionalChecks,name,expected,actual);
original('native owner/process/handles terminal',true,execution.handlesReaped);
original('complete clean execution',0,execution.status);
original('matching EXE',binarySHA256,hashFile(binary));original('matching PDB',pdbSHA256,hashFile(pdb));
// These two owner checks replace the retired frozen-tree owner checks; all
// original business assertions and independent graph checks remain intact.
original('actual producer source identity',true,/^[a-f0-9]{64}$/.test(receipt.inputHashes.sourceTree));
original('actual normal-feature producer artifact',binary,artifact.executable);
const assertions=frames.filter(row=>row.event==='assertion');
const prefix='query:syntax_paging:',varargPrefix='query:vararg_assignment:',emptyPrefix='query:empty_function:';
const isAdded=row=>row.name.startsWith(prefix)||row.name.startsWith(varargPrefix)||row.name.startsWith(emptyPrefix);
const originalAssertions=assertions.filter(row=>!isAdded(row));
original('all exact external assertion names',contract.assertions.map(row=>row.name),originalAssertions.map(row=>row.name));
for(let i=0;i<contract.assertions.length;i++){const expected=contract.assertions[i],actual=originalAssertions[i];if(expected.expected?.relation)original(expected.name,actual?.expected,actual?.actual);else{original(expected.name+' fixed expected',expected.expected,actual?.expected);original(expected.name,expected.expected,actual?.actual);}}
const identities=frames.filter(row=>row.event==='input_identity');
original('all eight exact source byte identities',contract.inputs.map(({file,bytes,sha256})=>({file,bytes,sha256})).sort((a,b)=>a.file.localeCompare(b.file)),identities.filter(row=>!isExtra(row.file)).map(({file,bytes,sha256})=>({file,bytes,sha256})).sort((a,b)=>a.file.localeCompare(b.file)));
original('compiled patch',receipt.inputHashes.emmyPatch,frames.find(row=>row.event==='build_identity')?.compiledPatchSHA256);
const originalSyntax=syntax.filter(row=>!isExtra(row.file));
original('actual Lua51 compiler and no source execution',true,originalSyntax.length===contract.inputs.length&&originalSyntax.every(row=>row.passed&&row.sourceExecuted===false));
const paging=frames.find(row=>row.event==='paging');
original('independent full page equals all continued rows',paging?.full.rows.map(row=>row.id),paging?.joined.map(row=>row.id));
original('independent page union has no duplicates',paging?.joined.length,new Set(paging?.joined.map(row=>row.id)).size);
const added=assertions.filter(isAdded);
const pagingNames=['references_cover_both_files','references_full_page_complete','references_exact_ordered_page_union','calls_cover_both_files','calls_full_page_complete','calls_exact_ordered_page_union'].map(name=>prefix+name);
const varargNames=[...['first','second','third','shifted_second','shifted_third','assigned_first','assigned_second','assigned_third'].map(name=>varargPrefix+name+':unknown_not_nil'),varargPrefix+'fixed_prefix_retained',varargPrefix+'ReturnTail:open_sequence_boundary',varargPrefix+'CallTail:open_sequence_boundary'];
const emptyChecks=[
  ...['named','anonymous','method'].map(name=>[emptyPrefix+name+':normal_zero_return',{returns:0,paths:1,continue:true,effects_complete:true,writes:0,body_coverage:'return_expressions_only'}]),
  [emptyPrefix+'scalar_and_assignment_nil',Array(9).fill(true)],
  [emptyPrefix+'non_tail_following_argument',sourcePoint(readFileSync(join(root,emptyPath),'utf8'),emptyInput.file,'local scalarArgA, scalarArgB = sink(one(7), 9)','9')],
  [emptyPrefix+'tail_and_non_tail_argument_arity',[0,2]],
  [emptyPrefix+'tail_return_zero_values',{returns:0,explicit_empty_return:true}],
  [emptyPrefix+'explicit_entry_zero_values',{returns:0,effects_complete:true,writes:0}],
];
const addedNames=[...pagingNames,...varargNames,...emptyChecks.map(([name])=>name)];
additions('all added exact assertion names',addedNames,added.map(row=>row.name));
for(const name of pagingNames){const row=added.find(row=>row.name===name);const expected=name.endsWith('cover_both_files')?['query-captures.lua','query-roles.lua']:name.endsWith('page_complete')?false:row?.expected;additions(name,expected,row?.actual);}
for(const name of varargNames){const row=added.find(row=>row.name===name);const expected=name.endsWith('unknown_not_nil')?'variadic_argument_slot_unavailable':name.endsWith('open_sequence_boundary')?true:sourcePoint(readFileSync(join(root,extraPath),'utf8'),extraInput.file,'local shiftedFirst, shiftedSecond, shiftedThird = 7, ...','7');additions(name,expected,row?.actual);}
additions('extra vararg fixture byte identity',[{file:extraInput.file,bytes:extraInput.bytes,sha256:extraInput.sha256}],identities.filter(row=>row.file===extraInput.file).map(({file,bytes,sha256})=>({file,bytes,sha256})));
additions('extra vararg fixture Lua51 syntax only',true,syntax.some(row=>row.file===extraInput.file&&row.passed&&row.sourceExecuted===false));
for(const [name,expected] of emptyChecks)additions(name,expected,added.find(row=>row.name===name)?.actual);
additions('extra empty function fixture byte identity',[{file:emptyInput.file,bytes:emptyInput.bytes,sha256:emptyInput.sha256}],identities.filter(row=>row.file===emptyInput.file).map(({file,bytes,sha256})=>({file,bytes,sha256})));
additions('extra empty function fixture Lua51 syntax only',true,syntax.some(row=>row.file===emptyInput.file&&row.passed&&row.sourceExecuted===false));
original('honest complete terminal',[{complete:true,error:null,checked:contract.assertions.length+addedNames.length,failed:0,fullSourceAcceptance:false}],frames.filter(row=>row.event==='end').map(({complete,error,checked,failed,fullSourceAcceptance})=>({complete,error,checked,failed,fullSourceAcceptance})));
assert.equal(checks.length,178,'all original checks retained');
const a={schema:'lycheedev.source-query-artifact.v1',binary,binarySHA256,pdb,pdbSHA256,contract,luac,runtime,producer,buildReceipt:join(evidence,'receipt.json'),sourceTree:receipt.inputHashes.sourceTree,
  // The unchanged verifier reads only dirname(sealedBuild.binary)/inputs.
  // This explicit anchor is not a copied or runnable build artifact.
  sealedBuild:{binary:join(out,'verifier-input-anchor'),inputAnchorOnly:true,actualBinary:binary},extraInputs,fixtureCopyScope:'eight original fixed Lua inputs, one vararg fixture and one empty-function fixture only; no source tree or binary copy'};
const report={schema:'lycheedev.source-query-owned.v1',passed:!error&&!execution.error&&checks.every(row=>row.passed)&&additionalChecks.every(row=>row.passed),artifact:a,process:execution.proof,handlesReaped:execution.handlesReaped,checks,additionalChecks,frames,worker:{analysis:{exit:execution.status,error:execution.error?.message},syntax},error,fullSourceAcceptance:false};
const reportPath=join(out,'report.json');writeFileSync(reportPath,JSON.stringify(report,null,2));
const independentPath=join(out,'independent.json');
const independent=ownedSpawnSync(process.execPath,[join(root,'tests/rust-source-engine/query-facade-independent.mjs'),reportPath,independentPath],{cwd:root,evidence:out,timeout:30000,orphanMs:5000});
writeFileSync(join(out,'independent.stdout.txt'),independent.stdout);writeFileSync(join(out,'independent.stderr.txt'),independent.stderr);
const verification=existsSync(independentPath)?JSON.parse(readFileSync(independentPath)):null;
const summary={report:reportPath,passed:report.passed&&independent.status===0&&!independent.error&&independent.handlesReaped&&verification?.passed===true,originalAssertions:originalAssertions.length,originalChecks:checks.length,addedAssertions:added.length,addedChecks:additionalChecks.length,terminal:frames.find(row=>row.event==='end'),independentChecks:verification?.checks.length,handlesReaped:execution.handlesReaped&&syntax.every(row=>row.handlesReaped)&&independent.handlesReaped,failures:[...checks,...additionalChecks].filter(row=>!row.passed).map(row=>row.name),independentProcess:independent.proof};
writeFileSync(join(out,'summary.json'),JSON.stringify(summary,null,2));console.log(JSON.stringify(summary));if(!summary.passed)process.exitCode=1;
