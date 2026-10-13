import assert from 'node:assert/strict';
import {mkdirSync,readFileSync,writeFileSync,existsSync,lstatSync} from 'node:fs';
import {join,resolve} from 'node:path';
import {createHash} from 'node:crypto';
import {ownedSpawnSync} from '../../tools/rust-owned-process.mjs';
const root=resolve('.'),release=resolve(process.env.LYCHEEDEV_RELEASE_ROOT??'.tmp/rust-luals-suspended-release-01');
const home=resolve(process.env.LYCHEEDEV_ENVIRONMENT_HOME??'.tmp/gob01');
const pin='PIN-f5ca7507c049580ad68831744678771cc49525719fdc901a91c739db2871b38b';
const out=resolve(process.argv[2]??'.tmp/rust-luals-active-cancel-01');
assert(!existsSync(out),'fresh evidence directory required');mkdirSync(out,{recursive:true});
const hash=path=>{const info=lstatSync(path);assert(info.isFile()&&!info.isSymbolicLink()&&info.size<=128<<20);return createHash('sha256').update(readFileSync(path)).digest('hex');};
const paths=['native/windows-amd64/lycheedev.exe','release.json','payload/tool/luals/bin/lua-language-server.exe'].map(path=>join(release,path));
const releaseManifest=JSON.parse(readFileSync(join(release,'release.json')));
const runtimeResources=releaseManifest.resources.filter(item=>item.path.startsWith('tool/luals/'));
assert.equal(runtimeResources.length,1207);
for(const item of runtimeResources){const path=join(release,'payload',item.path);assert.equal(lstatSync(path).size,item.bytes);assert.equal(hash(path),item.sha256);if(!paths.includes(path))paths.push(path);}
const manifest=join(root,'.tmp/rust-source-independent-retail-01/input/manifest.json');
const input=JSON.parse(readFileSync(manifest));assert.equal(input.luaFiles,2551);assert.equal(input.luaBytes,23354176);assert.equal(input.commit,'31c7f7b9cc79e56c986b365c06a6afbcf3c9177b');
paths.push(manifest,import.meta.filename,join(import.meta.dirname,'broker-active-console-cancel.ps1'),join(import.meta.dirname,'broker-console-cancel.ps1'));
const snapshot=()=>paths.map(path=>({path,sha256:hash(path)}));
const before=snapshot();writeFileSync(join(out,'inputs-before.json'),JSON.stringify(before,null,2)+'\n');
for(const path of paths.slice(-3))writeFileSync(join(out,path.split(/[\\/]/).at(-1)+'.snapshot'),readFileSync(path),{flag:'wx'});
const run=ownedSpawnSync('powershell.exe',['-NoProfile','-NonInteractive','-File',join(import.meta.dirname,'broker-active-console-cancel.ps1'),'-ReleaseRoot',release,'-WorkspaceRoot',home,'-Pin',pin,'-Output',join(out,'console')],{cwd:root,evidence:out,timeout:120000,env:process.env});
let result,error;try{result=JSON.parse(readFileSync(join(out,'console/report.json')));}catch(e){error=String(e);}
const after=snapshot(),stable=JSON.stringify(before)===JSON.stringify(after);
const report={schema:'lycheedev.source-active-console-cancel.owned.v1',passed:run.status===0&&!run.error&&run.handlesReaped&&stable&&result?.passed===true,before,after,inputsStable:stable,process:{exit:run.status,error:run.error?.message,handlesReaped:run.handlesReaped,proof:run.proof,logs:run.logs},result,error};
writeFileSync(join(out,'report.json'),JSON.stringify(report,null,2)+'\n');
console.log(JSON.stringify({passed:report.passed,report:join(out,'report.json'),failure:result?.failure,error:run.error?.message}));if(!report.passed)process.exitCode=1;
