// Read-only copy of the pinned historical runner's five pure trace functions.
// Their function bodies must stay byte-identical to run.mjs (tested); only local
// SHA/file helpers replace imports of the mutable Cargo/process tool chain.
import assert from 'node:assert/strict';
import {isDeepStrictEqual} from 'node:util';
import {createHash} from 'node:crypto';
import {lstatSync,readFileSync,readdirSync} from 'node:fs';
import {join} from 'node:path';
const SCHEMA='lycheedev.emmy-fixed-oracle.v1';
const MAX_TRACE=16<<20,MAX_TRACE_PARTS=8,MAX_TEST_TRACE=64<<20;
const sha256=data=>createHash('sha256').update(data).digest('hex');
function hashFile(path,max=MAX_TRACE){
  const info=lstatSync(path);
  assert(info.isFile()&&!info.isSymbolicLink()&&info.size<=max,'current_contract.trace_file_budget');
  const bytes=readFileSync(path);
  assert(bytes.length===info.size,'current_contract.trace_file_changed');
  return sha256(bytes);
}

export function validateFrames(frames,{target,test,role}){
  assert(Array.isArray(frames)&&frames.length>=2,'oracle.missing_frames');
  assert.deepEqual(frames[0],{kind:'header',schema:SCHEMA,role,target,test},'oracle.header');
  const rows=frames.slice(1,-1);
  assert(rows.length>0,'oracle.empty_test');
  assert.deepEqual(frames.at(-1),{kind:'end',rows:rows.length,complete:true},'oracle.incomplete');
  for(const [ordinal,row] of rows.entries()){
    assert.deepEqual(Object.keys(row).sort(),['kind','label','ordinal','value'],'oracle.row_fields');
    assert(row.kind==='row'&&row.ordinal===ordinal&&typeof row.label==='string'&&typeof row.value==='string','oracle.row');
  }
  return rows;
}

export function readTraces(directory,targets,role){
  const expected=targets.flatMap(({target,tests})=>tests.map(test=>({target,test,file:`${target}--${test}.jsonl`})));
  assert.equal(new Set(expected.map(e=>e.file)).size,expected.length,'oracle.test_inventory');
  const actual=new Set(readdirSync(directory));
  const allowed=new Set(expected.flatMap(e=>Array.from({length:MAX_TRACE_PARTS},(_,part)=>part?`${e.file}.part${part}`:e.file)));
  assert([...actual].every(file=>allowed.has(file)),'oracle.test_inventory');
  return expected.map(entry=>{
    assert(actual.has(entry.file),'oracle.test_inventory');
    const parts=[],frames=[],digest=createHash('sha256');
    let bytes=0,missing=false;
    for(let part=0;part<MAX_TRACE_PARTS;part++){
      const file=part?`${entry.file}.part${part}`:entry.file;
      if(!actual.has(file)){missing=true;continue;}
      assert(!missing,'oracle.test_inventory');
      const path=join(directory,file),info=lstatSync(path);
      assert(info.isFile()&&!info.isSymbolicLink()&&info.size<=MAX_TRACE,'oracle.trace_budget');
      assert(info.size>0,'oracle.empty_part');
      bytes+=info.size;
      assert(bytes<=MAX_TEST_TRACE,'oracle.test_trace_budget');
      parts.push({file,path,bytes:info.size});
    }
    // Admit the whole logical trace before allocating or parsing its contents.
    for(const part of parts){
      const data=readFileSync(part.path);
      assert.equal(data.length,part.bytes,'oracle.trace_changed');
      const text=new TextDecoder('utf-8',{fatal:true}).decode(data);
      assert(text.endsWith('\n'),'oracle.truncated_line');
      for(const line of text.slice(0,-1).split('\n')){
        assert(line.length>0,'oracle.empty_line');
        frames.push(JSON.parse(line));
      }
      digest.update(data);
      part.sha256=sha256(data);
    }
    const rows=validateFrames(frames,{...entry,role});
    return {...entry,path:parts[0].path,bytes,sha256:digest.digest('hex'),parts,rows};
  });
}

export function assertTracesUnchanged(directory,traces){
  const parts=traces.flatMap(trace=>trace.parts);
  assert.deepEqual(readdirSync(directory).sort(),parts.map(part=>part.file).sort(),'oracle.reference_inventory_changed');
  for(const part of parts){
    const path=join(directory,part.file),info=lstatSync(path);
    assert(info.isFile()&&!info.isSymbolicLink()&&info.size<=MAX_TRACE,'oracle.reference_trace_changed');
    assert.equal(info.size,part.bytes,'oracle.reference_trace_changed');
    assert.equal(hashFile(path,MAX_TRACE),part.sha256,'oracle.reference_trace_changed');
  }
}

export function oracleCargoArgs(targets){
  return ['test','--locked','--offline','--no-fail-fast','-p','lycheedev-source',
    ...targets.flatMap(t=>['--test',t.target]),'-j','1','--','--test-threads=1'];
}

export function traceDifferences(reference,candidate){
  assert.equal(candidate.length,reference.length,'oracle.trace_pair_inventory');
  const differences=[];
  for(let i=0;i<reference.length;i++){
    const old=reference[i],actual=candidate[i];
    assert.equal(actual.file,old.file,'oracle.trace_pair_identity');
    let differingRows=0;const first=[];
    for(let ordinal=0;ordinal<Math.max(old.rows.length,actual.rows.length);ordinal++){
      const expected=old.rows[ordinal],value=actual.rows[ordinal];
      if(isDeepStrictEqual(value,expected))continue;
      differingRows++;
      if(first.length<32)first.push({ordinal,referenceLabel:expected?.label??null,candidateLabel:value?.label??null,
        referenceSHA256:expected?sha256(Buffer.from(JSON.stringify(expected))):null,
        candidateSHA256:value?sha256(Buffer.from(JSON.stringify(value))):null});
    }
    if(differingRows)differences.push({file:old.file,referenceRows:old.rows.length,candidateRows:actual.rows.length,
      differingRows,first,moreDifferingRows:differingRows-first.length});
  }
  return differences;
}
