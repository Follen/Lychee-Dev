import test from 'node:test';
import assert from 'node:assert/strict';
import {existsSync, mkdirSync, readFileSync, writeFileSync} from 'node:fs';
import {spawnSync} from 'node:child_process';
import {createHash, randomUUID} from 'node:crypto';
import {join} from 'node:path';
import {fileURLToPath} from 'node:url';

const windows = {skip: process.platform !== 'win32'};
const wrapper = fileURLToPath(new URL('./rust-owned-process.ps1', import.meta.url));
const source = readFileSync(wrapper, 'utf8');

function fixture(t, label, {legacyName, v2 = false} = {}) {
  const evidence = join(process.cwd(), '.tmp', `atomic-job-${label}-${randomUUID()}`);
  mkdirSync(evidence, {recursive: true});
  t.diagnostic(`Preserved atomic Job evidence: ${evidence}`);
  const id = randomUUID(), base = join(evidence, `process-${id}`);
  const path = legacyName ? join(evidence, legacyName) : base + '.request.json';
  const marker = join(evidence, 'root-ran');
  const request = {
    command: process.execPath, args: ['-e', `require('fs').writeFileSync(${JSON.stringify(marker)},'ran');process.stdout.write('legacy')`],
    cwd: process.cwd(), timeoutMs: 10000, orphanMs: 1000, cargoBuild: false,
    stdout: base + '.stdout.bin', stderr: base + '.stderr.bin', receipt: base + '.receipt.json',
  };
  if (v2) Object.assign(request, {schema: 'lycheedev.owned-request.v2', runId: id,
    identity: base + '.identity.json', rootIdentity: base + '.root.json'});
  return {evidence, id, base, path, marker, request};
}

function run(f) {
  writeFileSync(f.path, JSON.stringify(f.request));
  const original = readFileSync(f.path);
  const execution = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass',
    '-File', wrapper, '-Request', f.path], {
    encoding: 'utf8', windowsHide: true, shell: false, timeout: 60000, maxBuffer: 1 << 20,
  });
  writeFileSync(join(f.evidence, 'wrapper-result.json'), JSON.stringify({status: execution.status,
    signal: execution.signal, error: execution.error?.message, stdout: execution.stdout, stderr: execution.stderr}, null, 2));
  assert.equal(execution.error, undefined);
  assert.deepEqual(readFileSync(f.path), original, 'legacy request bytes must remain unchanged');
  return execution;
}

function proof(f) { return JSON.parse(readFileSync(f.request.receipt)); }

function startupHashBlock(input = source) {
  const start = input.indexOf('$wrapperFile=[IO.File]::Open(');
  const end = input.indexOf('\n# Startup identity is evidence,', start);
  assert(start >= 0 && end > start, 'extract the actual production wrapper hash block');
  return input.slice(start, end);
}

function assertStartupHashBeforeIdentity(input) {
  const block = startupHashBlock(input);
  const identity = input.indexOf('$wrapperIdentity=@{');
  assert(identity >= 0, 'find the actual startup wrapper identity declaration');
  assert(input.indexOf(block) + block.length <= identity, 'complete the wrapper hash before constructing startup identity');
  const serialization = input.indexOf('\n$identityBytes=', identity);
  assert(serialization > identity, 'find startup identity serialization after its declaration');
  assert.match(input.slice(identity, serialization), /wrapperObservedSHA256=\$wrapperObservedSHA256;/);
}

test('startup wrapper identity uses the existing .NET stream hash without Security module autoload', () => {
  const block = startupHashBlock();
  assert.doesNotMatch(source, /Get-FileHash/);
  assert.match(block, /\[Security\.Cryptography\.SHA256\]::Create\(\)/);
  assert.match(block, /ComputeHash\(\$wrapperFile\)/);
  assert.match(block, /finally\{if\(\$wrapperDigest\)\{\$wrapperDigest\.Dispose\(\)\};\$wrapperFile\.Dispose\(\)\}/);
  assertStartupHashBeforeIdentity(source);
  assert.doesNotMatch(block, /PSModulePath|PSModuleAutoLoadingPreference|Import-Module|SetEnvironmentVariable/);
});

