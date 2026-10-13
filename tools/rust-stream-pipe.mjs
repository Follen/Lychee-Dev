// Real closed-consumer test against one explicit immutable CLI artifact.
import {spawn} from 'node:child_process';
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import {resolve,join} from 'node:path';
import {createHash} from 'node:crypto';
const [binaryArg,homeArg,pin,installation,outArg]=process.argv.slice(2);
if(!binaryArg||!homeArg||!/^PIN-[a-f0-9]{64}$/.test(pin??'')||!installation||!outArg)throw Error('usage: <exact-binary> <task-home> <pin> <read-only-installation> <new-report-directory>');
const binary=resolve(binaryArg),home=resolve(homeArg),out=resolve(outArg);mkdirSync(out);
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');const before=hash(readFileSync(binary));
const args=['data','db2','stream','Map','--snapshot',pin,'--installation',installation,'--limit','1184','--offline','--format','jsonl','--home',home];
const child=spawn(binary,args,{shell:false,windowsHide:true,stdio:['ignore','pipe','pipe']});
let first='',stderr='',closed=false,forced=false;
const timer=setTimeout(()=>{forced=true;child.kill();},15000);
child.stdout.on('data',chunk=>{first+=chunk.toString('utf8');if(!closed&&first.includes('\n')){closed=true;child.stdout.destroy();}});
child.stderr.on('data',chunk=>{stderr+=chunk.toString('utf8');if(stderr.length>65536){forced=true;child.kill();}});
const result=await new Promise((ok,bad)=>{child.once('error',bad);child.once('close',(code,signal)=>ok({code,signal}));});clearTimeout(timer);
const frames=first.split('\n').filter(Boolean).flatMap(line=>{try{return[JSON.parse(line)];}catch{return[];}});
const report={schema:'lycheedev.public-typed-stream-pipe.v1',binary:{path:binary,sha256:before,sha256After:hash(readFileSync(binary))},home,pin,args,pid:child.pid,consumerClosedAfterFirstFrame:closed,forcedFixtureCleanup:forced,...result,receivedFrames:frames,stderr,state:closed&&!forced&&result.code===5&&!frames.some(frame=>frame.frame==='end'||frame.type==='end')?'passed':'failed_or_not_run'};
writeFileSync(join(out,'report.json'),JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify({state:report.state,code:result.code,forcedFixtureCleanup:forced,receivedFrames:frames.length,report:join(out,'report.json')}));if(report.state!=='passed')process.exitCode=1;
