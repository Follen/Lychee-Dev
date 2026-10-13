#!/usr/bin/env node
// Rust candidate launcher: integrity verification and native process handoff only.
import { createHash,randomBytes } from 'node:crypto';
import { existsSync,readFileSync, realpathSync, statSync } from 'node:fs';
import { dirname, isAbsolute, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
export function nativePath(root, platform=process.platform, arch=process.arch) {
  if(platform!=='win32'||arch!=='x64')throw new Error(`distribution.unsupported_platform: ${platform}/${arch}`);
  const manifest=JSON.parse(readFileSync(resolve(root,'release.json'))),pkg=JSON.parse(readFileSync(resolve(root,'package.json'))),entry=manifest.binaries?.['windows-amd64'];
  if(manifest.schema!=='lycheedev.release.v1'||manifest.version!==pkg.version)throw new Error('distribution.version_mismatch');
  if(!entry||entry.path!=='native/windows-amd64/lycheedev.exe'||!/^[a-f0-9]{64}$/.test(entry.sha256)||!Number.isSafeInteger(entry.bytes)||entry.bytes<1||entry.bytes>256*1024*1024)throw new Error('distribution.invalid_binary_record');
  const binary=realpathSync(resolve(root,entry.path)),rel=relative(realpathSync(root),binary);if(isAbsolute(rel)||rel==='..'||rel.startsWith(`..${sep}`))throw new Error('distribution.binary_outside_package');
  const stat=statSync(binary);if(!stat.isFile()||stat.size!==entry.bytes||stat.size>256*1024*1024)throw new Error('distribution.binary_size');
  const bytes=readFileSync(binary);if(bytes.length!==entry.bytes||createHash('sha256').update(bytes).digest('hex')!==entry.sha256)throw new Error('distribution.binary_integrity');return binary;
}
export function launch(root,args,run=spawnSync){
  let index=0;while(index<args.length){if(args[index]==='--home'||args[index]==='--format')index+=2;else if(args[index].startsWith('--home=')||args[index].startsWith('--format='))index++;else break;}
  const managed=args[index]==='update'&&!args.some(arg=>['--help','-h','--plan','--release'].includes(arg)||arg.startsWith('--release='));
  const nonce=managed?randomBytes(32).toString('hex'):'';
  let result=run(nativePath(root),args,{stdio:managed?['inherit','pipe','pipe']:'inherit',encoding:managed?'utf8':undefined,maxBuffer:16*1024*1024,shell:false,windowsHide:true,env:managed?{...process.env,LYCHEEDEV_NODE_EXE:process.execPath,LYCHEEDEV_NPM_NONCE:nonce}:process.env});
  if(result.error)throw result.error;
  if(managed){const prefix=resolve(root,'../..'),handoff=resolve(prefix,'.lycheedev-update',`handoff-${nonce}.json`);
    if(result.status===6&&existsSync(handoff)){const stat=statSync(handoff);if(!stat.isFile()||stat.size>1024*1024)throw new Error('distribution.invalid_handoff');const record=JSON.parse(readFileSync(handoff,'utf8'));const staged=resolve(prefix,'.lycheedev-update/staging/node_modules/lycheedev');if(record.schema!=='lycheedev.npm-update.rust.v1'||record.nonce!==nonce||realpathSync(record.staged)!==realpathSync(staged))throw new Error('distribution.invalid_handoff');const helper=nativePath(staged);if(createHash('sha256').update(readFileSync(helper)).digest('hex')!==record.helperSha256)throw new Error('distribution.invalid_handoff_helper');result=run(helper,['__lycheedev_update_apply',handoff,nonce],{stdio:'inherit',shell:false,windowsHide:true});if(result.error)throw result.error;}
    else{if(result.stdout)process.stdout.write(result.stdout);if(result.stderr)process.stderr.write(result.stderr);}}
  if(result.signal)return{signal:result.signal};if(!Number.isInteger(result.status))throw new Error('distribution.missing_exit_status');return{status:result.status};
}
export function checkNodeVersion(version=process.versions.node){const[major,minor]=version.split('.').map(Number);if(major<22||(major===22&&minor<14))throw new Error('distribution.node_version: requires Node >=22.14.0');}
if(process.argv[1]&&realpathSync(resolve(process.argv[1]))===fileURLToPath(import.meta.url))try{checkNodeVersion();const result=launch(resolve(dirname(fileURLToPath(import.meta.url)),'..'),process.argv.slice(2));if(result.signal)process.kill(process.pid,result.signal);else process.exitCode=result.status;}catch(error){process.stderr.write(`lycheedev: ${error.message}\n`);process.exitCode=3;}
