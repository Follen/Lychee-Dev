// Only the outer existing Windows Job may run this controller. Every child
// (exporter, npm, installed Skill, package launcher and CLI) stays in that Job.
import assert from 'node:assert/strict';
import {agentEnvironment,environmentHash} from './agent-environment.mjs';
import {spawnSync} from 'node:child_process';
import {existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync} from 'node:fs';
import {dirname, join, resolve, delimiter} from 'node:path';
import {performance} from 'node:perf_hooks';
import {AgentClient, parseAgentJson} from '../../tools/rust-agent-client.mjs';
import {assertHelp, assertCommandSchema, assertSearch, assertDiscoveryNoHome} from './agent-discovery.mjs';
import {DatabaseSync} from 'node:sqlite';
import {hashFile} from '../../tools/rust-build.mjs';
import {CURRENT_GO_CONTRACT, inventory, inventoryHash, jsonFile, loadCurrentWorkflow, pendingIncrementTasks, within} from './assertions.mjs';
import {archiveFixture, bounded, digest, loadFixture, npmOfflineFlags, MISSING_KEYS, RESTORED_KEYS} from './agent-data-inputs.mjs';
import {PHASES, archiveBytes, assertAgentDataReport, assertEnvelope, queryFor, assertPage, assertCSV, assertCaptureBody, assertAgentCapture, assertAnalyzePage, assertAgentResources, assertAgentAnalyze, assertSourceIdentity} from './agent-data-assertions.mjs';
import {assertDataSkillDirectory, assertDataSkillInventory, dataSkillIdentity} from './data-skill-contract.mjs';

