// Public immutable-Git comparison against the frozen oracle; never imports user workspaces.
import {readFileSync,writeFileSync,mkdirSync,existsSync} from 'node:fs';
import {resolve,join} from 'node:path';
import {spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {isDeepStrictEqual} from 'node:util';
const root=resolve(process.cwd()),stage=resolve(process.argv[2]||'.tmp/rust-source-diff-public'),rust=resolve(process.argv[3]||'target/debug/lycheedev.exe'),go=join(root,'.tmp/lycheedev-oracle.exe');
const home=join(stage,'home'),mirror=join(home,'mirrors/wow-ui-source.git');
if(existsSync(home))throw new Error('Use a new task-owned stage; do not overwrite existing identity or evidence.');
mkdirSync(stage,{recursive:true});
const initialization=spawnSync(rust,['init','--home',home,'--format','json'],{encoding:'utf8',windowsHide:true,shell:false,timeout:30000});
if(initialization.status!==0)throw new Error(`fixture.init_failed:${initialization.stdout}`);
mkdirSync(mirror,{recursive:true});
const sha=path=>createHash('sha256').update(readFileSync(path)).digest('hex');
const git=(args,input='')=>{const run=spawnSync('git',[`--git-dir=${mirror}`,...args],{input,encoding:'utf8',windowsHide:true,shell:false,env:{...process.env,GIT_AUTHOR_NAME:'Diff Fixture',GIT_AUTHOR_EMAIL:'fixture@example.invalid',GIT_COMMITTER_NAME:'Diff Fixture',GIT_COMMITTER_EMAIL:'fixture@example.invalid',GIT_AUTHOR_DATE:'2026-10-08T00:00:00Z',GIT_COMMITTER_DATE:'2026-10-08T00:00:00Z'}});if(run.status!==0)throw new Error(run.stderr);return run.stdout.trim();};
git(['init','--bare']);
const commit=(files,message)=>{const entries=Object.entries(files).sort(([a],[b])=>a.localeCompare(b)).map(([path,text])=>`100644 blob ${git(['hash-object','-w','--stdin'],text)}\t${path}\n`).join('');return git(['commit-tree',git(['mktree'],entries)],message);};
const before={'a.lua':'function Shared()\nend\n','b.lua':'function Shared()\nend\n','changed.lua':'function Changed(value)\nend\n','moved.lua':'function Moved()\nend\n','removed.lua':'function Removed()\nend\n','unicode.lua':'local text="调用"\r\nfunction Unicode() end\r\n'};
const after={...before,'b.lua':'function Shared(argument)\nend\n','changed.lua':'function Changed(other)\nend\n','moved.lua':'\n\nfunction Moved()\nend\n','added.lua':'function Added()\nend\n'};delete after['removed.lua'];
const commits=[commit(before,'before'),commit(after,'after'),commit({...before,'error.lua':'function Broken(\n'},'broken')];
const runs=[];
function cli(binary,args){const run=spawnSync(binary,[...args,'--home',home,'--format','json'],{encoding:'utf8',windowsHide:true,shell:false,timeout:180000});const envelope=JSON.parse(run.stdout);runs.push({binary:binary===rust?'rust':'go',args,exit:run.status,outputSHA256:createHash('sha256').update(run.stdout).digest('hex'),envelope});return {exit:run.status,envelope};}
const pins=commits.map((exactCommit,index)=>{const path=join(stage,`pin-${index}.json`);writeFileSync(path,JSON.stringify({source:{repository:'wow-ui-source',product:'retail',requestedRef:'refs/heads/live',exactCommit,parserRevision:'source-v1'}}));const outcome=cli(rust,['target','resolve','--file',path]);if(outcome.exit!==0)throw new Error(JSON.stringify(outcome));return outcome.envelope.result.id;});
const beforeArgs=['source','diff','--from',pins[0],'--to',pins[1]],unreadyGo=cli(go,beforeArgs),unreadyRust=cli(rust,beforeArgs);
const cases=[{case:'indexRequired',goExit:unreadyGo.exit,rustExit:unreadyRust.exit,expectedFailure:5,errorEqual:isDeepStrictEqual(unreadyGo.envelope.error,unreadyRust.envelope.error)}];
for(const pin of pins)for(const binary of [go,rust]){const indexed=cli(binary,['source','index','--snapshot',pin]);if(indexed.exit!==0)throw new Error(`fixture.index_failed:${JSON.stringify(indexed)}`);}
for(const limit of ['1','50']){const args=['source','diff','--from',pins[0],'--to',pins[1],'--limit',limit],left=cli(go,args),right=cli(rust,args);const fields=['documents','declarations','documentChanges','declarationChanges','truncated'];cases.push({limit,goExit:left.exit,rustExit:right.exit,fields:fields.map(field=>({field,equal:isDeepStrictEqual(left.envelope.result?.[field],right.envelope.result?.[field])})),capturesVerified:right.envelope.captures.map(capture=>cli(rust,['evidence','verify',capture.id]).envelope.result.verified===true)});}
const sameArgs=['source','diff','--from',pins[0],'--to',pins[0]],sameGo=cli(go,sameArgs),sameRust=cli(rust,sameArgs);cases.push({case:'sameCommit',goExit:sameGo.exit,rustExit:sameRust.exit,unchanged:sameRust.envelope.result?.documentChanges===0&&sameRust.envelope.result?.declarationChanges===0});
const brokenArgs=['source','diff','--from',pins[0],'--to',pins[2]],brokenGo=cli(go,brokenArgs),brokenRust=cli(rust,brokenArgs);cases.push({case:'incompleteIndex',goExit:brokenGo.exit,rustExit:brokenRust.exit,expectedFailure:5,errorEqual:brokenGo.envelope.error.code===brokenRust.envelope.error.code&&brokenGo.envelope.error.message===brokenRust.envelope.error.message});
const report={schema:'lycheedev.rust-diff-public-parity.v1',binary:{go:{path:go,sha256:sha(go)},rust:{path:rust,sha256:sha(rust),profile:'debug'}},home,commits,pins,cases,runs,qualityState:cases.every(c=>c.expectedFailure?c.goExit===c.expectedFailure&&c.rustExit===c.expectedFailure&&c.errorEqual:c.goExit===0&&c.rustExit===0&&(c.fields?.every(f=>f.equal)??c.unchanged)&&(c.capturesVerified?.every(Boolean)??true))?'passed':'failed',fullSourceAcceptance:'not_run',performance50State:'not_run'};
writeFileSync(join(stage,'report.json'),JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify({report:join(stage,'report.json'),qualityState:report.qualityState,cases:report.cases}));if(report.qualityState!=='passed')process.exitCode=1;
