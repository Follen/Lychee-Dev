// Separate production-static-SQL lane. No DataFusion route, builds or downloads.
import assert from 'node:assert/strict';
import {existsSync, lstatSync, writeFileSync} from 'node:fs';
import {isAbsolute, join, resolve} from 'node:path';
import {beginScratch} from './rust-disk.mjs';
import {ownedSpawnSync} from './rust-owned-process.mjs';
import {hashFile, sourceDigest, verifyArtifact} from './rust-build.mjs';
import {generationMode, generationProfileEnvironment} from '../tests/rust-performance-paired/generation-profile.mjs';
import {verifyGenerationArtifact} from '../tests/rust-performance-paired/generation-contract.mjs';
import {inspectEmmy} from './rust-emmy-identity.mjs';
import {verifyGenerationCompletion} from '../tests/rust-performance-paired/generation-evidence.mjs';
import {verifyGoPackage} from '../tests/rust-performance-paired/inputs.mjs';
import {bounded, parse} from '../tests/rust-performance-paired/data-inputs.mjs';
import {CSV_RUNNER_FILES, assertCSVPolicy, csvEnvironment, evidenceFiles, loadCSVPreparation, validateCSVConditions} from '../tests/rust-performance-paired/data-csv-diagnostics.mjs';

const [requestPath, conditionsPath, mode, ...extra] = process.argv.slice(2);
assert(requestPath && conditionsPath && ['--observation', '--acceptance', '--profile-observation'].includes(mode) && !extra.length,
  'usage: node tools/rust-data-generation-paired.mjs <generation-config.json> <conditions.json> --observation|--acceptance|--profile-observation');
assert.equal(process.platform, 'win32');
const request = parse(bounded(requestPath, 16384)), conditions = validateCSVConditions(parse(bounded(conditionsPath, 16384)));
const selected=generationMode(request,mode),{profiled}=selected;
for(const key of ['ownerRoot','artifact','goSource','goPackage','luaLSArchive'])assert(isAbsolute(request[key]),`absolute ${key} required`);
assert.deepEqual(Object.keys(request.fixturePreparation).sort(),['report','reportSHA256','sourceTreeSHA256'].sort());assert(isAbsolute(request.fixturePreparation.report));
for(const key of ['reportSHA256','sourceTreeSHA256'])assert.match(request.fixturePreparation[key],/^[0-9a-f]{64}$/);
const root = resolve(request.ownerRoot), runnerRoot = resolve(import.meta.dirname, '..');
const env = generationProfileEnvironment(csvEnvironment(process.env)); assertCSVPolicy(root);
const paths = [...new Set([...CSV_RUNNER_FILES, 'tests/rust-performance-paired/data-csv-diagnostics.mjs',
  'tools/rust-data-generation-paired.mjs', 'tests/rust-performance-paired/generation-run.mjs', 'tests/rust-performance-paired/generation-contract.mjs', 'tests/rust-performance-paired/generation-evidence.mjs', 'tests/rust-performance-paired/generation-profile.mjs', 'tests/rust-performance-paired/data-phase-profile.mjs'])];
const inputs = Object.fromEntries(paths.map(p => [p, hashFile(join(runnerRoot,p))]));
const rust = verifyArtifact(root, request.artifact), go = verifyGoPackage(request.goPackage, request.luaLSArchive);
verifyGenerationArtifact(rust,{sourceRoot:root,sourceTree:sourceDigest(root),binarySHA256:rust.binarySHA256,emmy:inspectEmmy(root),channel:parse(bounded(join(root,'release/npm-channel.json')))});assert.equal(lstatSync(rust.sourceBinary).size,rust.bytes);
const artifactSHA256=hashFile(request.artifact,65536);
const producer = loadCSVPreparation(request.fixturePreparation).provenance;
const job = beginScratch(root, profiled?'paired-data-generation-profile':'paired-data-generation', {command:process.argv, inputs:{profiled,product:selected.product,artifactSHA256,rust:rust.binarySHA256,go:go.packageSHA256,files:inputs,producer}, admissionOptions:{requiredBytes:1<<30,scratchReserveBytes:1<<30}});
let state='failed', handlesReaped=true, report;
try {
  const configuration=join(job.evidence,'configuration.json');
  writeFileSync(configuration,JSON.stringify({request,conditions,root,runnerRoot,inputs,rust,go,artifactSHA256,producer,profiled,product:selected.product,acceptance:mode==='--acceptance',scratch:job.directory,evidence:job.evidence},null,2)+'\n');
  handlesReaped=false;
  const run=ownedSpawnSync(process.execPath,[join(runnerRoot,'tests/rust-performance-paired/generation-run.mjs'),configuration],{cwd:root,env,evidence:job.evidence,timeout:18000000});
  handlesReaped=run.handlesReaped===true;
  const observation=join(job.evidence,'report.json');
  let observed=null,stable=false,failure;
  try {observed=existsSync(observation)?parse(bounded(observation,32<<20)):null;stable=hashFile(request.artifact,65536)===artifactSHA256 && verifyArtifact(root,request.artifact).binarySHA256===rust.binarySHA256 && sourceDigest(root)===rust.inputHashes.sourceTree && verifyGoPackage(request.goPackage,request.luaLSArchive).packageSHA256===go.packageSHA256 && paths.every(p=>hashFile(join(runnerRoot,p))===inputs[p]);
  assert.deepEqual(loadCSVPreparation(request.fixturePreparation).provenance,producer);
  if(observed){assert.deepEqual(observed.inputs,inputs);assert.deepEqual(observed.producer,producer);verifyGenerationCompletion(run,observed,mode==='--acceptance',{profiled,product:selected.product});}
  }catch(error){failure=error.message.slice(0,4096);}
  const expected=profiled?'equivalent_profiled_generation_observation_only':mode==='--acceptance'?'passed_selected_generation_paths':'equivalent_generation_observation_only';
  const passed=!failure && stable && handlesReaped && !run.error && run.status===0 && observed?.state===expected && observed.acceptance===(mode==='--acceptance');
  report={schema:profiled?'lycheedev.paired-generation-profile-owned.v1':'lycheedev.paired-generation-owned.v1',profiled,product:selected.product,state:passed?expected:'failed',fullB04Acceptance:false,observation,inputs,rust,go,producer,failure,artifactAndRunnerStable:stable,process:{exit:run.status,error:run.error?.message,handlesReaped,proof:run.proof}};
  writeFileSync(join(job.evidence,'owned-report.json'),JSON.stringify(report,null,2)+'\n');
  state=passed?'succeeded':'failed'; if(!passed)process.exitCode=1;
} finally {
  job.seal({state:handlesReaped?state:'interrupted',handlesReaped,logs:evidenceFiles(job.evidence)});
  if(handlesReaped && state==='succeeded')job.cleanup();
}
console.log(JSON.stringify({state:report.state,report:join(job.evidence,'owned-report.json'),owner:job.receipt},null,2));
