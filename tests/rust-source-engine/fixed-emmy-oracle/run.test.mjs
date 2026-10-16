import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdtempSync,rmSync,writeFileSync,readFileSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {oracleCargoArgs,readTraces,traceDifferences,validateFrames} from './run.mjs';
const target='case_native_oracle',name='same_complete_contract';
const header={kind:'header',schema:'lycheedev.emmy-fixed-oracle.v1',role:'reference',target,test:name};
const row={kind:'row',ordinal:0,label:'query:all_fields',value:'BindingCoverage { state: "bound", proof: "real" }'};
const frames=()=>[header,row,{kind:'end',rows:1,complete:true}];
const context={target,test:name,role:'reference'};
test('all exact historical fields survive the trace parser',()=>{
  assert.deepEqual(validateFrames(frames(),context),[row]);
});
test('missing failed or truncated historical terminal cannot become expected',()=>{
  for(const broken of [[],[header],[header,row],[header,row,{kind:'end',rows:1,complete:false}]])
    assert.throws(()=>validateFrames(broken,context));
});
test('candidate output cannot masquerade as the independent reference',()=>{
  const bad=frames();bad[0]={...header,role:'candidate'};
  assert.throws(()=>validateFrames(bad,context),/oracle.header/);
});
test('test identities omitted fields surplus fields and row reorder all fail',()=>{
  for(const changed of [{...header,test:'another_test'}, {...header,target:'another_target'}])
    assert.throws(()=>validateFrames([changed,...frames().slice(1)],context));
  for(const changed of [{...row,ordinal:1},{kind:'row',ordinal:0,label:'x'},{...row,ignored:true}])
    assert.throws(()=>validateFrames([header,changed,frames()[2]],context));
  assert.throws(()=>validateFrames([header,{kind:'end',rows:0,complete:true}],context),/oracle.empty_test/);
});
test('trace directory requires every original test and refuses surplus outputs',t=>{
  const root=mkdtempSync(join(tmpdir(),'emmy-fixed-trace-'));
  t.after(()=>rmSync(root,{recursive:true,force:true}));
  const targets=[{target,tests:[name]}];
  assert.throws(()=>readTraces(root,targets,'reference'),/oracle.test_inventory/);
  const path=join(root,`${target}--${name}.jsonl`);
  writeFileSync(path,frames().map(f=>JSON.stringify(f)).join('\n')+'\n');
  assert.deepEqual(readTraces(root,targets,'reference')[0].rows,[row]);
  writeFileSync(join(root,'unexpected.jsonl'),'');
  assert.throws(()=>readTraces(root,targets,'reference'),/oracle.test_inventory/);
});
test('a final line lacking its terminator is a hard truncated evidence failure',t=>{
  const root=mkdtempSync(join(tmpdir(),'emmy-fixed-trace-'));
  t.after(()=>rmSync(root,{recursive:true,force:true}));
  writeFileSync(join(root,`${target}--${name}.jsonl`),frames().map(f=>JSON.stringify(f)).join('\n'));
  assert.throws(()=>readTraces(root,[{target,tests:[name]}],'reference'),/oracle.truncated_line/);
});
test('trace values have no state field or type normalization allowlist',()=>{
  const left=validateFrames(frames(),context);
  const changed={...row,value:row.value.replace('bound','owned_checked')};
  const right=validateFrames([header,changed,frames()[2]],context);
  assert.notDeepEqual(left,right);
});
test('fixed source archive and reference preserve the original 36 test identities',()=>{
  const inputs=JSON.parse(readFileSync(new URL('./inputs.json',import.meta.url)));
  assert.equal(inputs.referenceCommit,'cd87118123fcba3d26f8ee161166629a4570bdaa');
  assert.equal(inputs.targets.length,10);
  assert.equal(inputs.targets.flatMap(t=>t.tests).length,36);
  assert.equal(new Set(inputs.targets.map(t=>t.target)).size,10);
  assert(inputs.files.some(f=>f.path==='source-cd871181.tar.gz'));
  assert(inputs.files.some(f=>f.path==='historical-engine/patches/0001-binding-context.patch'));
});
test('reference and candidate capture the same labels in the same producer order',()=>{
  const inputs=JSON.parse(readFileSync(new URL('./inputs.json',import.meta.url)));
  for(const {target} of inputs.targets){
    const baseline=readFileSync(new URL(`./reference/crates/lycheedev-source/tests/${target}.rs`,import.meta.url),'utf8');
    const candidate=readFileSync(new URL(`../../../crates/lycheedev-source/tests/${target}.rs`,import.meta.url),'utf8');
    const extract=(text,pattern)=>[...text.matchAll(pattern)].map(m=>m[1]);
    const old=extract(baseline,/trace::record\(\s*([^,]+),/g);
    const actual=extract(candidate,/trace::(?:check|matches)\(\s*([^,]+),/g);
    // The aggregate explicitly records a failed owned result too, preserving
    // both rows and continuing all 448 cases instead of omitting failed cases.
    if(target==='selected_query_native_oracle'){
      assert.deepEqual(actual.slice(0,4),['"selected.type"','"selected.type_debug"','"selected.type"','"selected.type_debug"']);
      actual.splice(2,2);
    }
    assert.deepEqual(actual,old,target);
  }
});

test('Cargo runs every original target after a failure and keeps libtest serial',()=>{
  const inputs=JSON.parse(readFileSync(new URL('./inputs.json',import.meta.url)));
  const args=oracleCargoArgs(inputs.targets),separator=args.indexOf('--');
  assert.deepEqual(args.slice(0,7),['test','--locked','--offline','--no-fail-fast','-p','lycheedev-source','--test']);
  assert(args.indexOf('--no-fail-fast')<separator);
  assert.deepEqual(args.slice(separator),['--','--test-threads=1']);
  assert.deepEqual(args.flatMap((value,index)=>value==='--test'?[args[index+1]]:[]),inputs.targets.map(t=>t.target));
});
test('difference report retains exact independent values and all differing ordinals',()=>{
  const rows=Array.from({length:40},(_,ordinal)=>({...row,ordinal}));
  const reference=[{file:'same.jsonl',rows}],actual=structuredClone(reference);
  assert.deepEqual(traceDifferences(reference,actual),[]);
  for(const entry of actual[0].rows)entry.value=entry.value.replace('bound','native_projected');
  const [difference]=traceDifferences(reference,actual);
  assert.equal(difference.differingRows,40);
  assert.equal(difference.first.length,32);
  assert.equal(difference.moreDifferingRows,8);
  assert(difference.first.every(entry=>entry.referenceSHA256!==entry.candidateSHA256));
  assert(reference[0].rows.every(entry=>entry.value.includes('bound')));
  assert(actual[0].rows.every(entry=>entry.value.includes('native_projected')));
  actual[0].rows.pop();
  assert.equal(traceDifferences(reference,actual)[0].differingRows,40);
  assert.throws(()=>traceDifferences(reference,[]),/oracle.trace_pair_inventory/);
  assert.throws(()=>traceDifferences(reference,[{file:'other.jsonl',rows}]),/oracle.trace_pair_identity/);
});
