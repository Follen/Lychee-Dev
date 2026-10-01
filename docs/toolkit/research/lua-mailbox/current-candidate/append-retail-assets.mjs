import fs from 'node:fs/promises';
import path from 'node:path';
import {createHash} from 'node:crypto';
const base=path.resolve('docs/toolkit/research/lua-mailbox');
const dir=path.join(base,'current-candidate');
const source=path.resolve('.tmp/retail-readonly-retirement-20261001');
const hash=b=>createHash('sha256').update(b).digest('hex');
await fs.mkdir(dir,{recursive:true});
await fs.copyFile(path.join(base,'asset-digests.json'),path.join(dir,'previous-asset-digests.json'),fs.constants.COPYFILE_EXCL);
for(const file of await fs.readdir(source)) if(file!=='describe.json') await fs.copyFile(path.join(source,file),path.join(dir,file),fs.constants.COPYFILE_EXCL);
for(const file of ['report.json','activate-current-bytes.json','connect.json','recovery-status.json','recovery-resume.json']) await fs.copyFile(path.resolve('.tmp/mailbox-live/current-retail-core',file),path.join(dir,'original-'+file),fs.constants.COPYFILE_EXCL);
const jpath=path.resolve('.tmp/mailbox-live/current-retail-project/.lycheedev/live/connections/CON-2eda9cdcf05088ce5c1358e2c22ee994.jsonl');
const journal=await fs.readFile(jpath); const last=JSON.parse(journal.toString().trim().split(/\r?\n/).at(-1));
const summary={schema:'lycheedev.mailbox.current-candidate-acceptance.v1',recordedAt:new Date().toISOString(),client:'retail',build:'12.1.0.69933',cliSHA256:'9b4846015f9a22d64994b494be8c0412eace70c0fbdd24344239283bc125f7a8',pid:34732,creation:'134352920362860468',image:'D:/Game/World of Warcraft/_retail_/Wow.exe',executableSHA256:'d41f11de411f6fdb280a7c1c6380ba3d1b17614f03e5aca8fabce2cc715cd7dd',session:'CON-2eda9cdcf05088ce5c1358e2c22ee994',acceptance:{state:'blocked',core:'not_run',capacity47:'not_run',reason:'Activation input uncertain under external held-key interference; original process subsequently exited. No same-path replacement process exists at exact-path check.'},recovery:{state:'closed',method:'process_exited',publicCommand:'live disconnect',inputFree:true,oldReloadDisposition:'uncertain',oldReloadMessagesQueued:9,originalRuntime:'000000190001017ed9a31892c07a556f',observedDifferentRuntime:'0000001a0001050d505341dec3d8119f',runtimeReplacementProofEstablished:false,finalJournalSequence:last.sequence,finalJournalKind:last.kind,finalJournalSHA256:hash(journal)},library:{newConfirm:'not_run',reason:'Blocked acceptance is append-only evidence, not a new semantic validation or promotion of previous read confirmation.'},authorization:false};
await fs.writeFile(path.join(dir,'summary.json'),JSON.stringify(summary,null,2)+'\n',{flag:'wx'});
const walk=async d=>{let out=[];for(const e of await fs.readdir(d,{withFileTypes:true})){const f=path.join(d,e.name);if(e.isDirectory())out.push(...await walk(f));else out.push(f);}return out;};
const files=(await walk(base)).filter(f=>f!==path.join(base,'asset-digests.json')).sort();
const seal=JSON.parse(await fs.readFile(path.join(base,'asset-digests.json'),'utf8'));
seal.files=[];for(const f of files){const b=await fs.readFile(f);seal.files.push({file:path.relative(base,f).replaceAll('\\','/'),bytes:b.length,sha256:hash(b)});}
await fs.writeFile(path.join(base,'asset-digests.json'),JSON.stringify(seal,null,2)+'\n');
for(const f of seal.files){const b=await fs.readFile(path.join(base,f.file));if(b.length!==f.bytes||hash(b)!==f.sha256)throw Error('seal mismatch '+f.file);}
const evidence={schema:'lycheedev.mailbox.library-evidence.appendix.v1',createdAt:new Date().toISOString(),bundleID:seal.bundleID,recipeID:'retail-lua-state-root-rip-v1',validation:{currentCandidate:'blocked',core:'not_run',capacity47:'not_run',processCleanup:'passed',executionAuthorization:false,newLibraryConfirm:'not_run'},files:[]};
for(const file of [...seal.files.filter(f=>f.file.startsWith('current-candidate/')).map(f=>path.join(base,f.file)),path.join(base,'asset-digests.json')]){const b=await fs.readFile(file);evidence.files.push({file:path.relative(process.cwd(),file).replaceAll('\\','/'),bytes:b.length,sha256:hash(b),encoding:'base64',content:b.toString('base64')});}
const out=path.resolve('.tmp/retail-readonly-retirement-20261001/library-evidence-appendix.json');
await fs.writeFile(out,JSON.stringify(evidence,null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({assetFiles:seal.files.length,appendixFiles:evidence.files.length,sealVerified:true,summary,appendix:path.relative(process.cwd(),out)}));
