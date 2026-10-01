import {spawnSync} from 'node:child_process';
import {readFileSync,writeFileSync,mkdirSync,copyFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import path from 'node:path';
import {fileURLToPath} from 'node:url';

const PIN='38a34665624b8775bb875274b36191b21c38d97b';
const here=path.dirname(fileURLToPath(import.meta.url));
const [sourceArg,outArg]=process.argv.slice(2);
if(process.platform!=='win32' || !sourceArg || !outArg)throw new Error('Windows only: node build-oracle.mjs <pinned CascLib checkout> <new build directory>');
const source=path.resolve(sourceArg),out=path.resolve(outArg);
function run(tool,args,options={}){const r=spawnSync(tool,args,{encoding:'utf8',...options});if(r.status!==0)throw new Error(`${tool} failed: ${r.error??r.stderr??r.stdout}`);return r.stdout.trim();}
if(run('git',['-C',source,'rev-parse','HEAD'])!==PIN)throw new Error('CascLib commit differs from fixed audit baseline');
if(run('git',['-C',source,'status','--porcelain']))throw new Error('CascLib checkout must be clean');
mkdirSync(out,{recursive:false});
copyFileSync(path.join(source,'LICENSE'),path.join(out,'CascLib-LICENSE'));
const cmake=readFileSync(path.join(source,'CMakeLists.txt'),'utf8');
const main=cmake.match(/set\(SRC_FILES\s+([\s\S]*?)\n\)/)[1].trim().split(/\s+/);
const bundled=cmake.match(/else\(\)\s+set\(SRC_FILES \$\{SRC_FILES\}\s+([\s\S]*?)\n\s*\)/)[1].trim().split(/\s+/);
const objects=[];
const sources=[...main,...bundled,path.join(here,'oracle.cpp')];
for(const [i,file] of sources.entries()){
 const full=path.isAbsolute(file)?file:path.join(source,file),object=path.join(out,`${i}.o`);
 const compiler=file.endsWith('.c')?'gcc':'g++';
 run(compiler,['-O2','-DCASCLIB_NO_AUTO_LINK_LIBRARY','-DCASCLIB_NODEBUG','-I',path.join(source,'src'),'-c',full,'-o',object]);
 objects.push(object);
 process.stdout.write(`compiled ${i+1}/${sources.length}: ${path.basename(file)}\n`);
}
const exe=path.join(out,'casc-oracle.exe');
run('g++',[...objects,'-static-libgcc','-static-libstdc++','-lwininet','-lws2_32','-o',exe]);
writeFileSync(path.join(out,'oracle-build.json'),JSON.stringify({schema:'lycheedev.test.casc-oracle.v1',commit:PIN,executable:exe,executableSHA256:createHash('sha256').update(readFileSync(exe)).digest('hex'),adapterSHA256:createHash('sha256').update(readFileSync(path.join(here,'oracle.cpp'))).digest('hex'),license:'CascLib-LICENSE',platform:'windows-amd64'},null,2)+'\n');
process.stdout.write(`${exe}\n`);
