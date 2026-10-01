import {spawn} from 'node:child_process';
import {createWriteStream,readFileSync,writeFileSync,mkdirSync,existsSync} from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {assessEvidence} from './evidence-policy.mjs';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'../..');
const [mode,...rest]=process.argv.slice(2);
const options={};for(let i=0;i<rest.length;i+=2){if(!rest[i]?.startsWith('--')||rest[i+1]===undefined)throw new Error('arguments must be --name value');options[rest[i].slice(2)]=rest[i+1];}
if(!['benchmark','fixture-oracle','storage-oracle'].includes(mode)||!options.output)throw new Error('run.mjs benchmark|fixture-oracle|storage-oracle --output <new .jsonl> [--oracle exe] [--manifest fixed.json] [--benchtime 3x] [--count 1]');
const output=path.resolve(options.output);mkdirSync(path.dirname(output),{recursive:true});
if(existsSync(output+'.summary.json')||existsSync(output+'.comparison.json'))throw new Error('summary/comparison output already exists');
const env={...process.env};let args;
const count=Number(options.count??'1');
if(mode==='benchmark'){
 if(!Number.isSafeInteger(count)||count<1)throw new Error('count must be a positive integer');
 args=['test','-json','./internal/records/navigatetest','-run','^$','-bench','^BenchmarkDataPreparation$','-benchtime',options.benchtime??'3x','-count',String(count)];
}
if(mode==='fixture-oracle'){
 if(!options.oracle)throw new Error('fixture-oracle requires an explicitly built fixed oracle executable');
 env.LYCHEEDEV_CASC_ORACLE=path.resolve(options.oracle);
 args=['test','-json','./internal/records/navigatetest','-run','^TestCascLibRawFixtureOracle$','-count','1'];
}
if(mode==='storage-oracle'){
 if(!options.oracle||!options.manifest)throw new Error('storage-oracle requires oracle and fixed manifest');
 args=['run','-tags=casc_oracle','./tests/data-regression/compare','--oracle',path.resolve(options.oracle),'--manifest',path.resolve(options.manifest),'--output',output+'.comparison.json'];
}
const stream=createWriteStream(output,{flags:'wx'});
await new Promise((resolve,reject)=>{stream.once('open',resolve);stream.once('error',reject);});
const started=new Date();const child=spawn('go',args,{cwd:root,env,windowsHide:true,stdio:['ignore','pipe','pipe']});
child.stdout.pipe(stream,{end:false});child.stderr.pipe(stream,{end:false});
const exitCode=await new Promise((resolve,reject)=>{child.once('error',reject);child.once('close',resolve);});
await new Promise(resolve=>stream.end(resolve));
const text=readFileSync(output,'utf8');let comparison;
if(mode==='storage-oracle'){
 try{comparison=JSON.parse(readFileSync(output+'.comparison.json','utf8'));}catch{}
}
const assessment=assessEvidence({mode,text,exitCode,count,comparison});
const summary={schema:'lycheedev.test.data-regression.v1',mode,...assessment,exitCode,started:started.toISOString(),ended:new Date().toISOString(),command:['go',...args],platform:process.platform,arch:process.arch,evidence:output,scope:mode==='benchmark'?'synthetic-storage-real-preparation-chain':mode==='fixture-oracle'?'pinned-decoder-component':'pinned-storage-file',rss:'not_run',realClient:'not_run',performanceThreshold:'not_set'};
writeFileSync(output+'.summary.json',JSON.stringify(summary,null,2)+'\n',{flag:'wx'});
process.stdout.write(JSON.stringify(summary,null,2)+'\n');process.exitCode=summary.state==='verified'?0:1;
