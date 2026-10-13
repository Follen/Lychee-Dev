// Exercise pre-Cargo refusal paths with actual owned Node processes.
import assert from 'node:assert/strict';
import {copyFileSync,existsSync,lstatSync,mkdirSync,readFileSync,readdirSync,writeFileSync} from 'node:fs';
import {join,resolve,sep} from 'node:path';
import {hashFile} from '../../tools/rust-build.mjs';
import {ownedSpawnSync} from '../../tools/rust-owned-process.mjs';
import {inspectContext,inventory,validateAdmission,verifyWiringText} from './context-identity.mjs';
import {originalV7Contract,originalAssertionContract} from './wow-v7-contract.mjs';

const root=resolve('.'),out=resolve(process.argv[2]);
assert(out.startsWith(root+sep)&&!existsSync(out),'new evidence directory required');
const paths=['context-identity.mjs','context-build.mjs','context-native-owned.mjs','context-custody-check.mjs','wow-build.mjs','wow-owned-probe.mjs','wow-v7-contract.json','wow-v7-contract.mjs'].map(name=>`tests/rust-source-engine/${name}`);
const inputFiles=paths.map(path=>({path,sha256:hashFile(join(root,path))}));
const before=inspectContext(root);
const validation=[];
function check(id,body){try{body();validation.push({id,passed:true});}catch(error){validation.push({id,passed:false,error:String(error)});}}
const wiring=readFileSync(join(root,'crates/lycheedev-source/Cargo.toml'),'utf8');
check('actual-optional-wiring-accepted',()=>verifyWiringText(wiring));
check('comment-only-wiring-rejected',()=>assert.throws(()=>verifyWiringText(wiring.replace(/^emmylua_code_analysis = .+$/m,line=>'# '+line)),/exact optional pinned private path/));
check('default-feature-leak-rejected',()=>assert.throws(()=>verifyWiringText(wiring.replace('[features]','[features]\ndefault = ["emmy-prototype"]')),/must not enter default features/));
check('non-optional-dependency-rejected',()=>assert.throws(()=>verifyWiringText(wiring.replace(/(emmylua_code_analysis = .+)optional = true/, '$1optional = false')),/exact optional pinned private path/));
check('multiline-string-decoy-rejected',()=>assert.throws(()=>verifyWiringText(wiring.replace('[package]',"[package]\ndescription = '''\n[features]\n'''")),/forbids multiline strings/));
check('quoted-table-decoy-rejected',()=>assert.throws(()=>verifyWiringText(wiring.replace('[dependencies]','["dependencies"]')),/admitted unquoted table forms/));
const original=originalV7Contract(root);
check('actual-86-expectation-contract-accepted',()=>assert(originalAssertionContract(original.assertions,original)));
check('missing-green-assertion-rejected',()=>assert(!originalAssertionContract(original.assertions.slice(1),original)));
check('changed-expected-value-rejected',()=>{const changed=structuredClone(original.assertions);changed[0].expected=false;assert(!originalAssertionContract(changed,original));});
check('forged-out-of-scope-change-rejected',()=>assert.throws(()=>validateAdmission(root,{label:'fault',rulesVersion:'fault',changes:[{path:'Cargo.toml'}],inputs:[]}),/only admits semantic Rust changes/));
check('shared-module-omission-rejected',()=>{
 const examples=inventory(join(root,'crates/lycheedev-source/examples')).files.filter(row=>row.path.endsWith('.rs'));
 assert(examples.length>1);const omitted=`crates/lycheedev-source/examples/${examples.at(-1).path}`;
 const names=[...new Set(['Cargo.toml','Cargo.lock','crates/lycheedev-source/Cargo.toml',
  'tests/rust-source-engine/context-identity.mjs','tests/rust-source-engine/context-build.mjs',
  'tests/rust-source-engine/context-native-owned.mjs',
  'tests/rust-source-engine/wow-owned-probe.mjs','tests/rust-source-engine/wow-business-contract.mjs',
  'tests/rust-source-engine/wow-v7-contract.mjs','tests/rust-source-engine/wow-v7-contract.json',
  ...examples.map(row=>`crates/lycheedev-source/examples/${row.path}`),
  ...original.fixtures.map(row=>`tests/rust-source-engine/wow/${row.file}`)])].filter(path=>path!==omitted).sort();
 const inputs=names.map(path=>({path,bytes:lstatSync(join(root,path)).size,sha256:hashFile(join(root,path))}));
 assert.throws(()=>validateAdmission(root,{label:'fault',rulesVersion:'fault',changes:[{path:'crates/emmylua_code_analysis/src/semantic_model/fault.rs'}],inputs}),/freeze must include entire shared example closure/);
});
const evidenceRoot=join(root,'.tmp/rust-disk/evidence');
const receipts=readdirSync(evidenceRoot).sort();
mkdirSync(out);for(const row of inputFiles)copyFileSync(join(root,row.path),join(out,row.path.split('/').at(-1)));
const wrongSchema=join(out,'wrong-schema.json');writeFileSync(wrongSchema,JSON.stringify({schema:'unexpected'}));
const cases=[
 {id:'original-sealer-refuses-private-fork',script:'wow-build.mjs',args:[join(out,'forbidden-original-out'),'regressions'],message:'This original-engine sealer cannot attest a private fork'},
 {id:'missing-freeze-refused',script:'context-build.mjs',args:[join(out,'forbidden-missing-out'),join(out,'not-frozen.json'),'emmy_wow_binding_probe'],message:'ENOENT'},
 {id:'unknown-example-refused',script:'context-build.mjs',args:[join(out,'forbidden-example-out'),wrongSchema,'lycheedev'],message:'explicit isolated example required'},
 {id:'wrong-freeze-schema-refused',script:'context-build.mjs',args:[join(out,'forbidden-schema-out'),wrongSchema,'emmy_wow_binding_probe'],message:'lycheedev.emmy-context-freeze.v1'},
];
const results=[];
for(const item of cases){
 const run=ownedSpawnSync(process.execPath,[join(root,'tests/rust-source-engine',item.script),...item.args],{
  cwd:root,env:process.env,evidence:out,windowsHide:true,timeout:10000,orphanMs:5000,cargoBuild:false,
 });
 results.push({id:item.id,command:[process.execPath,...[join(root,'tests/rust-source-engine',item.script),...item.args]],
  exit:run.status,handlesReaped:run.handlesReaped,error:run.error?.message,proof:run.proof,logs:run.logs,
  expectedMessage:item.message,expectedRefusal:run.status===1&&!run.error&&run.handlesReaped&&run.stderr.toString().includes(item.message),destinationAbsent:!existsSync(item.args[0])});
}
const after=inspectContext(root);
const inputsStable=inputFiles.every(row=>hashFile(join(root,row.path))===row.sha256);
const baseStable=JSON.stringify(before.baseIdentity)===JSON.stringify(after.baseIdentity);
const noCargoReceipts=JSON.stringify(receipts)===JSON.stringify(readdirSync(evidenceRoot).sort());
const passed=inputsStable&&baseStable&&noCargoReceipts&&validation.every(row=>row.passed)&&results.every(row=>row.expectedRefusal&&row.destinationAbsent);
const report={schema:'lycheedev.emmy-context-custody-check.v1',passed,inputFiles,inputsStable,baseStable,noCargoReceipts,
 baseIdentity:before.baseIdentity,validation,results,scope:'actual pre-Cargo refusal and custody checks only; no frozen patch, compilation or semantic acceptance'};
writeFileSync(join(out,'report.json'),JSON.stringify(report,null,2)+'\n');
console.log(JSON.stringify({report:join(out,'report.json'),passed,cases:results.length,validation:validation.length,handlesReaped:results.every(row=>row.handlesReaped)}));
if(!passed)process.exitCode=1;