test('startup hash ordering is checkout-line-ending independent and rejects missing identity or late hashing', () => {
  const lf = source.replaceAll('\r\n', '\n');
  for (const input of [lf, lf.replaceAll('\n', '\r\n')]) {
    assertStartupHashBeforeIdentity(input);
    assert.throws(() => assertStartupHashBeforeIdentity(input.replace('$wrapperIdentity=@{', '$otherIdentity=@{')),
      /find the actual startup wrapper identity declaration/);
    const block = startupHashBlock(input);
    const lateHash = input.replace(block, '') + '\n' + block + '\n# Startup identity is evidence,';
    assert.throws(() => assertStartupHashBeforeIdentity(lateHash), /complete the wrapper hash before constructing startup identity/);
    assert.throws(() => assertStartupHashBeforeIdentity(input.replace('wrapperObservedSHA256=$wrapperObservedSHA256;', 'wrapperObservedSHA256="uncomputed";')),
      /wrapperObservedSHA256/);
  }
});

test('Windows hashes the actual startup block in an empty runspace with no Get-FileHash or module autoload', windows, t => {
  const f = fixture(t, 'hash-no-modules'), script = join(f.evidence, 'hash-no-modules.ps1');
  const binary = join(f.evidence, 'raw hash [literal].bin'), empty = join(f.evidence, 'empty.bin');
  writeFileSync(binary, Buffer.from([0xef, 0xbb, 0xbf, 0, 0xff, 0x80, 10, 13, 0x61]));
  writeFileSync(empty, Buffer.alloc(0));
  const inputs = [wrapper, binary, empty];
  const literal = value => "'" + value.replaceAll("'", "''") + "'";
  writeFileSync(script, '\uFEFF' + `$ErrorActionPreference='Stop'
# A fresh runspace has no cmdlets or imported modules. Disable module autoload
# only inside this isolated fixture; the production process environment is untouched.
$initial=[System.Management.Automation.Runspaces.InitialSessionState]::Create()
$initial.LanguageMode=[System.Management.Automation.PSLanguageMode]::FullLanguage
$initial.Variables.Add([System.Management.Automation.Runspaces.SessionStateVariableEntry]::new('PSModuleAutoLoadingPreference','None','fixture isolation'))
$runspace=[System.Management.Automation.Runspaces.RunspaceFactory]::CreateRunspace($initial)
$probe=$null;$hash=$null
try{
 $runspace.Open()
 $probe=[System.Management.Automation.PowerShell]::Create();$probe.Runspace=$runspace
 [void]$probe.AddScript(@'
$ErrorActionPreference='Stop'
try{Get-FileHash;'unexpected-command'}catch [System.Management.Automation.CommandNotFoundException]{'missing-command'}
'@)
 $absent=@($probe.Invoke())
 if($absent.Count -ne 1 -or [string]$absent[0] -cne 'missing-command'){throw 'hash_cmdlet_not_absent'}
 foreach($path in @(${inputs.map(literal).join(',')})){
  $hash=[System.Management.Automation.PowerShell]::Create();$hash.Runspace=$runspace
  [void]$hash.AddScript(@'
param([string]$path)
$ErrorActionPreference='Stop'
if($PSModuleAutoLoadingPreference -cne 'None'){throw 'hash_module_autoload_enabled'}
$PSCommandPath=$path
${startupHashBlock()}
# Exclusive reopen also proves that the extracted block released its file handle.
$released=[IO.File]::Open($path,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::None)
$released.Dispose()
$wrapperObservedSHA256
'@).AddArgument($path)
  $values=@($hash.Invoke())
  if($hash.HadErrors -or $values.Count -ne 1){throw ('hash_block_failed: '+[string]$hash.Streams.Error)}
  [Console]::Out.WriteLine([string]$values[0])
  $hash.Dispose();$hash=$null
 }
}finally{if($hash){$hash.Dispose()};if($probe){$probe.Dispose()};$runspace.Dispose()}
`);
  const execution = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', script],
    {encoding: 'utf8', windowsHide: true, shell: false, timeout: 30000, maxBuffer: 1 << 20});
  writeFileSync(join(f.evidence, 'wrapper-result.json'), JSON.stringify({status: execution.status, error: execution.error?.message,
    stdout: execution.stdout, stderr: execution.stderr}, null, 2));
  assert.equal(execution.error, undefined); assert.equal(execution.status, 0, execution.stderr);
  assert.deepEqual(execution.stdout.trim().split(/\r?\n/), inputs.map(path => createHash('sha256').update(readFileSync(path)).digest('hex')));
});

