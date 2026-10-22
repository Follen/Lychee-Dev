// New public-base reconstruction custody. This never authenticates or claims
// recovery of the historical private BindingContext experiment.
import assert from 'node:assert/strict';
import {closeSync,fstatSync,lstatSync,openSync,opendirSync,readSync,realpathSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {dirname,join,resolve} from 'node:path';

export const emmySourcePath='.tmp/emmy-reconstruction-v1/upstream';
export const emmyRecipePath='third_party/emmy';
// Static imports of recipe check.mjs plus its runtime PowerShell helper/policy.
export const emmySupportFiles=['tools/rust-emmy-identity.mjs','tools/rust-disk.mjs','tools/rust-owned-process.mjs','tools/rust-owned-process.ps1','tools/rust-build-auxiliary.json','release/rust-disk-policy.json'];
export const emmyOwnedCrate='crates/lycheedev-binding-contracts';
export const emmyOwnedFiles=['Cargo.toml','src/lib.rs','tests/contracts.rs'].map(path=>`${emmyOwnedCrate}/${path}`);
export const emmyRecipeFiles=['.gitignore','README.md','check.mjs','files.json','identity.json','prepare.py','scratch.mjs','seal.py','test_prepare.py','standalone-Cargo.toml'];
const digest=bytes=>createHash('sha256').update(bytes).digest('hex');
const hex=value=>typeof value==='string'&&/^[a-f0-9]{64}$/.test(value);
// Python orders Unicode code points; JS default order compares UTF-16 units.
const sorted=(a,b)=>Buffer.compare(Buffer.from(a,'utf8'),Buffer.from(b,'utf8'));
const maxFile=64*2**20,maxTotal=128*2**20,maxEntries=10000;
function check(condition,message){assert(condition,`emmy.${message}`);}
export function emmyRelativePath(name){
  check(typeof name==='string'&&name.length>0&&name.length<=1024&&name.normalize('NFC')===name,'path');
  for(const part of name.split('/'))check(part&&part!=='.'&&part!=='..'&&
    !/[\\:<>"|?*\x00-\x1f]/.test(part)&&!/[. ]$/.test(part)&&
    !/^(?:con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\.|$)/i.test(part),'path');
  return name;
}
// Reject links at every ancestor, including the caller-supplied root.
export function emmyNoLinks(path){
  path=resolve(path);let current=path;
  for(;;){check(!lstatSync(current).isSymbolicLink(),'symlink');const parent=dirname(current);if(parent===current)break;current=parent;}
  return realpathSync(path);
}
function readBounded(path,max=maxFile){
  emmyNoLinks(path);const before=lstatSync(path);
  check(before.isFile()&&before.size<=max,'file_budget');
  const fd=openSync(path,'r');
  try{
    const opened=fstatSync(fd);check(opened.isFile()&&opened.dev===before.dev&&opened.ino===before.ino&&opened.size===before.size,'file_changed');
    const result=Buffer.alloc(before.size);let offset=0;
    while(offset<result.length){const count=readSync(fd,result,offset,result.length-offset,null);check(count>0,'file_changed');offset+=count;}
    check(readSync(fd,Buffer.alloc(1),0,1,null)===0,'file_changed');
    const after=fstatSync(fd),named=lstatSync(path);
    check(after.size===before.size&&after.mtimeMs===before.mtimeMs&&after.ctimeMs===before.ctimeMs&&named.dev===before.dev&&named.ino===before.ino&&!named.isSymbolicLink(),'file_changed');
    return result;
  }finally{closeSync(fd);}
}
function row(root,path,max=maxFile){emmyRelativePath(path);const bytes=readBounded(join(root,path),max);return{path,bytes:bytes.length,sha256:digest(bytes)};}
// Match Python json.dumps(ensure_ascii=True,separators=(',', ':')) exactly.
export function emmyTreeDigest(rows){return digest(JSON.stringify(rows).replace(/[\u007f-\uffff]/g,c=>'\\u'+c.charCodeAt(0).toString(16).padStart(4,'0')));}
export function emmyInventory(directory){
  emmyNoLinks(directory);const files=[],directories=[],lowerPaths=new Set(),upperPaths=new Set();let entries=0,totalBytes=0;
  function visit(path,name){
    check(++entries<=maxEntries,'entry_budget');
    if(name){emmyRelativePath(name);const lower=name.toLowerCase(),upper=name.toUpperCase();check(!lowerPaths.has(lower)&&!upperPaths.has(upper),'path_alias');lowerPaths.add(lower);upperPaths.add(upper);}
    const stat=lstatSync(path);check(!stat.isSymbolicLink(),'symlink');
    if(stat.isDirectory()){
      if(name)directories.push(name);
      const directory=opendirSync(path);
      try{let child;while((child=directory.readSync())){const relative=name?`${name}/${child.name}`:child.name;emmyRelativePath(relative);visit(join(path,child.name),relative);}}finally{directory.closeSync();}
    }
    else{const record=row(directory,name);totalBytes+=record.bytes;check(totalBytes<=maxTotal,'total_budget');files.push(record);}
  }
  visit(directory,'');files.sort((a,b)=>sorted(a.path,b.path));
  const expectedDirectories=new Set();
  for(const file of files){let parent=file.path;while(parent.includes('/')){parent=parent.slice(0,parent.lastIndexOf('/'));expectedDirectories.add(parent);check(expectedDirectories.size<=maxEntries,'entry_budget');}}
  assert.deepEqual(directories.sort(sorted),[...expectedDirectories].sort(sorted),'emmy.unexpected_directory');
  return{files,totalBytes};
}
export function inspectEmmy(root){
  root=emmyNoLinks(root);const recipe=join(root,emmyRecipePath),source=join(root,emmySourcePath);
  const identityBytes=readBounded(join(recipe,'identity.json'),64*1024);
  const identity=JSON.parse(identityBytes),files=JSON.parse(readBounded(join(recipe,'files.json'),4*2**20));
  check(identity.schema==='lycheedev.emmy-reconstruction.v1'&&['public-base-binding-context-reconstruction-v1','public-upstream-unmodified-v1'].includes(identity.identity)&&identity.historicalPrivatePatchRecovered===false,'identity_schema');
  check(identity.base?.repository==='https://github.com/CppCXY/emmylua-analyzer-rust'&&identity.base.commit==='08b0873962e83bb8307e9a81052e3f41578fbc78'&&
    identity.base.archiveSHA256==='ae529a06b551dbb301744bea2b58ba94cb3e30340f63a8c83f9335be9086fdb7'&&identity.base.archiveBytes===2386393&&
    identity.base.treeSHA256==='375d92c5d5e503d77232fabf22a21a82f504c2b38dc5def8fcca1d6ecf06e104'&&identity.base.files===848&&identity.base.bytes===8382174,'base_identity');
  check(identity.base.archiveURL===`https://codeload.github.com/CppCXY/emmylua-analyzer-rust/tar.gz/${identity.base.commit}`&&identity.base.archiveRoot===`emmylua-analyzer-rust-${identity.base.commit}`,'archive_identity');
  check(identity.base.license?.path==='LICENSE'&&identity.base.license.spdx==='MIT'&&identity.base.license.sha256==='7f8fd5ae0962c6757a8980592c264c8bce5004aa9f369084db761874b8969ff8','license_identity');
  check(Array.isArray(files)&&files.length>0&&files.length<=maxEntries&&files.length===identity.files,'inventory_budget');
  let total=0,previous='',seen=new Set();
  for(const file of files){
    check(Object.keys(file).sort().join(',')==='bytes,path,sha256','inventory_fields');emmyRelativePath(file.path);
    check(sorted(previous,file.path)<0&&!seen.has(file.path.toLowerCase()),'inventory_order');previous=file.path;seen.add(file.path.toLowerCase());
    check(Number.isSafeInteger(file.bytes)&&file.bytes>=0&&file.bytes<=maxFile&&hex(file.sha256),'inventory_entry');total+=file.bytes;
  }
  check(total<=maxTotal&&total===identity.bytes,'inventory_bytes');
  check(hex(identity.patchedTreeSHA256)&&emmyTreeDigest(files)===identity.patchedTreeSHA256,'inventory_digest');
  check(Array.isArray(identity.patches)&&identity.patches.length<=32,'patch_budget');
  const unmodified=identity.identity==='public-upstream-unmodified-v1';
  check(unmodified?identity.patches.length===0&&identity.patchedTreeSHA256===identity.base.treeSHA256&&identity.files===identity.base.files&&identity.bytes===identity.base.bytes:identity.patches.length>0,'unmodified_identity');
  const patches=[];
  for(const patch of identity.patches){
    emmyRelativePath(patch.path);check(/^patches\/[^/]+\.patch$/.test(patch.path)&&hex(patch.sha256)&&!patches.includes(patch.path),'patch_path');
    check(row(recipe,patch.path,8*2**20).sha256===patch.sha256,'patch_digest');patches.push(patch.path);
  }
  const patchNames=[];
  let patchDirectory;
  try { patchDirectory=opendirSync(emmyNoLinks(join(recipe,'patches'))); }
  catch(error){if(!unmodified||error.code!=='ENOENT')throw error;}
  if(patchDirectory)try{let entry;while((entry=patchDirectory.readSync())){check(patchNames.length<32&&entry.isFile()&&!entry.isSymbolicLink(),'unknown_patch');patchNames.push(entry.name);}}finally{patchDirectory.closeSync();}
  check(JSON.stringify(patchNames.sort(sorted))===JSON.stringify(patches.map(path=>path.slice(8)).sort(sorted)),'unknown_patch');
  const recipeFiles=[...emmyRecipeFiles,...patches].sort(sorted).map(path=>row(recipe,path,8*2**20));
  const owned=emmyInventory(join(root,emmyOwnedCrate)).files.map(file=>({...file,path:`${emmyOwnedCrate}/${file.path}`}));
  assert.deepEqual(owned.map(file=>file.path),emmyOwnedFiles,'emmy.owned_contract_inventory');
  assert.deepEqual(owned,identity.ownedFiles,'emmy.owned_contract_identity');
  check(emmyTreeDigest(owned)===identity.ownedTreeSHA256,'owned_contract_identity');
  const supportFiles=[...emmySupportFiles,...emmyOwnedFiles].map(path=>row(root,path,8*2**20));
  const actual=emmyInventory(source);assert.deepEqual(actual.files,files,'emmy.generated_inventory');
  check(actual.totalBytes===total,'generated_bytes');
  check(files.find(file=>file.path==='LICENSE')?.sha256===identity.base.license.sha256,'original_license_bytes');
  return{schema:identity.schema,identity:identity.identity,historicalPrivatePatchRecovered:false,base:identity.base,
    identitySHA256:digest(identityBytes),recipeSHA256:emmyTreeDigest([...recipeFiles.map(file=>({...file,path:`${emmyRecipePath}/${file.path}`})),...supportFiles].sort((a,b)=>sorted(a.path,b.path))),patchedTreeSHA256:identity.patchedTreeSHA256,
    source,files,totalBytes:total,recipeFiles,supportFiles};
}
export function verifyEmmyWiring(cargo,{optional=true}={}){
  check(typeof optional==='boolean','dependency_wiring');
  check(!cargo.includes('"""')&&!cargo.includes("'''"),'dependency_wiring');
  let section='',found=new Set(),dependencyTables=0;
  for(const raw of cargo.split(/\r?\n/)){
    const line=raw.trim();if(line.startsWith('[')){section=line;if(line==='[dependencies]')dependencyTables++;continue;}
    if(section!=='[dependencies]'||!/^emmylua_(?:code_analysis|parser)\s*=/.test(line))continue;
    const match=/^(emmylua_code_analysis|emmylua_parser) = \{ path = "([^\"]+)", version = "=0\.30\.0"(, optional = true)? \}$/.exec(line);
    check(match&&!found.has(match[1])&&match[2]===`../../${emmySourcePath}/crates/${match[1]}`&&Boolean(match[3])===optional,'dependency_wiring');found.add(match[1]);
  }
  check(found.size===2&&dependencyTables===1,'dependency_wiring');
}
