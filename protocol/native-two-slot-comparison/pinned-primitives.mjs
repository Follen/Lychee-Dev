// Read-only extraction of already pinned official source blobs. No fetch,
// compiler, client, OS input, or claim about native implementation semantics.
import {spawnSync} from 'node:child_process';
import {createHash,randomUUID} from 'node:crypto';
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import {join} from 'node:path';
import assert from 'node:assert/strict';
const root=process.cwd(),pinsPath=join(root,'.tmp/rust-source-matrix-four-01/prepared-pins.json');
const mirror=join(root,'.tmp/rust-source-matrix-four-01/home/mirrors/wow-ui-source.git');
const evidence=join(root,'.tmp/rust-supervision',`two-slot-pinned-primitives-${randomUUID()}`);mkdirSync(evidence,{recursive:true});
const sha=x=>createHash('sha256').update(x).digest('hex'),pinsBytes=readFileSync(pinsPath),pins=JSON.parse(pinsBytes),calls=[],blobs=[];
writeFileSync(join(evidence,'pins.json'),pinsBytes,{flag:'wx'});
writeFileSync(join(evidence,'extractor.mjs'),readFileSync(new URL(import.meta.url)),{flag:'wx'});
const names=['OsDocumentation.lua','SystemTimeDocumentation.lua','FrameScriptDocumentation.lua','SimpleEditBoxAPIDocumentation.lua','SimpleFrameAPIDocumentation.lua'];
const fixed={retail:'31c7f7b9cc79e56c986b365c06a6afbcf3c9177b',classic:'a53b9b28857fd11184573f8dda6a746a789abc0e',titan:'0f73fd6b0e12bf8de89a8084ebd81aade0afe8d7',forever:'15666a6e67938a1ab5caf041406464251db111ca'};
function git(args){const result=spawnSync('git',[`--git-dir=${mirror}`,...args],{cwd:root,timeout:5000,maxBuffer:1<<20,windowsHide:true});calls.push({args,status:result.status,error:result.error?.message,stderr:String(result.stderr||''),bytes:result.stdout?.length||0});assert.equal(result.status,0,JSON.stringify(calls.at(-1)));return result.stdout;}
try{
  for(const target of pins.targets){const product=target.product,commit=target.pin.source.exactCommit;assert.equal(commit,fixed[product]);const dir=join(evidence,product);mkdirSync(dir);
    for(const name of names){const path=`Interface/AddOns/Blizzard_APIDocumentationGenerated/${name}`,spec=`${commit}:${path}`;
      const expected=git(['rev-parse',spec]).toString('ascii').trim(),bytes=git(['show',spec]);
      const gitSHA1=createHash('sha1').update(Buffer.from(`blob ${bytes.length}\0`)).update(bytes).digest('hex');assert.equal(gitSHA1,expected);
      const sealed=join(dir,name);writeFileSync(sealed,bytes,{flag:'wx'});assert.equal(sha(readFileSync(sealed)),sha(bytes));
      const text=bytes.toString('utf8'),lines=text.split(/\r?\n/),declarations=[];
      for(const [i,line]of lines.entries())if(/Name = "(GetTimePreciseSec|GetTime|GetServerTime|GetSessionTime|GetTickTime|debugprofilestart|debugprofilestop|SetMaxBytes|SetFocus|ClearFocus|HasFocus|EnableKeyboard|SetPropagateKeyboardInput)"/.test(line))declarations.push({line:i+1,text:line.trim()});
      blobs.push({product,commit,path,gitSHA1,sha256:sha(bytes),bytes:bytes.length,sealed,declarations});
    }
  }
  const result={schema:'lycheedev.protocol-comparison.pinned-primitives.v1',state:'sealed_pinned_declarations_only',pinsSHA256:sha(pinsBytes),extractorSHA256:sha(readFileSync(new URL(import.meta.url))),blobs,calls,
    facts:{getTimePreciseSecDeclaredInFourPins:true,debugprofileStopDefinedSinceLastDebugprofileStart:true,editBoxMaxBytesAndFocusDeclaredInFourPins:true,frameEnableKeyboardProtected:true},
    unproven:['native clock monotonicity and progress across process/OS pause','host QPC clock mapping and drift/reset bounds','GetTime/GetTimePreciseSec actual client implementation','actual keyboard text framing/focus/release in all clients','actual bit.bxor availability/unsigned semantics and crypto throughput','runtime signing key isolation'],finalProtocolPassed:false};
  writeFileSync(join(evidence,'result.json'),JSON.stringify(result,null,2)+'\n',{flag:'wx'});console.log(JSON.stringify({state:result.state,evidence,blobs:blobs.length}));
}catch(error){writeFileSync(join(evidence,'failure.json'),JSON.stringify({state:'failed',error:error.stack,calls,blobs,finalProtocolPassed:false},null,2)+'\n',{flag:'wx'});console.error(JSON.stringify({state:'failed',evidence,error:error.message}));process.exitCode=1;}
