import assert from 'node:assert/strict';
import {join} from 'node:path';
import {writeFileSync,existsSync} from 'node:fs';
import {parse} from './agent-json.mjs';
import {blob,digest} from './agent-fixture-inputs.mjs';
import {inventory} from './inputs.mjs';
export function seedAgentHome({fixture,home,ownerReceipt,scratch,importer,invoke}){const path=join(scratch,home.split(/[\\/]/).at(-1)+'-import.json');assert(!existsSync(path));const documents=fixture.documents.filter(row=>row.key.startsWith('definition/')).map(row=>{const value=parse(row.bytes);assert.equal(digest(blob(fixture.identity.home,value.blob)),value.blob.sha256);return{key:row.key,value,blob_path:join(fixture.identity.home,'blobs',value.blob.sha256.slice(0,2),value.blob.sha256.slice(2))};});writeFileSync(path,JSON.stringify({schema:'lycheedev.paired-agent-seed.recovered.v1',owner_receipt:ownerReceipt,home,fixture_home:fixture.identity.home,snapshot:fixture.identity.snapshot,pin:fixture.identity.pin,documents}),{flag:'wx'});invoke(importer.executables[0].path,['import_owned_agent_fixture','--exact','--ignored','--nocapture','--test-threads=1'],{LYCHEEDEV_AGENT_IMPORT:path});return inventory(home);}
