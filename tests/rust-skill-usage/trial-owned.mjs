import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {existsSync, readFileSync, lstatSync, readdirSync, writeFileSync} from 'node:fs';
import {join, resolve} from 'node:path';
import {hashFile, sourceDigest, verifyArtifact} from '../../tools/rust-build.mjs';
import {beginScratch, contained} from '../../tools/rust-disk.mjs';
import {ownedSpawnSync} from '../../tools/rust-owned-process.mjs';
import {currentWorkflowInputs, inventory, inventoryHash, jsonFile, loadCurrentWorkflow, sha256, within, verifyCandidate} from './assertions.mjs';
import {bounded, loadArchivedFixture, loadExporters, loadFixture} from './data-inputs.mjs';
import {findNpm} from './data-owned.mjs';
import {assertDataSkillDirectory, dataSkillIdentity, loadDataSkillContract} from './data-skill-contract.mjs';
import {PRODUCT_COMMIT, LIMITS} from './trial-policy.mjs';
const sharedTools = ['tools/rust-build.mjs','tools/rust-emmy-identity.mjs','tools/rust-disk.mjs','tools/rust-owned-process.mjs','tools/rust-owned-process.ps1','tools/rust-build-auxiliary.json'];
export function checkProduct(root, runnerRoot, artifactPath, candidatePath) {
  for (const path of sharedTools) assert.equal(hashFile(join(runnerRoot,path)), hashFile(join(root,path)), `unchanged product identity/ownership tool: ${path}`);
  const artifact = verifyArtifact(root, artifactPath), sourceTree = sourceDigest(root), workflow = loadCurrentWorkflow(runnerRoot);
  assert.equal(artifact.inputHashes.sourceTree, sourceTree, 'current matching product artifact required');
  const candidate = verifyCandidate(root, candidatePath, artifact, workflow);
  assert.equal(candidate.report.commit, PRODUCT_COMMIT, 'trial.fixed_product_commit');
  assert.equal(candidate.report.workspaceDirty,false,'trial.clean_product_report_required');
  assert.equal(candidate.report.artifactRecordSHA256, hashFile(artifactPath));
  return {artifact, sourceTree, workflow, candidate};
}
export function guideContract(directory) {
  const identities = [dataSkillIdentity(), {goContract:'3766f9cacb52e93a803a626187e2a1d54a445468',goTree:'b9e162ace7abe7ea4a2feecef822283767eb1210',candidateSourceCommit:'0002ce02bec5fb7c9c31ae40fd96eadf0da17dc6',candidateTree:'5edd025cae923fb2235086f250862696db7a2483'}];
  const actual = inventory(directory, {maxBytes:4<<20,maxFiles:100});
  const identity = identities.find(identity => JSON.stringify(loadDataSkillContract(identity).files.map(({path,bytes,sha256}) => ({path,bytes,sha256}))) === JSON.stringify(actual));
  assert(identity, 'trial.known_readonly_guide_required'); assertDataSkillDirectory(directory, identity); return identity;
}
export function verifyTrialArchive(report, evidence) {
  assert.equal(report.archiveComplete,true,'trial.archive_incomplete');
  function read(ref,max) {
    assert(ref&&typeof ref.file==='string'&&Number.isSafeInteger(ref.bytes)&&ref.bytes>=0&&ref.bytes<=max,'trial.archive_ref');
    const path=within(evidence,ref.file),bytes=bounded(path,max);assert.equal(bytes.length,ref.bytes);assert.equal(sha256(bytes),ref.sha256,'trial.archive_hash');return bytes;
  }
  assert.deepEqual(Object.keys(report.archives).sort(),['guide','home','inbox']);
  for(const [label,directory]of [['home',report.config.home],['guide',join(report.config.scratch,'guide')],['inbox',join(report.config.scratch,'inbox')]]) {
    const ref=report.archives[label];
    if(ref===null){assert(!existsSync(directory),'trial.unarchived_directory');assert.notEqual(report.state,'finished','trial.finished_archive_missing');continue;}
    const manifest=JSON.parse(read(ref,12<<20));assert.equal(manifest.directory,directory,'trial.archive_directory');assert(Array.isArray(manifest.files)&&manifest.files.length<=20000);
    const actual=inventory(directory,{maxBytes:512<<20,maxFiles:20000});assert.deepEqual(manifest.files.map(({chunks,...row})=>row),actual,'trial.archive_inventory');
    for(const row of manifest.files){assert(Array.isArray(row.chunks)&&row.chunks.length>=1&&row.chunks.length<=16);let total=0;const hash=createHash('sha256');
      for(const chunk of row.chunks){const bytes=read(chunk,8<<20);total+=bytes.length;assert(total<=row.bytes);hash.update(bytes);}
      assert.equal(total,row.bytes);assert.equal(hash.digest('hex'),row.sha256,'trial.archive_content');
    }
  }
  for(const row of report.commands){read(row.stdout,LIMITS.streamBytes+65536);read(row.stderr,LIMITS.streamBytes+65536);}
  for(const row of report.requests){const raw=read(row.archive,LIMITS.requestBytes);assert.deepEqual(JSON.parse(raw),row.request);}
  if(report.inputs)read(report.inputs,LIMITS.requestBytes);
  if(report.initialKeys)read(report.initialKeys,1024);if(report.restoredKeys)read(report.restoredKeys,1024);
  if(report.fixture){const fixture=loadArchivedFixture(evidence,report.fixture,report.config.kind),current=loadFixture(join(report.config.scratch,report.config.kind+'-fixture'),report.config.kind);assert.deepEqual(current.files,fixture.manifest.files.map(({evidence,...row})=>row),'trial.fixture_archive_identity');}
  else if(report.state==='finished')assert.equal(report.config.kind,'source','trial.finished_fixture_missing');
  return true;
}
export function retireTrial(job, {state, handlesReaped, logs, archiveComplete}) {
  job.seal({state: handlesReaped ? state : 'interrupted', handlesReaped, logs});
  // No cleanup before custody and all task bytes are sealed. A killed controller
  // can leave an incomplete archive; that scratch is explicitly retained.
  if (handlesReaped && archiveComplete) job.cleanup();
}
export function runOwnedTrial(root, argv) {
  assert.equal(process.platform,'win32'); assert.equal(process.arch,'x64');
  assert.equal(argv.length,5,'start <artifact.json> <candidate-report.json> <sealed-exporter-receipt.json|-> <guide-directory> <catalog|partial|source>');
  root = resolve(root); const runnerRoot = resolve(import.meta.dirname,'../..');
  const [artifactArg,candidateArg,exportersArg,guideArg,kind] = argv; assert(['catalog','partial','source'].includes(kind));
  const artifactPath = contained(root,resolve(artifactArg)), candidatePath = contained(root,resolve(candidateArg));
  const {artifact,sourceTree,workflow,candidate} = checkProduct(root,runnerRoot,artifactPath,candidatePath);
  const guideDirectory = resolve(guideArg), guideIdentity = guideContract(guideDirectory);
  const exporters = kind === 'source' ? null : loadExporters(root,contained(root,resolve(exportersArg)),sourceTree);
  if (kind === 'source') assert.equal(exportersArg,'-','source trial does not invoke Data exporters');
  const npm = findNpm(), names = [...sharedTools,'tools/rust-skill-trial.mjs',...readdirSync(join(runnerRoot,'tests/rust-skill-usage')).filter(n=>n.endsWith('.mjs')).map(n=>'tests/rust-skill-usage/'+n),...currentWorkflowInputs(workflow),'tests/rust-performance-paired/data-inputs.mjs','tests/rust-performance-paired/data-contract.mjs','tests/rust-performance-paired/inputs.mjs','tests/rust-performance-paired/contract.mjs','tests/rust-parity/describe-contract.mjs','tests/rust-source-full-corpus/assertions.mjs','tests/rust-source-full-corpus/retail-oracle.json','tools/zip.mjs'];
  const testInputs = Object.fromEntries([...new Set(names)].map(name=>[name,hashFile(join(runnerRoot,name))]));
  const job = beginScratch(root,'skill-data-usage',{command:process.argv,inputs:{purpose:'independent-agent-trial',productCommit:PRODUCT_COMMIT,sourceTree,runnerRoot,testInputs,guideIdentity},admissionOptions:{requiredBytes:1<<30,scratchReserveBytes:1<<30}});
  const config = {schema:'lycheedev.skill-trial-config.v1',root,runnerRoot,kind,scratch:job.directory,evidence:job.evidence,ownerId:job.id,sourceTree,productCommit:PRODUCT_COMMIT,testInputs,artifactPath,artifactSHA256:hashFile(artifactPath),binarySHA256:artifact.binarySHA256,candidatePath,candidateSHA256:candidate.reportSHA256,sourcePackage:candidate.packageRoot,packageSHA256:candidate.packageSHA256,packageFiles:candidate.files,version:candidate.manifest.version,guideDirectory,guideIdentity,exporters,npm,node:process.execPath,nodeSHA256:hashFile(process.execPath),prefix:join(job.directory,'npm-prefix'),home:join(job.directory,'home'),limits:LIMITS};
  config.launcher = join(config.prefix,'node_modules/lycheedev/payload/skill/scripts/lycheedev.mjs');
  const save = (name,value) => writeFileSync(join(job.evidence,name),JSON.stringify(value,null,2)+'\n',{flag:'wx'});
  let state = 'failed', handlesReaped = true, archiveComplete = false; const logs=[];
  try {
    save('configuration.json',config); save('candidate-identity.json',{artifact,candidateReport:candidate.report,candidateManifest:candidate.manifest});
    // This is coordinator metadata, never the blind task's prompt or oracle.
    console.log(JSON.stringify({event:'trial-owner',session:join(job.evidence,'session.json'),inputs:join(job.directory,'inputs.json')}));
    handlesReaped=false;
    const run = ownedSpawnSync(process.execPath,[join(runnerRoot,'tests/rust-skill-usage/trial-run.mjs'),join(job.evidence,'configuration.json')],{cwd:root,env:{...process.env},evidence:job.evidence,timeout:LIMITS.totalMillis+60000});
    handlesReaped=run.handlesReaped===true; logs.push(...run.logs);
    const report = existsSync(join(job.evidence,'trial-report.json')) ? jsonFile(join(job.evidence,'trial-report.json'),12<<20) : null;
    archiveComplete=false;
    let integrityError;
    try {
      const after=checkProduct(root,runnerRoot,artifactPath,candidatePath);
      assert.equal(after.sourceTree,sourceTree); assert.equal(hashFile(artifactPath),config.artifactSHA256); assert.equal(hashFile(candidatePath),config.candidateSHA256);
      assert.equal(inventoryHash(inventory(candidate.packageRoot)),config.packageSHA256);
      if(existsSync(join(config.prefix,'node_modules/lycheedev')))assert.deepEqual(inventory(join(config.prefix,'node_modules/lycheedev')),config.packageFiles,'trial.final_installed_package');
      for (const [path,hash] of Object.entries(testInputs)) assert.equal(hashFile(join(runnerRoot,path)),hash);
      assert.deepEqual(guideContract(guideDirectory),guideIdentity); assert.equal(hashFile(process.execPath),config.nodeSHA256); assert.deepEqual(findNpm(),npm);
      if (exporters) assert.deepEqual(loadExporters(root,exporters.receiptPath,sourceTree),exporters);
      assert.deepEqual(report.config,config); verifyTrialArchive(report,job.evidence);
      if(existsSync(join(config.scratch,'guide','SKILL.md')))assertDataSkillDirectory(join(config.scratch,'guide'),guideIdentity);
      archiveComplete=true;
    } catch(error) { integrityError=error.message; }
    const completed=handlesReaped&&!run.error&&run.status===0&&!integrityError&&report?.state==='finished';
    state=completed?'succeeded':'failed';
    save('owned-report.json',{schema:'lycheedev.skill-trial-owned.v1',completed,scored:false,completeSkillAcceptance:false,integrityError,productCommit:PRODUCT_COMMIT,sourceTree,runnerRoot,testInputs,report:join(job.evidence,'trial-report.json'),process:{exit:run.status,error:run.error?.message,handlesReaped,proof:run.proof}});
    console.log(JSON.stringify({completed,scored:false,report:join(job.evidence,'owned-report.json'),integrityError})); if(!completed)process.exitCode=1;
  } catch(error) { save('controller-failure.json',{error:error.stack,handlesReaped}); throw error; }
  finally {
    for(const name of readdirSync(job.evidence)){const path=join(job.evidence,name);if(name!=='receipt.json'&&lstatSync(path).isFile()&&!logs.includes(path))logs.push(path);}
    retireTrial(job,{state,handlesReaped,logs,archiveComplete});
  }
}