assert.equal(process.platform, 'win32'); assert.equal(process.arch, 'x64');
const [configPath, ...extra] = process.argv.slice(2); assert(configPath && !extra.length);
const config = jsonFile(configPath), {scratch, evidence, root} = config;
assert.equal(resolve('.'), root); assert.equal(config.node, process.execPath); assert.equal(config.platform, process.platform);
const owner = jsonFile(join(evidence, 'receipt.json'));
assert.equal(owner.owner, 'skill-data-usage'); assert.equal(owner.state, 'active'); assert.equal(owner.directory, scratch);
assert.equal(config.ownerId, owner.id); assert.equal(configPath, join(evidence, 'configuration.json'));
assert.deepEqual(readdirSync(scratch), []); assert.equal(inventoryHash(inventory(config.sourcePackage)), config.packageSHA256);
assert.equal(config.runnerRoot, resolve(import.meta.dirname, '../..'));
const workflow = loadCurrentWorkflow(config.runnerRoot);
assert.deepEqual(config.skillIdentity, dataSkillIdentity());
const report = {schema: 'lycheedev.skill-agent-data-usage.v1', goContract: CURRENT_GO_CONTRACT, skillTree: dataSkillIdentity().candidateTree, state: 'running', completeSkillAcceptance: false, config, startedAt: new Date().toISOString(), checks: [], commands: [], discovery: [], sessions: [], fixtures: {}, seeded: {}, imports: {}, workspaces: {}, queries: {}, keys: {}, proofs: {}, completedTasks: [], pendingTasks: pendingIncrementTasks(workflow), boundaries: ['Synthetic authenticated local CASC inputs; no actual client observation.', 'Logical memoryBudget is actual product output, not RSS or OS Job pressure evidence.', 'Pressure recovery and all Live tasks remain NOT_RUN.', 'New revision: prior lost-file Node results are not inherited.']};
const save = () => writeFileSync(join(evidence, 'report.json'), JSON.stringify(report, null, 2) + '\n');
save();
const deadline = performance.now() + 570000;
let fatal = false, totalBytes = 0;
const env=agentEnvironment(config);assert.equal(environmentHash(env),config.environmentSHA256);
for (const path of [env.HOME, env.TEMP]) mkdirSync(path);
writeFileSync(join(scratch, 'user.npmrc'), '', {flag: 'wx'}); writeFileSync(join(scratch, 'global.npmrc'), '', {flag: 'wx'});
function command(id, executable, args, {cwd = scratch, kind = 'public', environment = {}} = {}) {
  assert(!fatal, 'prior process failure requires outer Job retirement');
  const remaining = Math.floor(deadline - performance.now());
  if (remaining < 1000 || report.commands.length >= 80 || totalBytes >= 128 << 20) { fatal = true; throw new Error('skill-data.total_controller_budget'); }
  const began = performance.now(); const run = spawnSync(executable, args, {cwd, env: {...env, ...environment}, shell: false, windowsHide: true, timeout: Math.min(120000, remaining), maxBuffer: 12 << 20});
  const stdout = run.stdout ?? Buffer.alloc(0), stderr = run.stderr ?? Buffer.alloc(0); totalBytes += stdout.length + stderr.length;
  const row = {id, executable, args, cwd, kind, environment, exit: run.status, signal: run.signal, error: run.error?.message, wallMillis: performance.now() - began, stdout: archiveBytes(evidence, `${String(report.commands.length).padStart(3, '0')}-${id}.stdout.bin`, stdout), stderr: archiveBytes(evidence, `${String(report.commands.length).padStart(3, '0')}-${id}.stderr.bin`, stderr)};
  report.commands.push(row); save();
  if (run.error || run.signal || totalBytes > 128 << 20) fatal = true;
  assert.equal(run.error, undefined); assert.equal(run.signal, null); assert.equal(run.status, 0, `${id}: ${stderr.toString().slice(0, 400)}`);
  return stdout;
}
function invoke(id, args, kind = 'catalog') {
  const home = join(scratch, `${kind}-home`), raw = command(id, process.execPath, [config.launcher, ...args, ...(['cli','schema'].includes(args[0])?[]:['--home',home]), '--format', 'json'], {environment: {LYCHEEDEV_HOME: home}});
  const envelope = JSON.parse(raw); assertEnvelope(envelope); return envelope;
}
function check(caseId, work) {
  try { const detail = work() ?? {}; report.checks.push({caseId, state: 'passed', ...detail}); save(); }
  catch (error) { report.checks.push({caseId, state: 'failed', reason: error.message}); save(); throw error; }
}
const fixtures = {};
function initialize(kind) {
 const home=join(scratch,`${kind}-home`);assert(!existsSync(home));invoke(`${kind}-init`,['init'],kind);
 report.workspaces[kind]=archiveBytes(evidence,`${kind}-workspace.json`,bounded(join(home,'workspace.json')));
 const fixture=fixtures[kind], request={schema:'lycheedev.paired-agent-seed.recovered.v1',owner_receipt:join(evidence,'receipt.json'),home,fixture_home:fixture.identity.home,snapshot:fixture.identity.snapshot,pin:fixture.identity.pin,documents:fixture.documents.filter(row=>row.key.startsWith('definition/')).map(row=>{const value=parseAgentJson(row.bytes);return{key:row.key,value,blob_path:join(fixture.identity.home,'blobs',value.blob.sha256.slice(0,2),value.blob.sha256.slice(2))};})};
 const requestPath=join(scratch,`${kind}-import.json`);const importBytes=Buffer.from(JSON.stringify(request));writeFileSync(requestPath,importBytes,{flag:'wx'});report.imports[kind]=archiveBytes(evidence,`${kind}-import.json`,importBytes);
 const importer=config.exporters.executables.find(row=>row.name==='agent_fixture_import');assert(importer);assert.equal(hashFile(importer.path,128<<20),importer.sha256);
 command(`${kind}-import`,importer.path,['import_owned_agent_fixture','--exact','--ignored','--nocapture'],{cwd:root,kind:'importer',environment:{LYCHEEDEV_AGENT_IMPORT:requestPath}});
 const seeded=[];
 for(const [dbPath,prefix] of [['state/toolkit.sqlite','pin/'],['state/data/cache-v1.sqlite','definition/']]) {
  const db=new DatabaseSync(join(home,dbPath),{readOnly:true});try{for(const row of db.prepare('SELECT key,generation,value FROM documents ORDER BY key').all()){assert(row.key.startsWith(prefix));seeded.push({key:row.key,generation:row.generation,valueBase64:Buffer.from(row.value).toString('base64')});}}finally{db.close();}
 }
 assert.deepEqual(seeded.sort((a,b)=>a.key.localeCompare(b.key)),fixture.documents.map(row=>({key:row.key,generation:1,valueBase64:row.bytes.toString('base64')})).sort((a,b)=>a.key.localeCompare(b.key)));
 report.seeded[kind]=archiveBytes(evidence,`${kind}-seeded.json`,Buffer.from(JSON.stringify(seeded)));save();
}
async function session(phase) {
 const kind=phase==='catalog'?'catalog':'partial', fixture=fixtures[kind],home=join(scratch,`${kind}-home`), query=queryFor(phase);
 const args=[config.launcher,'data','agent','serve','--home',home,'--snapshot',fixture.identity.snapshot,'--installation',fixture.identity.installation,'--format','jsonl'];
 if(kind==='partial')args.push('--key-file',join(scratch,'sample.keys'));
 const observation={phase,executable:process.execPath,args,cwd:scratch,environmentSHA256:environmentHash(env),frames:[]};report.sessions.push(observation);save();
 const client=new AgentClient(process.execPath,args,{cwd:scratch,env,deadline,onFrame:row=>{const raw=archiveBytes(evidence,`agent-${phase}-${observation.frames.length}.json`,row.raw);observation.frames.push({request:row.request,frame:raw});save();}});
 const discovery=await client.success({task:'discover',table:{name:kind==='catalog'?'HouseDecor':'Sample',mode:'static'}});assert.equal(discovery.rowCount,phase==='catalog'?3:phase==='partial'?1:2);
 const result=await client.success({task:'query',query});assert.equal(result.complete,phase!=='partial');assertAgentResources(result.provenance);assert.equal(result.provenance.sources.length,1);assertSourceIdentity(result.provenance.sources[0],phase,fixture);
 const handle=result.result.handle;assert.equal(typeof handle,'string');
 assertPage(await client.success({task:'page',handle,offset:0,limit:100}),phase);
 assertCSV(await client.success({task:'export',handle,offset:0,limit:100,format:'csv'}),phase);
 const capture=await client.success({task:'capture',handle});const raw=bounded(join(home,'blobs',capture.blob.sha256.slice(0,2),capture.blob.sha256.slice(2)),16<<20);
 assertCaptureBody(parseAgentJson(raw),phase,query,result.provenance);
 const db=new DatabaseSync(join(home,'state/data/evidence-v2.sqlite'),{readOnly:true});let metadata;
 try{const row=db.prepare('SELECT key,generation,length(CAST(value AS BLOB)) AS bytes,CASE WHEN length(CAST(value AS BLOB))<=1048576 THEN CAST(value AS BLOB) ELSE NULL END AS value FROM documents WHERE key=?').get('capture/'+capture.id);assert(row&&row.bytes<=1048576&&row.value);metadata={database:'state/data/evidence-v2.sqlite',key:row.key,generation:row.generation,valueBase64:Buffer.from(row.value).toString('base64')};}finally{db.close();}
 assertAgentCapture(capture,raw,metadata,fixture.identity.snapshot,query);
 observation.captureBody=archiveBytes(evidence,`agent-${phase}-capture.json`,raw);observation.captureMetadata=archiveBytes(evidence,`agent-${phase}-capture-metadata.json`,Buffer.from(JSON.stringify(metadata)));
 invoke(`agent-${phase}-verify`,['evidence','verify',capture.id],kind);
 await client.success({task:'release',handle});
 if(phase==='catalog') {const analyzed=await client.success({task:'explain',query,verbose:true,analyze:true});assertAgentAnalyze(analyzed.provenance,query);assertAnalyzePage(await client.success({task:'page',handle:analyzed.result.handle,offset:0,limit:100}));await client.success({task:'release',handle:analyzed.result.handle});}
 const closed=await client.close();observation.process=closed.process;save();
}

