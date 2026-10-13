// Independent rejection-only probes. An unknown flag always remains in the
// argument list, so a bad parser cannot fall through to a valid business call.
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {appendFileSync,existsSync,mkdirSync,readFileSync,writeFileSync} from 'node:fs';
import {dirname,join,resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import {isDeepStrictEqual} from 'node:util';
import {ownedSpawnSync} from '../../tools/rust-owned-process.mjs';
const root=resolve(dirname(fileURLToPath(import.meta.url)),'../..');
const [stageArg,rustArg,mode]=process.argv.slice(2);
assert(stageArg&&rustArg,'usage: valued-flag-differential.mjs <new-stage> <sealed-rust-exe>');
const stage=resolve(stageArg),rust=resolve(rustArg),go=join(root,'.tmp/lycheedev-oracle.exe');
const sha=x=>createHash('sha256').update(x).digest('hex');
const fileHash=x=>sha(readFileSync(x));
const save=(name,x)=>writeFileSync(join(stage,name),JSON.stringify(x,null,2)+'\n');
if(mode!=='--inside-owned-job'){
  assert.equal(existsSync(stage),false,'keep prior evidence; use a new stage');mkdirSync(stage,{recursive:true});
  const run=ownedSpawnSync(process.execPath,[fileURLToPath(import.meta.url),stage,rust,'--inside-owned-job'],{
    cwd:root,evidence:stage,env:process.env,timeout:600000,orphanMs:1000});
  save('owned-process.json',{status:run.status,error:run.error?.message,handlesReaped:run.handlesReaped,proof:run.proof,logs:run.logs});
  process.stdout.write(run.stdout);process.stderr.write(run.stderr);process.exitCode=run.error?1:run.status??1;
}else{
  const identities={go:{path:go,sha256:fileHash(go)},rust:{path:rust,sha256:fileHash(rust)},scriptSHA256:fileHash(fileURLToPath(import.meta.url))};
  assert.equal(identities.go.sha256,'5738418756ee57c536518ff71223f2e7843ac3495a846b4f063d3104d83aae87');
  const home=join(stage,'isolated-home');mkdirSync(home);const env={...process.env,LYCHEEDEV_HOME:home};
  let serial=0;
  function invoke(kind,args){
    const base=`${String(++serial).padStart(4,'0')}-${kind}`;
    const p=spawnSync(kind==='go'?go:rust,args,{cwd:stage,env,encoding:'utf8',windowsHide:true,shell:false,timeout:10000,maxBuffer:2<<20});
    writeFileSync(join(stage,base+'.stdout.txt'),p.stdout||'');writeFileSync(join(stage,base+'.stderr.txt'),p.stderr||'');
    const out={args,pid:p.pid,exit:p.status,signal:p.signal,error:p.error?.message,
      stdout:p.stdout,stderr:p.stderr,stdoutFile:base+'.stdout.txt',stderrFile:base+'.stderr.txt',
      stdoutSHA256:sha(p.stdout||''),stderrSHA256:sha(p.stderr||'')};
    assert.equal(p.error,undefined,`${base} process error`);assert.equal(p.signal,null,`${base} signalled`);return out;
  }
  function comparable(p){let body;try{body={json:JSON.parse(p.stdout)};}catch{body={text:p.stdout.replaceAll('\r\n','\n')};}
    return {exit:p.exit,...body,stderr:p.stderr.replaceAll('\r\n','\n')};}
  const described=invoke('go',['describe','--format','json']);assert.equal(described.exit,0);
  const commands=JSON.parse(described.stdout).result.commands;assert.equal(commands.length,89);
  assert.deepEqual(commands,JSON.parse(readFileSync(join(root,'crates/lycheedev-cli/src/commands.json'))));
  const takesValue=flag=>{const word=flag.trim().split(/\s+/)[1];return word?.startsWith('<')||['text|json|jsonl','precise|exploratory','csv','raw|png|webp'].includes(word);};
  const manifest=commands.flatMap(command=>command.flags.filter(takesValue).map(flag=>({path:command.path,arguments:command.arguments,flag:flag.split(/\s+/)[0],description:flag}))).filter(x=>x.flag!=='--format');
  assert(manifest.length>0&&manifest.length*2<=4096,'bounded parameter inventory');save('parameter-inventory.json',manifest);
  const cases=[],progress={schema:'lycheedev.valued-flag-differential.v1',state:'running',identities,oracleCommands:89,
    parameterOccurrences:manifest.length,expectedCases:manifest.length*2,
    scope:'Every advertised valued flag except format in frozen 89-command inventory; missing value before unknown flag and empty inline value before unknown flag. Complete rejection envelope/text, stderr and exit. No valid business or game calls.',
    normalization:['JSON object member order','CRLF/LF in text and stderr only'],completedCases:0,equal:0,different:0};
  save('progress.json',progress);
  for(const item of manifest){
    const base=[...item.path.split(' '),...item.arguments.map(()=>'__parser_probe__'),'--format','json'];
    for(const [profile,tail] of [['missing-before-unknown',[item.flag,'--__rust_parity_unknown_flag']],['empty-inline-before-unknown',[item.flag+'=','--__rust_parity_unknown_flag']]]){
      const args=[...base,...tail],left=invoke('go',args),right=invoke('rust',args);
      const equal=left.exit===2&&right.exit===2&&isDeepStrictEqual(comparable(left),comparable(right));
      const record={path:item.path,flag:item.flag,profile,go:left,rust:right,bothArgumentErrors:left.exit===2&&right.exit===2,equal};
      cases.push(record);appendFileSync(join(stage,'cases.jsonl'),JSON.stringify(record)+'\n');
      progress.completedCases++;progress[equal?'equal':'different']++;
      if(progress.completedCases%16===0)save('progress.json',progress);
    }
  }
  progress.binaryStable=fileHash(go)===identities.go.sha256&&fileHash(rust)===identities.rust.sha256;
  progress.state=progress.binaryStable&&progress.different===0?'passed':'failed';
  save('progress.json',progress);save('report.json',{...progress,cases});
  console.log(JSON.stringify({state:progress.state,report:join(stage,'report.json'),cases:cases.length,equal:progress.equal,different:progress.different,parameterOccurrences:manifest.length}));
  process.exitCode=progress.state==='passed'?0:1;
}
