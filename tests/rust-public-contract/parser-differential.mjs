// Independent public-contract regression. Every probe is a parser error, never
// a valid business request. The outer invocation owns the complete Windows Job.
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {existsSync, mkdirSync, readFileSync, writeFileSync} from 'node:fs';
import {dirname, join, resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import {isDeepStrictEqual} from 'node:util';
import {ownedSpawnSync} from '../../tools/rust-owned-process.mjs';

const root=resolve(dirname(fileURLToPath(import.meta.url)),'../..');
const [stageArg,rustArg,mode]=process.argv.slice(2);
assert.ok(stageArg && rustArg, 'usage: node parser-differential.mjs <new-stage> <sealed-rust-exe>');
const stage=resolve(stageArg),rust=resolve(rustArg),go=join(root,'.tmp/lycheedev-oracle.exe');
const hash=path=>createHash('sha256').update(readFileSync(path)).digest('hex');
const save=(name,value)=>writeFileSync(join(stage,name),JSON.stringify(value,null,2)+'\n');
if(mode!=='--inside-owned-job'){
  assert.equal(existsSync(stage),false,'preserve earlier evidence; use a new stage');
  mkdirSync(stage,{recursive:true});
  const owned=ownedSpawnSync(process.execPath,[fileURLToPath(import.meta.url),stage,rust,'--inside-owned-job'],{
    cwd:root,env:process.env,evidence:stage,timeout:600000,orphanMs:1000
  });
  save('owned-process.json',{status:owned.status,error:owned.error?.message,handlesReaped:owned.handlesReaped,proof:owned.proof,logs:owned.logs});
  process.stdout.write(owned.stdout);process.stderr.write(owned.stderr);
  process.exitCode=owned.error?1:owned.status??1;
}else{
  const identities={go:{path:go,sha256:hash(go)},rust:{path:rust,sha256:hash(rust)},scriptSHA256:hash(fileURLToPath(import.meta.url))};
  assert.equal(identities.go.sha256,'5738418756ee57c536518ff71223f2e7843ac3495a846b4f063d3104d83aae87');
  const home=join(stage,'isolated-home');mkdirSync(home);
  const env={...process.env,LYCHEEDEV_HOME:home};
  let serial=0;
  function invoke(kind,args){
    const ordinal=++serial,base=`${String(ordinal).padStart(4,'0')}-${kind}`;
    const run=spawnSync(kind==='go'?go:rust,args,{cwd:stage,env,encoding:'utf8',windowsHide:true,shell:false,timeout:10000,maxBuffer:2*1024*1024});
    writeFileSync(join(stage,`${base}.stdout.txt`),run.stdout||'');
    writeFileSync(join(stage,`${base}.stderr.txt`),run.stderr||'');
    const fact={kind,args,pid:run.pid,exit:run.status,signal:run.signal,error:run.error?.message,
      stdoutFile:`${base}.stdout.txt`,stderrFile:`${base}.stderr.txt`,
      stdoutSHA256:createHash('sha256').update(run.stdout||'').digest('hex'),
      stderrSHA256:createHash('sha256').update(run.stderr||'').digest('hex')};
    save(`${base}.json`,fact);
    assert.equal(run.error,undefined,`process failed: ${base}`);
    assert.equal(run.signal,null,`process signal: ${base}`);
    return {...fact,stdout:run.stdout,stderr:run.stderr};
  }
  function comparable(run){
    let body;try{body={json:JSON.parse(run.stdout)};}catch{body={text:run.stdout.replaceAll('\r\n','\n')};}
    return {exit:run.exit,...body,stderr:run.stderr.replaceAll('\r\n','\n')};
  }
  const oracle=invoke('go',['describe','--format','json']);assert.equal(oracle.exit,0);
  const contract=JSON.parse(oracle.stdout).result.commands;assert.equal(contract.length,89);
  const imported=JSON.parse(readFileSync(join(root,'crates/lycheedev-cli/src/commands.json')));
  assert.deepEqual(imported,contract,'the complete inventory must exactly match the frozen executable');
  const advertised=invoke('rust',['describe','--format','json']);assert.equal(advertised.exit,0);
  const actual=JSON.parse(advertised.stdout).result.commands;
  const byPath=new Map(actual.map(command=>[command.path,command]));
  const publicContractDifferences=actual.filter(command=>!isDeepStrictEqual(command,contract.find(c=>c.path===command.path))).map(command=>command.path);
  const report={schema:'lycheedev.parser-differential.v1',state:'running',identities,oracleInventory:89,
    scope:'All 89 frozen command paths, three parser rejection profiles each; complete stdout envelope or text, stderr and exit. Does not prove successful business, provider, recovery, timeout, cancellation or final protocol parity.',
    normalization:['JSON object member order','CRLF vs LF in plain text/stderr only'],
    rustAdvertised:actual.length,publicContractDifferences,
    missingCommands:contract.filter(command=>!byPath.has(command.path)).map(command=>command.path),cases:[]};
  save('report.json',report);
  for(const command of contract){
    const path=[...command.path.split(' '),...command.arguments.map(()=>'__parser_probe__')];
    for(const [profile,tail] of [
      ['unknown-flag',['--format','json','--__rust_parity_unknown_flag']],
      ['missing-format-value',['--format']],
      ['invalid-format',['--format','__invalid_format__']]
    ]){
      const args=[...path,...tail];
      const left=invoke('go',args),right=invoke('rust',args);
      // A parser rejection must not silently become a successful operation.
      const rejected=left.exit===2&&right.exit===2;
      report.cases.push({path:command.path,profile,registered:byPath.has(command.path),
        go:left,rust:right,bothArgumentErrors:rejected,equal:rejected&&isDeepStrictEqual(comparable(left),comparable(right))});
      save('report.json',report);
    }
  }
  report.binaryStable=hash(go)===identities.go.sha256&&hash(rust)===identities.rust.sha256;
  report.equal=report.cases.filter(item=>item.equal).length;
  report.different=report.cases.length-report.equal;
  report.state=report.binaryStable&&report.different===0&&publicContractDifferences.length===0?'passed':'failed';
  save('report.json',report);
  console.log(JSON.stringify({report:join(stage,'report.json'),state:report.state,cases:report.cases.length,equal:report.equal,different:report.different,registered:actual.length,publicContractDifferences}));
  process.exitCode=report.state==='passed'?0:1;
}
