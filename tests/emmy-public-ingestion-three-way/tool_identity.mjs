// External tool identity only. Source/payload/cache inventories never use this.
import assert from 'node:assert/strict';
import * as fs from 'node:fs';
import {resolve,dirname,basename,isAbsolute,win32} from 'node:path';
import {createHash} from 'node:crypto';

// The hosted Python installer creates exactly python3.exe -> python.exe.
// No PATH/env-selected directory, arbitrary tool link or OS/.NET link is trusted.
export function hostedPythonAlias(python) {
  assert(/^C:\\hostedtoolcache\\windows\\Python\\3\.(?:12|13|14)\.\d+\\x64\\python\.exe$/i.test(python),
    'windows-contract.fixed_hosted_python_root');
  return Object.freeze({path:win32.join(win32.dirname(python),'python3.exe'),target:python});
}

function identity(stat) {
  return Object.fromEntries(['dev','ino','mode','nlink','size','mtimeNs','ctimeNs','birthtimeNs']
    .map(key=>[key,stat[key].toString()]));
}
const ordered=rows=>Object.fromEntries(Object.entries(rows).sort(([a],[b])=>a<b?-1:a>b?1:0));
const samePath=(a,b)=>process.platform==='win32'?resolve(a).toLowerCase()===resolve(b).toLowerCase():resolve(a)===resolve(b);

// io is replaceable only for deterministic filesystem-race fixtures. The fixed
// Windows entrypoint supplies no override and authenticates this module first.
export function toolFileIdentities(paths,pythonAlias,{io=fs,maxBytes=512<<20}={}) {
  assert(isAbsolute(pythonAlias.path)&&isAbsolute(pythonAlias.target)
    &&samePath(dirname(pythonAlias.path),dirname(pythonAlias.target))
    &&basename(pythonAlias.path)==='python3.exe'&&basename(pythonAlias.target).toLowerCase()==='python.exe',
  'windows-contract.fixed_python_alias');
  const selected=[...new Set(paths.map(p=>resolve(p)))].sort();
  assert(selected.length<=1024&&selected.some(p=>samePath(p,pythonAlias.target)),
    'windows-contract.tool_file_count_or_python');
  const files={},mappings={};
  function noLinks(path) {
    for(let p=resolve(path);;p=dirname(p)) {
      assert(!io.lstatSync(p).isSymbolicLink(),`windows-contract.tool_reparse:${p}`);
      if(p===dirname(p))break;
    }
  }
  function mapping(path) {
    noLinks(dirname(path));
    const entry=io.lstatSync(path,{bigint:true});let link=null;
    if(entry.isSymbolicLink()) {
      assert(samePath(path,pythonAlias.path),'windows-contract.unapproved_tool_alias');
      link=io.readlinkSync(path);
      // Check the direct target too: an intermediate link, even one resolving
      // back to python.exe, is not this reviewed single-leaf mapping.
      assert(samePath(resolve(dirname(path),link),pythonAlias.target),'windows-contract.tool_alias_escape');
      noLinks(pythonAlias.target);
    } else {
      assert(entry.isFile(),'windows-contract.tool_not_file');
    }
    const target=io.realpathSync(path);
    assert(samePath(target,link===null?path:pythonAlias.target),'windows-contract.tool_resolution_drift');
    noLinks(target);
    return {target,link,entry:identity(entry)};
  }
  for(const requested of selected) {
    try {
      const before=mapping(requested),stat=io.lstatSync(before.target,{bigint:true});
      assert(stat.isFile()&&stat.size<=BigInt(maxBytes),'windows-contract.tool_file_budget');
      const expected=identity(stat),fd=io.openSync(before.target,'r');
      const hash=createHash('sha256'),buffer=Buffer.alloc(65536);let bytes=0;
      try {
        assert.deepEqual(identity(io.fstatSync(fd,{bigint:true})),expected,'windows-contract.tool_open_drift');
        for(;;) {
          const n=io.readSync(fd,buffer,0,buffer.length,null);if(!n)break;
          bytes+=n;assert(bytes<=maxBytes,'windows-contract.tool_file_grew');hash.update(buffer.subarray(0,n));
        }
        assert.equal(BigInt(bytes),stat.size,'windows-contract.tool_size_drift');
        assert.deepEqual(identity(io.fstatSync(fd,{bigint:true})),expected,'windows-contract.tool_handle_drift');
        noLinks(before.target);
        assert.deepEqual(identity(io.lstatSync(before.target,{bigint:true})),expected,'windows-contract.tool_target_drift');
        assert.deepEqual(mapping(requested),before,'windows-contract.tool_mapping_drift');
      } finally {io.closeSync(fd);}
      const value={sha256:hash.digest('hex'),identity:expected};
      if(Object.hasOwn(files,before.target))assert.deepEqual(files[before.target],value,'windows-contract.tool_alias_identity_drift');
      files[before.target]=value;mappings[requested]=before;
    } catch(error) {
      // Pre-owner failures are retained in the CI log with their exact path.
      throw new Error(`windows-contract.tool_identity:${JSON.stringify({requested,cause:error.message})}`,{cause:error});
    }
  }
  return {files:ordered(files),mappings:ordered(mappings)};
}
