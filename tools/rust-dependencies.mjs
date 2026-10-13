// Exact lockfile source closure and notices; no npm or registry mutation.
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { createHash } from 'node:crypto';
import {runCargo} from './rust-build.mjs';
import {licensedDependencies} from './rust-license-inventory.mjs';
const root=resolve(process.cwd()),out=resolve(process.argv[2]||'.tmp/rust-dependencies');
const run=runCargo(root,['metadata','--locked','--offline','--format-version','1','--filter-platform','x86_64-pc-windows-msvc'],{owner:'rust-dependency-inventory'});
const metadata=JSON.parse(run.stdout);mkdirSync(out);
const catalog=JSON.parse(readFileSync(join(root,'release/licenses/catalog.json'))),supplements=catalog.packages;
const records=licensedDependencies(root,metadata,supplements);
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
const dataResources=(catalog.dataResources??[]).map(record=>{const source=readFileSync(join(root,record.sourcePath)),license=readFileSync(join(root,record.licensePath)),generated=readFileSync(join(root,record.generatedPath)),generator=readFileSync(join(root,record.generatorPath));if(hash(source)!==record.sourceSHA256||hash(license)!==record.licenseSHA256)throw new Error(`dependencies.data_identity_mismatch:${record.name}`);if(!generated.toString('utf8').includes(record.sourceSHA256)||!generator.toString('utf8').includes(record.sourceSHA256))throw new Error(`dependencies.data_generation_identity_mismatch:${record.name}`);return{...record,sourceBytes:source.length,generatorSHA256:hash(generator),generatedSHA256:hash(generated),licenseText:license.toString('utf8')};});
const report={schema:'lycheedev.rust-dependencies.v1',target:'x86_64-pc-windows-msvc',cargoLockSHA256:hash(readFileSync(join(root,'Cargo.lock'))),metadataEvidence:run.buildEvidence,packages:records,dataResources};
writeFileSync(join(out,'dependency-closure.json'),JSON.stringify(report,null,2)+'\n');writeFileSync(join(out,'THIRD_PARTY_NOTICES'),[...records.map(pkg=>`${pkg.name} ${pkg.version} (${pkg.license})\n${pkg.repository||pkg.source}\n\n${pkg.licenseFiles.map(file=>file.text).join('\n')}`),...dataResources.map(data=>`${data.name} ${data.version} (${data.license})\n${data.source}\nSHA256 ${data.sourceSHA256}\n\n${data.licenseText}`)].join('\n\n'+'='.repeat(72)+'\n\n'));console.log(JSON.stringify({packages:records.length,dataResources:dataResources.length,cargoLockSHA256:report.cargoLockSHA256,output:out,licenseReview:'inventory_complete_policy_review_pending'}));
