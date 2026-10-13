// Include the complete, pinned private Emmy dependency in corresponding source.
import assert from 'node:assert/strict';
import {cpSync,mkdirSync} from 'node:fs';
import {join,resolve} from 'node:path';
import {inspectContext} from '../tests/rust-source-engine/context-identity.mjs';

export function stageEmmySources(root,destination){
  root=resolve(root);destination=resolve(destination);
  assert.notEqual(root,destination);
  const before=inspectContext(root);
  for(const [directory,manifest] of [
    ['.tmp/rust-source-engine-research-01','source-identity.json'],
    ['.tmp/rust-source-binding-context-01','identity.json'],
  ]){
    const target=join(destination,directory);mkdirSync(target,{recursive:true});
    cpSync(join(root,directory,'upstream'),join(target,'upstream'),{recursive:true,errorOnExist:true,force:false});
    cpSync(join(root,directory,manifest),join(target,manifest),{errorOnExist:true,force:false});
  }
  const after=inspectContext(destination);
  assert.equal(after.patchSHA256,before.patchSHA256);
  assert.deepEqual(after.baseIdentity,before.baseIdentity);
  assert.deepEqual(after.fork.files,before.fork.files);
  return {baseIdentity:after.baseIdentity,patchSHA256:after.patchSHA256,files:after.fork.files.length,bytes:after.fork.totalBytes};
}
