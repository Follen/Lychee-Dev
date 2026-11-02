// Corresponding source includes the complete NEW public-base reconstruction,
// its original MIT license, and the independently verifiable tracked recipe.
import assert from 'node:assert/strict';
import {constants,copyFileSync,existsSync,mkdirSync} from 'node:fs';
import pathAPI,{join,resolve} from 'node:path';
import {emmyNoLinks,emmyRecipePath,emmyRelativePath,emmySourcePath,inspectEmmy} from './rust-emmy-identity.mjs';

// Inventory paths are always POSIX-style. Do not feed them through the host's
// dirname and then split on '/', which can lose parent boundaries on Windows.
export function emmyPathsOverlap(left,right,paths=pathAPI){
  const contains=(parent,child)=>{const rel=paths.relative(parent,child);return rel===''||(!paths.isAbsolute(rel)&&rel!=='..'&&!rel.startsWith(`..${paths.sep}`));};
  return contains(left,right)||contains(right,left);
}
export function emmyStageParentComponents(path){return emmyRelativePath(path).split('/').slice(0,-1);}
function directory(root,components){
  let path=root;emmyNoLinks(path);
  for(const part of components){
    path=join(path,part);if(!existsSync(path))mkdirSync(path);emmyNoLinks(path);
  }
  return path;
}
export function stageEmmySources(root,destination){
  root=resolve(root);destination=resolve(destination);
  assert.notEqual(root,destination);
  const before=inspectEmmy(root);destination=emmyNoLinks(destination);
  assert(!emmyPathsOverlap(before.source,destination),'emmy.overlapping_source_stage');
  for(const target of [emmyRecipePath,emmySourcePath])assert(!existsSync(join(destination,target)),'emmy.fresh_stage_required');
  for(const [prefix,rows] of [[emmyRecipePath,before.recipeFiles],[emmySourcePath,before.files],['',before.supportFiles]]){
    for(const file of rows){
      const path=prefix?`${prefix}/${file.path}`:file.path;
      directory(destination,emmyStageParentComponents(path));
      // COPYFILE_EXCL: never replace another owner's file, including a link.
      // The enclosing source snapshot may already include release policy.
      // Never replace it; the complete post-copy identity check authenticates it.
      if(!prefix&&existsSync(join(destination,path)))continue;
      copyFileSync(emmyNoLinks(join(root,path)),join(destination,path),constants.COPYFILE_EXCL);
    }
  }
  // A standalone stage has no product workspace. Give the owned dependency a
  // bounded local workspace instead of letting Cargo discover an outer checkout.
  const rootManifest=join(destination,'Cargo.toml');
  if(!existsSync(rootManifest))copyFileSync(join(destination,emmyRecipePath,'standalone-Cargo.toml'),rootManifest,constants.COPYFILE_EXCL);
  const after=inspectEmmy(destination),original=inspectEmmy(root);
  for(const current of [after,original]){
    assert.equal(current.recipeSHA256,before.recipeSHA256,'emmy.recipe_changed_during_stage');
    assert.equal(current.patchedTreeSHA256,before.patchedTreeSHA256,'emmy.tree_changed_during_stage');
  }
  return {schema:after.schema,identity:after.identity,historicalPrivatePatchRecovered:false,
    base:after.base,identitySHA256:after.identitySHA256,recipeSHA256:after.recipeSHA256,
    patchedTreeSHA256:after.patchedTreeSHA256,files:after.files.length,bytes:after.totalBytes};
}