try {
  for (const [index, kind] of ['catalog', 'partial'].entries()) check(`export-${kind}`, () => {
    const path = join(scratch, `${kind}-fixture`); assert(!existsSync(path));
    const exporter = config.exporters.executables[index]; assert.equal(hashFile(exporter.path, 128 << 20), exporter.sha256);
    command(`export-${kind}`, exporter.path, [kind === 'catalog' ? 'prepare_catalog_public_fixture' : 'prepare_skill_partial_fixture', '--exact', '--ignored', '--nocapture'], {cwd: root, kind: 'exporter', environment: kind === 'catalog' ? {LYCHEEDEV_CATALOG_FIXTURE: path} : {LYCHEEDEV_SKILL_PARTIAL_FIXTURE: path, LYCHEEDEV_SKILL_OWNER_RECEIPT: join(evidence, 'receipt.json')}});
    fixtures[kind] = loadFixture(path, kind); report.fixtures[kind] = archiveFixture(fixtures[kind], kind, evidence);
  });
  check('npm-installed-package', () => {
    assert.equal(hashFile(config.npm.path), config.npm.sha256);
    const raw = command('npm-pack', process.execPath, [config.npm.path, 'pack', config.sourcePackage, '--json', '--pack-destination', scratch, ...npmOfflineFlags(scratch)], {kind: 'npm'});
    const packed = JSON.parse(raw); assert.equal(packed.length, 1); assert.equal(packed[0].filename, `lycheedev-${config.version}.tgz`);
    const tgz = bounded(config.tgz, 128 << 20); assert.equal(digest(tgz, 'sha1'), packed[0].shasum); assert.equal('sha512-' + Buffer.from(digest(tgz, 'sha512'), 'hex').toString('base64'), packed[0].integrity);
    report.tgz = {path: config.tgz, bytes: tgz.length, sha256: digest(tgz)};
    command('npm-install', process.execPath, [config.npm.path, 'install', '--global', '--prefix', config.prefix, config.tgz, ...npmOfflineFlags(scratch)], {kind: 'npm'});
    const installed = inventory(join(config.prefix, 'node_modules/lycheedev')); assert.deepEqual(installed, config.packageFiles); assert.equal(inventoryHash(installed), config.packageSHA256);
    report.installedInventory = archiveBytes(evidence, 'installed-inventory.json', Buffer.from(JSON.stringify(installed)));
    report.npmShim = archiveBytes(evidence, 'npm-lycheedev.cmd', bounded(join(config.prefix, 'lycheedev.cmd')));
    assert(!existsSync(join(config.prefix, 'lycheedev.exe')), 'npm prefix must choose installed package launcher');
    const skillRoot = join(config.prefix, 'node_modules/lycheedev/payload/skill');
    const contract = assertDataSkillDirectory(skillRoot);
    assertDataSkillInventory(installed);
    report.skillProof = {...dataSkillIdentity(contract), files: contract.files.map((row, index) => ({path: row.path,
      archive: archiveBytes(evidence, `installed-skill-${index}.bin`, bounded(within(skillRoot, row.path)))}))};
  });
  for(const [id,args] of [['version',['version']],['describe',['describe']],['help',['--help']],['agent-schema',['data','agent','schema']],['agent-help',['data','agent','serve','--help']],['agent-command-schema',['schema','data','agent','serve']],['agent-search',['cli','search','data agent serve']],['empty-search',['cli','search','zz-no-command-7b95']]]) {
   const paths=[join(scratch,'discovery-home'),join(env.USERPROFILE,'.lycheedev')],before=paths.map(path=>existsSync(path));assert.deepEqual(before,[false,false]);const result=invoke(id,args,'discovery').result;const after=paths.map(path=>existsSync(path));assert.deepEqual(after,[false,false]);report.discovery.push({id,paths,before,after});save();
   if(id==='help')assertHelp(result);if(id==='agent-help')assertHelp(result,'data agent serve');if(id==='agent-command-schema')assertCommandSchema(result,'data agent serve');if(id.endsWith('search'))assertSearch(result,args.at(-1),id==='agent-search'?'data agent serve':null);
  }
  for(const kind of ['catalog','partial']) initialize(kind);
  writeFileSync(join(scratch,'sample.keys'),MISSING_KEYS,{flag:'wx'});report.keys.missing=archiveBytes(evidence,'sample-missing.keys',bounded(join(scratch,'sample.keys')));
  await session('catalog');await session('partial');
  assert.equal(readFileSync(join(scratch,'sample.keys'),'utf8'),MISSING_KEYS);writeFileSync(join(scratch,'sample.keys'),RESTORED_KEYS);report.keys.restored=archiveBytes(evidence,'sample-restored.keys',bounded(join(scratch,'sample.keys')));await session('restored');
  for (const kind of ['catalog', 'partial']) assert.deepEqual(inventory(fixtures[kind].directory, {maxFiles: 256, maxBytes: 64 << 20}), fixtures[kind].files, 'fixture inputs unchanged');
  assert.equal(inventoryHash(inventory(config.sourcePackage)), config.packageSHA256); assert.equal(hashFile(config.tgz, 128 << 20), report.tgz.sha256);
  report.state = 'passed_selected_agent_data_workflow'; report.pendingTasks = pendingIncrementTasks(workflow).filter(row=>row.taskId!=='g323-data-query'); report.completedTasks = [{taskId: 'g323-data-query', runtimeVerified: true, basis: 'actual_owned_npm_installed_agent_and_independent_full_capture_replay', captures: 3}];
  assertAgentDataReport(report, workflow, {evidenceDirectory:evidence}); report.endedAt = new Date().toISOString(); save();
} catch (error) {
  report.state = 'failed'; report.completedTasks = []; report.pendingTasks = pendingIncrementTasks(workflow); report.error = error.stack;

  report.endedAt = new Date().toISOString(); save(); process.exitCode = 1;
}
