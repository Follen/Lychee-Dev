// Static/checker regressions only; these successes do not claim Lua was run.
import test from 'node:test';
import assert from 'node:assert/strict';
import { appendFileSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { ownedUserEnvironment } from './rust-build.mjs';
import { SUPPORT, cleanLuaEnvironment, hash, regular, verifyCandidateArtifact, verifyInterpreter, preserveInterpreterEvidence, verifyInterpreterEvidence } from './rust-formal-lua.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const scratch = join(root, '.tmp', `formal-lua-tool-test-${process.pid}`);
function withScratch(fn) {
  rmSync(scratch, { recursive: true, force: true });
  mkdirSync(scratch, { recursive: true });
  try { return fn(scratch); } finally { rmSync(scratch, { recursive: true, force: true }); }
}
function file(path, value) { mkdirSync(dirname(path), { recursive: true }); writeFileSync(path, Buffer.isBuffer(value) || typeof value === 'string' ? value : JSON.stringify(value)); return path; }

test('owned Lua environment reuses Cargo isolation, then strips every LUA setting', () => {
  const parent = { PATH: 'system', HOME: 'foreign-home', userprofile: 'foreign-user', CODEX_HOME: 'foreign-agent',
    APPDATA: 'foreign-apps', LOCALAPPDATA: 'foreign-local', TEMP: 'foreign-temp', TMP: 'foreign-tmp',
    CARGO_HOME: 'offline-cargo', RUSTUP_HOME: 'offline-rustup', LYCHEEDEV_BIN: 'foreign-cli', LYCHEEDEV_HOME: 'foreign-workspace',
    LYCHEEDEV_LUA51: 'fixed-lua', LYCHEEDEV_FORMAL_LUA_BRIDGE_ADMISSION: 'owned-admission', LUA_INIT: 'foreign()', LUA_PATH: 'foreign/?.lua', lua_cpath: 'foreign-cpath' };
  const before = structuredClone(parent), owned = ownedUserEnvironment(scratch, parent), env = cleanLuaEnvironment(owned);
  assert.deepEqual(parent, before);
  for (const key of ['HOME', 'USERPROFILE', 'CODEX_HOME', 'APPDATA', 'LOCALAPPDATA', 'TEMP', 'TMP', 'LYCHEEDEV_HOME']) assert(owned[key].startsWith(scratch));
  for (const key of ['HOME', 'TEMP', 'APPDATA', 'LOCALAPPDATA']) assert(existsSync(owned[key]));
  assert.equal(owned.userprofile, undefined); assert.equal(owned.LYCHEEDEV_BIN, undefined);
  assert.equal(owned.LUA_PATH, 'foreign/?.lua');
  assert.equal(env.LUA_PATH, ''); assert.equal(env.LUA_CPATH, ''); assert.equal(env.LUA_INIT, undefined);
  for (const key of Object.keys(env)) assert.equal(key.toUpperCase() === 'LUA_PATH' || key.toUpperCase() === 'LUA_CPATH' || !key.toUpperCase().startsWith('LUA'), true, key);
  for (const key of ['PATH', 'CARGO_HOME', 'RUSTUP_HOME', 'LYCHEEDEV_LUA51', 'LYCHEEDEV_FORMAL_LUA_BRIDGE_ADMISSION']) assert.equal(env[key], owned[key]);
});

test('runner support is a current, self-contained tool set', () => {
  assert.ok(SUPPORT.includes('tools/rust-formal-lua.mjs'));
  assert.ok(SUPPORT.includes('tools/rust-formal-lua-bridge.mjs'));
  assert.ok(SUPPORT.includes('release/tools/luac.json'));
  for (const path of SUPPORT) {
    assert.equal(path.includes('tests/rust-parity'), false, path);
    assert.equal(path.includes('formal-lua-3.2'), false, path);
    assert.ok(existsSync(join(root, path)), path);
  }
  const formal = readFileSync(join(root, 'tools/rust-formal-lua.mjs'), 'utf8');
  for (const forbidden of ['tests/rust-parity', 'formal-lua-3.2', 'go-test-map', 'fixed Go', 'runAcceptance']) assert.equal(formal.includes(forbidden), false, forbidden);
});

test('interpreters are admitted only through the sealed reporter', () => withScratch(directory => {
  const report = file(join(directory, 'build-report.json'), { schema: 'lycheedev.formal-lua-interpreter.v1', version: '5.1.5', platform: 'windows-amd64', executable: file(join(directory, 'lua.exe'), Buffer.from('MZ fake').toString()), executableSHA256: hash(Buffer.from('MZ fake')), executableBytes: 7, source: { url: 'x', sha256: 'y', archive: join(directory, 'source.tar.gz') }, license: { path: join(directory, 'LICENSE'), sha256: 'z' }, toolchain: { family: 'msvc', version: '1.0.0', sdkVersion: '1.0.0.0' }, support: {}, imports: [], buildEvidence: join(directory, 'receipt.json') });
  for (const [name, value] of [['wrong schema', { ...JSON.parse(readFileSync(report)), schema: 'other' }], ['wrong version', { ...JSON.parse(readFileSync(report)), version: '5.4.0' }], ['missing receipt', JSON.parse(readFileSync(report))]]) {
    writeFileSync(report, JSON.stringify(value));
    assert.throws(() => verifyInterpreter(report), undefined, name);
  }
}));
test('regular files reject missing, oversized and non-regular inputs', () => withScratch(directory => {
  assert.throws(() => regular(join(directory, 'missing.bin')));
  file(join(directory, 'big.bin'), Buffer.alloc(17));
  assert.throws(() => regular(join(directory, 'big.bin'), 16), /invalid_file/);
}));
test('candidate artifacts are admitted only from this source tree', () => withScratch(directory => {
  const artifact = file(join(directory, 'artifact.json'), { schema: 'lycheedev.rust-build-artifact.v1' });
  assert.throws(() => verifyCandidateArtifact(root, artifact));
}));
test('hash and regular helpers are byte exact', () => withScratch(directory => {
  const path = file(join(directory, 'bytes.bin'), Buffer.from('abc'));
  assert.equal(hash(readFileSync(path)), 'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad');
  assert.deepEqual(regular(path), Buffer.from('abc'));
}));

test('portable interpreter evidence preserves original bytes and verifies without historical paths',()=>withScratch(directory=>{
 const original=join(directory,'original-owner'),output=join(directory,'runtime');mkdirSync(original);mkdirSync(output);
 const report={schema:'lycheedev.formal-lua-interpreter.v1',version:'5.1.5',platform:'windows-amd64',buildEvidence:join(output,'evidence/receipt.json')},bytes=Buffer.from(JSON.stringify(report)+'\n');
 const logs=[['interpreter-report.json',bytes],['process.stdout.bin',Buffer.from('original sealed compiler log')]];
 const receipt={schema:'lycheedev.disk-owner.v1',owner:'formal-lua-build',category:'scratch',state:'retired',handlesReaped:true,activePid:null,sealedLogs:logs.map(([name,value])=>({path:join(original,name),sha256:hash(value)}))};
 for(const[name,value]of logs)file(join(original,name),value);
 const originalReceipt=Buffer.from(JSON.stringify(receipt,null,2)+'\n');file(join(original,'receipt.json'),originalReceipt);
 preserveInterpreterEvidence(join(original,'receipt.json'),output);
 assert.deepEqual(readFileSync(report.buildEvidence),originalReceipt);
 for(const[name,value]of logs)assert.deepEqual(readFileSync(join(output,'evidence',name)),value);
 rmSync(original,{recursive:true});
 assert.equal(verifyInterpreterEvidence(report,bytes).buildReceiptSHA256,hash(originalReceipt));
 appendFileSync(join(output,'evidence/process.stdout.bin'),'changed');
 assert.throws(()=>verifyInterpreterEvidence(report,bytes),/sealed_log_corrupt/);
}));
