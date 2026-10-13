// Keep historical business expectations intact while identifying the new rule.
import assert from 'node:assert/strict';
import {existsSync,mkdirSync,readFileSync,writeFileSync} from 'node:fs';
import {join,resolve,sep} from 'node:path';
import {hashFile} from '../../tools/rust-build.mjs';

const root=resolve('.'),out=resolve(process.argv[2]);
assert(out.startsWith(root+sep)&&!existsSync(out),'fresh versioned contract directory required');
const rulesVersion='wow-native-binding-context-v2-object-observations';
const rows=[
 ['context-native-contract.json',193],
 ['context-coverage-fixed-contract.json',12],
 ['context-object-contract.json',101],
];
const inputs=rows.map(([file,count])=>{
 const path=join(root,'tests/rust-source-engine',file),contract=JSON.parse(readFileSync(path));
 assert.equal(contract.schema,'lycheedev.emmy-native-context-contract.v1');
 assert.equal(contract.rulesVersion,'wow-native-binding-context-v1');
 assert.equal(contract.capabilityProfile,'typed-call-v1');
 assert.equal(contract.assertions.length,count);
 return {file,path,sha256:hashFile(path),count,contract};
});
mkdirSync(out);
const manifest=[];
for(const {file,path,sha256,count,contract} of inputs){
 const versioned={...contract,rulesVersion};
 const output=join(out,file);
 writeFileSync(output,JSON.stringify(versioned,null,2)+'\n',{flag:'wx'});
 const reread=JSON.parse(readFileSync(output));
 assert.deepEqual({...reread,rulesVersion:contract.rulesVersion},contract,
  'only analysis-rule identity may differ; assertions, source bytes and coverage stay fixed');
 assert.equal(hashFile(path),sha256,'historical v1 contract must stay immutable');
 manifest.push({file,originalSHA256:sha256,versionedSHA256:hashFile(output),assertions:count});
}
writeFileSync(join(out,'identity-manifest.json'),JSON.stringify({
 schema:'lycheedev.object-observation-contracts.v1',rulesVersion,
 originalRulesVersion:'wow-native-binding-context-v1',onlyRulesIdentityChanged:true,contracts:manifest,
},null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({out,rulesVersion,contracts:manifest}));