test('atomic startup retains the handle list and persists exact root before Resume', () => {
  assert.match(source, /InitializeProcThreadAttributeList\(attributes,2,/);
  assert.match(source, /new IntPtr\(0x20002\)/);
  assert.match(source, /new IntPtr\(0x2000D\)/);
  assert.ok(source.indexOf('owned.atomic_job_attribute') < source.indexOf('if(!CreateProcessW'));
  assert.ok(source.indexOf('Persist(rootIdentity,rootJson)') < source.indexOf('if(ResumeThread'));
  assert.doesNotMatch(source, /AssignProcessToJobObject/);
  assert.match(source, /file\.Flush\(true\)/);
  assert.match(source, /assigned=true; \/\/ A successful JOB_LIST/);
});

test('unversioned direct consumers keep caller-chosen basenames and generated identity is explicit', () => {
  assert.match(source, /Existing direct PS1 consumers use unversioned requests/);
  assert.match(source, /\$null -ne \$requestData.schema -and/);
  assert.match(source, /\$requestData\|Add-Member -NotePropertyMembers/);
  assert.match(source, /else\{\$runId=\[Guid\]::NewGuid\(\)\.ToString\('D'\);\$identitySource='legacy-generated'\}/);
  assert.match(source, /identitySource=\$identitySource;requestPath=\$requestPath/);
  assert.match(source, /startupIdentity=\[string\]\$requestData.identity;rootIdentity=/);
  assert.match(source, /owned.start_identity_run/);
  assert.match(source, /owned.start_identity_path/);
});

for (const [label, options, expectedSource] of [
  ['legacy-uuid', {}, 'legacy-request-basename'],
  ['legacy-name', {legacyName: 'request.json'}, 'legacy-generated'],
  ['v2', {v2: true}, 'request-v2'],
]) test(`Windows ${label} executes through atomic Job creation with exact independent startup identity`, windows, t => {
  const f = fixture(t, label, options), execution = run(f);
  assert.equal(execution.status, 0, execution.stderr);
  assert.equal(readFileSync(f.request.stdout, 'utf8'), 'legacy');
  const p = proof(f), root = JSON.parse(readFileSync(p.rootIdentity)), start = JSON.parse(readFileSync(p.startupIdentity));
  for (const key of ['rootReaped', 'treeReaped', 'handlesClosed', 'outputReadersComplete']) assert.equal(p[key], true);
  assert.equal(root.jobAssignedAtCreation, true);
  assert.equal(root.pid, p.pid);
  assert.equal(root.creationFileTime, p.creationFileTime);
  assert.equal(start.runId, p.runId);
  if (options.legacyName) assert.notEqual(start.runId, f.id);
  else assert.equal(start.runId, f.id);
  assert.equal(start.identitySource, expectedSource);
  assert.equal(p.identitySource, expectedSource);
  assert.equal(start.requestPath, f.path);
  assert.equal(start.requestSHA256, createHash('sha256').update(readFileSync(f.path)).digest('hex'));
  assert.equal(start.wrapperObservedSHA256, createHash('sha256').update(readFileSync(wrapper)).digest('hex'));
  assert.equal(start.historicalReapProof, false);
  assert.equal(root.historicalReapProof, false);
  assert.equal(start.bootClock, 'QueryUnbiasedInterruptTime');
  assert.match(start.wrapperCreationFileTime, /^[1-9]\d*$/);
  assert.match(start.bootUptime100ns, /^\d+$/);
});

test('Windows explicit v2 conflicting path cannot create a root or overwrite unrelated evidence', windows, t => {
  const f = fixture(t, 'invalid-path', {v2: true}), foreign = join(f.evidence, 'foreign.json');
  writeFileSync(foreign, 'preserve');
  f.request.identity = foreign;
  const execution = run(f);
  assert.notEqual(execution.status, 0);
  assert.match(execution.stderr, /owned.start_identity_path/);
  assert.equal(readFileSync(foreign, 'utf8'), 'preserve');
  assert.equal(existsSync(f.marker), false);
  assert.equal(existsSync(f.base + '.root.json'), false);
  assert.equal(existsSync(f.request.receipt), false);
});

test('Windows startup identity CreateNew failure leaves existing bytes and starts no root', windows, t => {
  const f = fixture(t, 'start-failure', {v2: true});
  writeFileSync(f.request.identity, 'original startup evidence');
  const execution = run(f);
  assert.notEqual(execution.status, 0);
  assert.equal(readFileSync(f.request.identity, 'utf8'), 'original startup evidence');
  assert.equal(existsSync(f.marker), false);
  assert.equal(existsSync(f.request.rootIdentity), false);
  assert.equal(existsSync(f.request.receipt), false);
});

test('Windows root identity CreateNew failure never resumes and reaps exact already-owned root', windows, t => {
  const f = fixture(t, 'root-failure', {v2: true});
  writeFileSync(f.request.rootIdentity, 'original root evidence');
  const execution = run(f);
  assert.notEqual(execution.status, 0);
  assert.equal(readFileSync(f.request.rootIdentity, 'utf8'), 'original root evidence');
  assert.equal(existsSync(f.marker), false, 'no user code before durable exact root evidence');
  const p = proof(f);
  assert.ok(p.pid > 0);
  assert.match(p.creationFileTime, /^[1-9]\d*$/);
  assert.equal(p.rootReaped, true);
  assert.equal(p.treeReaped, true);
  assert.equal(p.activeProcesses, 0);
  assert.equal(p.handlesClosed, true);
  assert.equal(p.forced, true);
  assert.equal(p.outputReadersComplete, false, 'unstarted readers do not become a successful run');
  assert.match(p.error, /System\.IO\.IOException/);
});

test('sidecar array values group path concatenation before the PowerShell comma operator', () => {
  assert.match(source, /@\('identity',\(\$expectedStem\+'\.identity\.json'\)\)/);
  assert.match(source, /@\('rootIdentity',\(\$expectedStem\+'\.root\.json'\)\)/);
});

test('Windows evaluates the exact production sidecar array and admission block before launching any root', windows, t => {
  const f = fixture(t, 'path-expression');
  const start = source.indexOf('foreach($entry in @('), end = source.indexOf('\nif($requestData.schema', start);
  assert(start >= 0 && end > start);
  const block = source.slice(start, end), expression = block.match(/^foreach\(\$entry in (.*)\)\{/)[1];
  const script = join(f.evidence, 'path-expression.ps1');
  writeFileSync(script, '\uFEFF' + `$ErrorActionPreference='Stop'
$expectedStem=Join-Path $PSScriptRoot 'process-00000000-0000-4000-8000-000000000001'
$entries=${expression}
if($entries.Count -ne 2 -or $entries[0].Count -ne 2 -or $entries[1].Count -ne 2){throw 'sidecar_array_shape'}
if($entries[0][1] -cne ($expectedStem+'.identity.json') -or $entries[1][1] -cne ($expectedStem+'.root.json')){throw 'sidecar_array_precedence'}
function Test-ActualAdmission($requestData){
${block}
}
$good=[pscustomobject]@{schema='lycheedev.owned-request.v2';identity=($expectedStem+'.identity.json');rootIdentity=($expectedStem+'.root.json')}
Test-ActualAdmission $good
Test-ActualAdmission ([pscustomobject]@{})
foreach($field in @('identity','rootIdentity')){
 $bad=[pscustomobject]@{schema='lycheedev.owned-request.v2';identity=$good.identity;rootIdentity=$good.rootIdentity}
 $bad.$field=Join-Path $PSScriptRoot 'foreign.json'
 $rejected=$false;try{Test-ActualAdmission $bad}catch{if($_.Exception.Message -cne 'owned.start_identity_path'){throw};$rejected=$true}
 if(-not $rejected){throw 'conflicting_sidecar_accepted'}
}
@{passed=$true;cases=5;scope='actual PowerShell evaluation of unchanged production block; no root process or Cargo'}|ConvertTo-Json -Compress
`);
  const execution = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', script],
    {encoding: 'utf8', windowsHide: true, shell: false, timeout: 30000, maxBuffer: 1 << 20});
  writeFileSync(join(f.evidence, 'wrapper-result.json'), JSON.stringify({status: execution.status, error: execution.error?.message, stdout: execution.stdout, stderr: execution.stderr}, null, 2));
  assert.equal(execution.error, undefined); assert.equal(execution.status, 0, execution.stderr);
  assert.equal(JSON.parse(execution.stdout.replace(/^\uFEFF/, '')).passed, true);
});

// PowerShell try/foreach blocks share script scope. Cargo admission used to
// replace the startup identity with a helper file identity before Run's call.
test('startup wrapper binding is independent of Cargo helper admission variables',()=>{
 assert.match(source,/\$wrapperIdentity=@\{schema='lycheedev.owned-start.v1'/);
 assert.match(source,/GetBytes\(\(\$wrapperIdentity\|ConvertTo-Json -Depth 4\)\)/);
 assert.match(source,/\[int\]\$wrapperIdentity.wrapperPid,\[string\]\$wrapperIdentity.wrapperCreationFileTime/);
 assert.equal((source.match(/\$wrapperIdentity\s*=/g)||[]).length,1,'only startup creates the wrapper binding');
 const cargo=source.slice(source.indexOf('}elseif($requestData.cargoBuild'),source.indexOf('# Process creation is suspended'));
 assert.match(cargo,/\$helperIdentity=Open-IdentityFile \$helperPath/);
 assert.doesNotMatch(cargo,/\$identity\b/);
});

test('Windows Cargo helper admission retains identical startup job and root wrapper binding',windows,t=>{
 const policy=JSON.parse(readFileSync(new URL('./rust-build-auxiliary.json',import.meta.url)));
 const programFiles=Object.entries(process.env).find(([key])=>key.toLowerCase()==='programfiles')?.[1];
 if(!programFiles||!policy.helpers.some(row=>existsSync(join(programFiles,row.programFilesPath))))return t.skip('reviewed installed Cargo helper required');
 const f=fixture(t,'cargo-helper-binding',{v2:true});f.request.command='cargo';f.request.args=['test','--help'];f.request.cargoBuild=true;
 const result=run(f);assert.equal(result.status,0,result.stderr);const terminal=proof(f);
 const cargoSHA256=createHash('sha256').update(readFileSync(terminal.binary)).digest('hex');
 const admissionEvidence=JSON.stringify({helperAdmission:terminal.helperAdmission,cargo:{path:terminal.binary,sha256:cargoSHA256,reviewedSHA256:policy.cargoSHA256}});
 t.diagnostic(admissionEvidence);
 assert.equal(terminal.helperAdmission.state,'enabled',admissionEvidence);assert.equal(terminal.handlesClosed,true);assert.equal(terminal.treeReaped,true);
 const start=JSON.parse(readFileSync(f.request.identity)),job=JSON.parse(readFileSync(f.base+'.job.json')),root=JSON.parse(readFileSync(f.request.rootIdentity));
 for(const sidecar of [job,root]){assert.equal(sidecar.wrapperPid,start.wrapperPid);assert.equal(sidecar.wrapperCreationFileTime,start.wrapperCreationFileTime);assert.equal(sidecar.runId,start.runId);assert.equal(sidecar.requestSHA256,start.requestSHA256);}
});
