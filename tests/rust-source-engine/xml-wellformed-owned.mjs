// Independent negative fixtures against a sealed XML extractor, not an XSD validator.
import assert from 'node:assert/strict';
import {resolve,join} from 'node:path';
import {readFileSync,writeFileSync,mkdirSync,existsSync,copyFileSync} from 'node:fs';
import {spawnSync} from 'node:child_process';
import {randomUUID} from 'node:crypto';
import {hashFile} from '../../tools/rust-build.mjs';
const root=resolve('.'),out=resolve(process.argv[2]??'.tmp/rust-emmy-xml-wellformed-red-01');
const sealed=resolve(process.argv[3]??'.tmp/rust-emmy-xml-01');
const identity=JSON.parse(readFileSync(join(sealed,'artifact.json')));
const suite=process.argv[4]??'basic';assert(['basic','declaration'].includes(suite));
const cases=suite==='basic'?[['attribute_raw_lt','<Ui a="<"/>'],['text_cdata_close','<Ui>]]></Ui>'],['tail_bom','<Ui/>\uFEFF'],['tail_nbsp','<Ui/>\u00A0'],['invalid_element_name','<Ui><1bad/></Ui>'],['invalid_attribute_name','<Ui 1bad="x"/>']]:[
 ['attribute_missing_separator','<Ui a="1"b="2"/>'],
 ['declaration_standalone_invalid','<?xml version="1.0" standalone="maybe"?><Ui/>'],
 ['declaration_unknown_field','<?xml version="1.0" other="x"?><Ui/>'],
 ['declaration_duplicate_version','<?xml version="1.0" version="1.0"?><Ui/>'],
 ['declaration_field_order','<?xml version="1.0" standalone="yes" encoding="UTF-8"?><Ui/>'],
 ['declaration_invalid_tail','<?xml version="1.0" encoding="UTF-8" broken?><Ui/>'],
];
const exe=join(out,'emmy_xml_program.exe');
if(!process.argv.includes('--worker')){
 assert(out.startsWith(root+'\\')&&!existsSync(out));mkdirSync(out);
 assert.equal(hashFile(identity.binary),identity.binarySHA256);assert.equal(hashFile(identity.pdb),identity.pdbSHA256);
 copyFileSync(identity.binary,exe);copyFileSync(identity.pdb,exe.replace(/\.exe$/,'.pdb'));
 for(const [name,text]of cases)writeFileSync(join(out,name+'.xml'),Buffer.from(text));
 const template=readFileSync(join(root,'tools/rust-owned-process.ps1'),'utf8'),anchor='var limits=new LIMITS();limits.basic.flags=0x2000;Set(job,9,limits);';assert.equal(template.split(anchor).length,2);
 const wrapper=join(out,'owned-xml-negative-2gib.ps1');writeFileSync(wrapper,template.replace(anchor,'var limits=new LIMITS();limits.basic.flags=0x2000|0x200;limits.jobMemory=new UIntPtr(2147483648UL);Set(job,9,limits);'));
 writeFileSync(join(out,'artifact.json'),JSON.stringify({...identity,binary:exe,pdb:exe.replace(/\.exe$/,'.pdb'),runnerSHA256:hashFile(import.meta.filename),wrapperSHA256:hashFile(wrapper)},null,2));
 writeFileSync(join(out,'runner-snapshot.mjs'),readFileSync(import.meta.filename));
 const base=join(out,'process-'+randomUUID()),spec={command:process.execPath,args:[import.meta.filename,out,sealed,suite,'--worker'],cwd:root,timeoutMs:15000,orphanMs:5000,cargoBuild:false,stdout:base+'.stdout.txt',stderr:base+'.stderr.txt',receipt:base+'.receipt.json'};
 writeFileSync(base+'.request.json',JSON.stringify(spec,null,2));
 const run=spawnSync('powershell.exe',['-NoProfile','-NonInteractive','-File',wrapper,'-Request',base+'.request.json'],{cwd:root,windowsHide:true,timeout:45000,maxBuffer:1<<20});
 let worker,proof,error;try{worker=JSON.parse(readFileSync(join(out,'worker.json')));proof=JSON.parse(readFileSync(spec.receipt));}catch(e){error=String(e);}
 const reaped=run.status===0&&proof?.rootReaped&&proof.treeReaped&&proof.handlesClosed&&proof.activeProcesses===0;
 const report={schema:'lycheedev.xml-wellformed-owned.v1',suite,passed:reaped&&!error&&proof.exitCode===0&&worker?.passed===true,worker,proof,error,handlesReaped:reaped,scope:'fixed XML 1.0 well-formedness counterexamples; invalid content must be rejected transactionally; no full XSD or semantic acceptance'};
 assert.equal(hashFile(exe),identity.binarySHA256);
 writeFileSync(join(out,'report.json'),JSON.stringify(report,null,2));console.log(JSON.stringify({report:join(out,'report.json'),passed:report.passed,cases:worker?.records.length,failed:worker?.records.filter(r=>!r.passed).length,handlesReaped:reaped}));if(!report.passed)process.exitCode=1;
}else{
 const records=[];
 for(const [name]of cases){
  const file=join(out,name+'.xml'),run=spawnSync(exe,['extract',file],{cwd:root,windowsHide:true,timeout:2000,maxBuffer:8<<20});
  writeFileSync(join(out,name+'.stdout.json'),run.stdout??'');writeFileSync(join(out,name+'.stderr.txt'),run.stderr??'');
  let value,error;try{value=JSON.parse(run.stdout);}catch(e){error=String(e);}
  records.push({name,inputSHA256:hashFile(file),actualExit:run.status,value,error,launchError:run.error?.message,expected:'exit1/state failed/factsExported=false',passed:!error&&!run.error&&run.status===1&&value?.state==='failed'&&value.factsExported===false});
 }
 const report={records,passed:records.every(r=>r.passed)};writeFileSync(join(out,'worker.json'),JSON.stringify(report,null,2));if(!report.passed)process.exitCode=1;
}
