import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {join} from 'node:path';
import {hashFile} from '../../tools/rust-build.mjs';

export function originalV7Contract(root){
 const path=join(root,'tests/rust-source-engine/wow-v7-contract.json');
 assert.equal(hashFile(path),'80a00ac97b33435a850ac87142d60c259df1e86af404f7e4967b31b087865589','original V7 expected contract must remain fixed');
 const contract=JSON.parse(readFileSync(path));assert.equal(contract.schema,'lycheedev.wow-v7-fixed-contract.v1');
 assert.equal(contract.assertions.length,86);assert.equal(contract.fixtures.length,11);
 assert.equal(new Set(contract.assertions.map(row=>row.name)).size,86);return contract;
}
export function originalAssertionContract(assertions,contract){
 try{assert.deepEqual(assertions.map(row=>({name:row.name,expected:row.expected})),contract.assertions);return true;}catch{return false;}
}
