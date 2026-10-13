// Independent custody for the private BindingContext dependency experiment.
// The pinned upstream is immutable; a fork is identified by all actual bytes.
import assert from 'node:assert/strict';
import {existsSync,lstatSync,readFileSync,readdirSync,writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {join,relative,resolve,sep} from 'node:path';
import {hashFile} from '../../tools/rust-build.mjs';
import {originalV7Contract} from './wow-v7-contract.mjs';

const commit='08b0873962e83bb8307e9a81052e3f41578fbc78';
const pinnedManifestSHA256='7f4134cf5f6466ada590f2fad2693929ed71efe8d6e6b51ebf3105b0f18b9fd7';
const baseName='.tmp/rust-source-engine-research-01/upstream';
const forkName='.tmp/rust-source-binding-context-01/upstream';
const digest=value=>createHash('sha256').update(JSON.stringify(value)).digest('hex');
function inside(root,name){
 assert(typeof name==='string'&&name.length>0&&!name.includes('\\'));
 assert(!name.split('/').some(part=>!part||part==='.'||part==='..'));
 const path=resolve(root,name);assert(path.startsWith(resolve(root)+sep));
 let parent=resolve(root);for(const component of name.split('/')){parent=join(parent,component);assert(!lstatSync(parent).isSymbolicLink(),'context inputs forbid parent links');}
 return path;
}
export function inventory(directory){
 const files=[];let entries=0,totalBytes=0;
 function walk(path){
  assert(++entries<=10000,'context inventory entry budget');
  const stat=lstatSync(path);assert(!stat.isSymbolicLink(),'context inventory forbids links');
  if(stat.isDirectory()){for(const child of readdirSync(path).sort())walk(join(path,child));}
  else{
   assert(stat.isFile()&&stat.size<=64*2**20,'context inventory file budget');
   totalBytes+=stat.size;assert(totalBytes<=128*2**20,'context inventory total byte budget');
   files.push({path:relative(directory,path).split(sep).join('/'),bytes:stat.size,sha256:hashFile(path,64*2**20)});
  }
 }
 walk(directory);files.sort((a,b)=>a.path<b.path?-1:a.path>b.path?1:0);
 return {files,totalBytes};
}
export function inspectContext(root){
 root=resolve(root);
 const baseManifest=join(root,'.tmp/rust-source-engine-research-01/source-identity.json');
 const admissionPath=join(root,'.tmp/rust-source-binding-context-01/identity.json');
 const base=JSON.parse(readFileSync(baseManifest)),admission=JSON.parse(readFileSync(admissionPath));
 assert.equal(hashFile(baseManifest),pinnedManifestSHA256,'base identity must be the originally pinned manifest');
 assert.equal(base.commit,commit);assert.equal(base.files.length,848);
 assert.equal(base.totalBytes,8382174);
 // The pinned manifest keeps its original absolute research location. A
 // corresponding-source archive can move, while retaining that exact manifest
 // and validating the actual complete source inventories at the new root.
 const originalRoot=resolve(base.source,'../../..');
 assert.equal(resolve(base.source),join(originalRoot,baseName));
 assert.equal(admission.schema,'lycheedev.emmy-context-base.v1');
 assert.equal(admission.state,'admitted_unmodified_private_copy');
 assert.equal(admission.baseCommit,commit);assert.equal(admission.baseManifestSHA256,hashFile(baseManifest));
 assert.equal(resolve(admission.source),join(originalRoot,forkName));
 assert.equal(resolve(admission.baseSource),join(originalRoot,baseName));
 const original=inventory(join(root,baseName));
 const expected={files:[...base.files].sort((a,b)=>a.path<b.path?-1:a.path>b.path?1:0),totalBytes:base.totalBytes};
 assert.deepEqual(original,expected,'immutable upstream bytes must match all 848 pinned files');
 assert.deepEqual(admission.files,base.files);assert.equal(admission.totalBytes,base.totalBytes);
 const fork=inventory(join(root,forkName)),oldFiles=new Map(original.files.map(file=>[file.path,file]));
 const newFiles=new Map(fork.files.map(file=>[file.path,file]));
 const changes=[];
 for(const path of [...new Set([...oldFiles.keys(),...newFiles.keys()])].sort()){
  const before=oldFiles.get(path)??null,after=newFiles.get(path)??null;
  if(before?.sha256!==after?.sha256||before?.bytes!==after?.bytes)
   changes.push({path,kind:before?(after?'modified':'deleted'):'added',before,after});
 }
 const baseIdentity={commit,files:848,bytes:original.totalBytes,manifestSHA256:hashFile(baseManifest),admissionSHA256:hashFile(admissionPath)};
 const patchSHA256=digest({baseCommit:commit,files:fork.files,changes});
 return {baseIdentity,fork:{source:join(root,forkName),...fork},changes,patchSHA256};
}
function inputIdentity(root,names){
 assert(Array.isArray(names)&&names.length>0&&names.length<=512);
 assert.equal(new Set(names).size,names.length);
 return [...names].sort().map(path=>{
  assert(['Cargo.toml','Cargo.lock','crates/lycheedev-source/Cargo.toml','crates/lycheedev-source/src/lib.rs'].includes(path)||
   /^crates\/lycheedev-source\/src\/native\/.+\.rs$/.test(path)||
   /^crates\/lycheedev-source\/examples\/.+\.rs$/.test(path)||
   /^tests\/rust-source-engine\/.+\.(?:lua|json|mjs)$/.test(path),'explicit probe/config/fixture identity required');
  const full=inside(root,path),stat=lstatSync(full);
  assert(stat.isFile()&&!stat.isSymbolicLink()&&stat.size<=64*2**20);
  return {path,bytes:stat.size,sha256:hashFile(full,64*2**20)};
 });
}
export function verifyWiringText(cargo){
 // Accept the exact simple TOML declaration shape this experiment uses.
 // Other valid TOML forms require a reviewed parser change, not substring
 // matching that can accidentally accept a comment or unrelated table.
 let section='',featureCount=0;const features=new Map(),dependencies=new Map();
 const sections=new Set(['[package]','[features]','[[example]]','[[test]]','[dependencies]']);
 for(const raw of cargo.split(/\r?\n/)){
  const line=raw.trim();if(!line||line.startsWith('#'))continue;
  assert(!line.includes('"""')&&!line.includes("'''"),'context manifest forbids multiline strings');
  if(line.startsWith('[')){
   assert(sections.has(line),'context manifest only accepts admitted unquoted table forms');
   section=line;if(section==='[features]')featureCount++;continue;
  }
  assert(sections.has(section)&&/^[a-zA-Z0-9_.-]+\s*=/.test(line),'context manifest only accepts admitted unquoted key forms');
  if(section==='[features]'){
   const match=/^([a-zA-Z0-9_-]+)\s*=\s*(\[[^\]]*\])\s*(?:#.*)?$/.exec(line);
   assert(match,'context features require simple arrays');assert(!features.has(match[1]),'duplicate feature');features.set(match[1],match[2]);
  }
  if(section==='[dependencies]'){
   const match=/^(emmylua_code_analysis|emmylua_parser)\s*=\s*(.+)$/.exec(line);
   if(match){assert(!dependencies.has(match[1]),'duplicate context dependency');dependencies.set(match[1],match[2]);}
  }
 }
 assert.equal(featureCount,1);
 assert(!features.has('default')||/^\[\s*\]$/.test(features.get('default')),'native experiment must not enter default features');
 assert.deepEqual(JSON.parse(features.get('emmy-prototype')),['dep:emmylua_code_analysis','dep:emmylua_parser','dep:rowan','dep:tokio-util']);
 for(const crate of ['emmylua_code_analysis','emmylua_parser']){
  const expected=`{ path = "../../${forkName}/crates/${crate}", version = "=0.30.0", optional = true }`;
  assert.equal(dependencies.get(crate),expected,'native dependency must be the exact optional pinned private path');
 }
}
function verifyWiring(root){verifyWiringText(readFileSync(join(root,'crates/lycheedev-source/Cargo.toml'),'utf8'));}
export function validateAdmission(root,record){
 assert(typeof record.label==='string'&&/^[a-z0-9-]{1,96}$/.test(record.label));
 assert(typeof record.rulesVersion==='string'&&record.rulesVersion.length>0&&record.rulesVersion.length<=160);
 assert(Array.isArray(record.changes)&&record.changes.length>0,'a patched fork is required');
 for(const change of record.changes)
  assert(/^crates\/emmylua_code_analysis\/src\/.+\.rs$/.test(change.path),'first slice only admits semantic Rust changes, never dependency/config edits');
 const names=record.inputs.map(row=>row.path),required=['Cargo.toml','Cargo.lock','crates/lycheedev-source/Cargo.toml',
  'tests/rust-source-engine/context-identity.mjs','tests/rust-source-engine/context-build.mjs',
  'tests/rust-source-engine/context-native-owned.mjs',
  'tests/rust-source-engine/wow-owned-probe.mjs','tests/rust-source-engine/wow-business-contract.mjs',
  'tests/rust-source-engine/wow-v7-contract.mjs','tests/rust-source-engine/wow-v7-contract.json'];
 for(const row of inventory(join(root,'crates/lycheedev-source/examples')).files)
  if(row.path.endsWith('.rs'))required.push(`crates/lycheedev-source/examples/${row.path}`);
 // Extraction into the optional library must not make the shared fact engine
 // disappear from the sealed input closure. Bind its feature entry and every
 // native module, including modules that a particular fixture does not reach.
 if(existsSync(join(root,'crates/lycheedev-source/src/native'))){
  required.push('crates/lycheedev-source/src/lib.rs');
  for(const row of inventory(join(root,'crates/lycheedev-source/src/native')).files)
   if(row.path.endsWith('.rs'))required.push(`crates/lycheedev-source/src/native/${row.path}`);
 }
 for(const name of ['profile.lua','functions.lua','future-a.lua','future-b.lua','mixins.lua','shadow.lua','unsupported.lua','effect-mutation.lua','parallel-assignment.lua','colon-call.lua','conditional-call.lua'])
  required.push(`tests/rust-source-engine/wow/${name}`);
 for(const name of required)assert(names.includes(name),`freeze must include entire shared example closure and original fixtures: ${name}`);
 for(const row of originalV7Contract(root).fixtures){
  const frozen=record.inputs.find(input=>input.path===`tests/rust-source-engine/wow/${row.file}`);
  assert(frozen&&frozen.sha256===row.sha256&&frozen.bytes===row.bytes,'original V7 fixture bytes must remain unchanged');
 }
 assert.deepEqual(inputIdentity(root,names),record.inputs,'all probe/config/fixture bytes must match');
 verifyWiring(root);
}
export function freezeContext(root,review,path){
 root=resolve(root);path=resolve(path);assert(path.startsWith(root+sep)&&!existsSync(path),'fresh freeze record required');
 assert.equal(review.schema,'lycheedev.emmy-context-review.v1');
 assert(typeof review.label==='string'&&/^[a-z0-9-]{1,96}$/.test(review.label));
 assert(typeof review.rulesVersion==='string'&&review.rulesVersion.length>0&&review.rulesVersion.length<=160);
 const actual=inspectContext(root);assert(actual.changes.length>0,'a patched fork is required');
 assert.equal(review.patchSHA256,actual.patchSHA256,'review binds exact complete patch bytes');
 assert.deepEqual(review.changedPaths,[...actual.changes.map(row=>row.path)],'review must acknowledge every actual changed path');
 const inputs=inputIdentity(root,review.inputFiles);
 const record={schema:'lycheedev.emmy-context-freeze.v1',state:'frozen_private_experiment',label:review.label,rulesVersion:review.rulesVersion,...actual,inputs,
  scope:'private native semantic experiment; no final engine selection, public command migration or full Source acceptance'};
 validateAdmission(root,record);
 writeFileSync(path,JSON.stringify(record,null,2)+'\n',{flag:'wx'});return record;
}
export function verifyFrozenContext(root,path){
 root=resolve(root);path=resolve(path);assert(path.startsWith(root+sep));
 const stat=lstatSync(path);assert(stat.isFile()&&!stat.isSymbolicLink()&&stat.size<=2*2**20);
 const record=JSON.parse(readFileSync(path));assert.equal(record.schema,'lycheedev.emmy-context-freeze.v1');
 assert.equal(record.state,'frozen_private_experiment');
 const actual=inspectContext(root);
 for(const key of ['baseIdentity','fork','changes','patchSHA256'])assert.deepEqual(actual[key],record[key],`context ${key} changed after freeze`);
 validateAdmission(root,record);return {path,sha256:hashFile(path),record};
}
if(process.argv[1]&&resolve(process.argv[1])===resolve(import.meta.filename)){
 const [mode,path,out]=process.argv.slice(2),root=resolve('.');
 if(mode==='inspect'){
  const actual=inspectContext(root);console.log(JSON.stringify({baseIdentity:actual.baseIdentity,files:actual.fork.files.length,bytes:actual.fork.totalBytes,patchSHA256:actual.patchSHA256,changes:actual.changes},null,2));
 }else if(mode==='freeze'){
  assert(path&&out);const record=freezeContext(root,JSON.parse(readFileSync(path)),out);
  console.log(JSON.stringify({path:resolve(out),sha256:hashFile(resolve(out)),patchSHA256:record.patchSHA256,changes:record.changes.length}));
 }else if(mode==='verify'){
  assert(path);const verified=verifyFrozenContext(root,path);
  console.log(JSON.stringify({path:verified.path,sha256:verified.sha256,patchSHA256:verified.record.patchSHA256,changes:verified.record.changes.length}));
 }else throw new Error('usage: context-identity.mjs inspect | freeze <review.json> <new-freeze.json> | verify <freeze.json>');
}
