import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdtempSync,rmSync,writeFileSync,readFileSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {fileURLToPath} from 'node:url';
import {createHash} from 'node:crypto';
import {spawnSync} from 'node:child_process';
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
test('all 31 harness inputs remain individually sealed including the immutable archive',()=>{
  const inputs=JSON.parse(readFileSync(new URL('./inputs.json',import.meta.url)));
  assert.equal(inputs.files.length,31);
  assert.equal(new Set(inputs.files.map(row=>row.path)).size,31);
  for(const row of inputs.files){
    const bytes=readFileSync(new URL(row.path,new URL('./',import.meta.url)));
    assert.equal(bytes.length,row.bytes,row.path);
    assert.equal(createHash('sha256').update(bytes).digest('hex'),row.sha256,row.path);
  }
  assert.equal(inputs.files.find(row=>row.path==='source-cd871181.tar.gz').sha256,
    '4a4657eec2b2fe7186e9815d663da938ce360fe08970240fd343fa98ea6ebdb0');
});
test('compile-only overlay has exact frozen preimages and reversible edits without prepare or Cargo',()=>{
  const script=String.raw`
import copy, hashlib, importlib.util, json, pathlib, sys, tarfile, tempfile
here=pathlib.Path(sys.argv[1])
spec=importlib.util.spec_from_file_location('fixed_materialize', here/'materialize.py')
module=importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
manifest=json.loads((here/'inputs.json').read_bytes())
frozen=json.loads((here/'source-cd871181.json').read_bytes())
rows=manifest['historicalCompileCompatibility']['files']
assert len(rows)==10
assert sum(edit['occurrences'] for row in rows for edit in row['edits'])==14
assert sum(row['kind']=='public-api-compile-adaptation' for row in rows)==9
cache=[row for row in rows if row['kind']=='missing-cache-declaration']
assert len(cache)==1
assert cache[0]['path']=='crates/lycheedev-emmy-adapter/src/session/backend.rs'
assert cache[0]['edits']==[
    {'before':'    member_lists: HashMap<OwnedValue, Vec<OwnedMemberInfo>>,',
     'after':'    member_lists: HashMap<OwnedValue, Vec<OwnedMemberInfo>>,\n    member_items: HashMap<(OwnedValue, LuaMemberKey), Option<OwnedMemberInfo>>,',
     'occurrences':1},
    {'before':'            member_lists: HashMap::new(),',
     'after':'            member_lists: HashMap::new(),\n            member_items: HashMap::new(),',
     'occurrences':1}]
originals={row['path']:row for row in frozen['files']}
archive=(here/frozen['archive']).read_bytes()
assert len(archive)==frozen['archiveBytes']
assert hashlib.sha256(archive).hexdigest()==frozen['archiveSHA256']
with tarfile.open(here/frozen['archive']) as source:
    content={entry.name:source.extractfile(entry).read() for entry in source}
assert len(content)==len(originals)==417
member_body=content['crates/lycheedev-emmy-adapter/src/session/backend/member_general.rs']
assert b'self.member_items.get(&cache_key).cloned()' in member_body
assert b'self.member_items.insert(cache_key, info.clone())' in member_body
assert not any(row['path'].endswith('/member_general.rs') for row in rows)
for name,data in content.items():
    assert len(data)==originals[name]['bytes']
    assert hashlib.sha256(data).hexdigest()==originals[name]['sha256']
def rejected(fn,reason):
    try: fn()
    except ValueError as error: assert str(error)==reason, str(error)
    else: raise AssertionError(reason)
for row in rows:
    original=content[row['path']]
    repaired=module.compatibility_bytes(row,originals[row['path']],original)
    assert repaired!=original
    restored=repaired
    for edit in reversed(row['edits']):
        restored=restored.replace(edit['after'].encode(),edit['before'].encode())
    assert restored==original
    rejected(lambda:module.compatibility_bytes(row,originals[row['path']],original+b'\n'),
             'oracle.compatibility_preimage')
    bad=copy.deepcopy(row);bad['edits'][0]['occurrences']+=1
    rejected(lambda:module.compatibility_bytes(bad,originals[row['path']],original),
             'oracle.compatibility_edit_preimage')
    bad=copy.deepcopy(row);bad['afterSHA256']='0'*64
    rejected(lambda:module.compatibility_bytes(bad,originals[row['path']],original),
             'oracle.compatibility_postimage')
with tempfile.TemporaryDirectory(prefix='emmy-compile-fixture-') as directory:
    stage=pathlib.Path(directory)
    for row in rows:
        path=stage/row['path'];path.parent.mkdir(parents=True,exist_ok=True)
        path.write_bytes(content[row['path']])
    module.apply_compile_compatibility(stage,manifest,frozen,lambda:None)
    for row in rows:
        assert hashlib.sha256((stage/row['path']).read_bytes()).hexdigest()==row['afterSHA256']
    rejected(lambda:module.apply_compile_compatibility(stage,manifest,frozen,lambda:None),
             'oracle.compatibility_preimage')
    bad=copy.deepcopy(manifest);bad['historicalCompileCompatibility']['files'][0]['path']='Cargo.lock'
    rejected(lambda:module.apply_compile_compatibility(stage,bad,frozen,lambda:None),
             'oracle.compatibility_scope')
    bad=copy.deepcopy(manifest);bad['historicalCompileCompatibility']['files'][0]=bad['historicalCompileCompatibility']['files'][1]
    rejected(lambda:module.apply_compile_compatibility(stage,bad,frozen,lambda:None),
             'oracle.compatibility_inventory')
print('authenticated 417 frozen files; 10 compile-only overlays reverse exactly; no build or prepare')
`;
  const result=spawnSync(process.platform==='win32'?'python':'python3',
    ['-I','-B','-S','-c',script,fileURLToPath(new URL('./',import.meta.url))],
    {encoding:'utf8',timeout:30000,maxBuffer:1<<20});
  assert.equal(result.status,0,result.error?.message||result.stderr);
  assert.match(result.stdout,/authenticated 417 frozen files; 10 compile-only overlays reverse exactly/);
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
