import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { peImports, verifySystemImports, stageLuac } from './luac.mjs';
function image(name, machine=0x8664) {
  const bytes=Buffer.alloc(1024);bytes.write('MZ');bytes.writeUInt32LE(128,60);bytes.write('PE\0\0',128);bytes.writeUInt16LE(machine,132);bytes.writeUInt16LE(1,134);bytes.writeUInt16LE(240,148);bytes.writeUInt16LE(0x20b,152);bytes.writeUInt32LE(0x1000,152+120);const section=392;bytes.writeUInt32LE(512,section+8);bytes.writeUInt32LE(0x1000,section+12);bytes.writeUInt32LE(512,section+16);bytes.writeUInt32LE(512,section+20);bytes.writeUInt32LE(0x1060,512+12);bytes.write(name,608);return bytes;
}
test('PE inspection rejects non-x64 and external CRT runtime dependencies',()=>{assert.deepEqual(peImports(image('KERNEL32.dll')),['kernel32.dll']);assert.deepEqual(verifySystemImports(image('KERNEL32.dll')),['kernel32.dll']);assert.throws(()=>verifySystemImports(image('VCRUNTIME140.dll')),/external_runtime_dependency/);assert.throws(()=>peImports(image('KERNEL32.dll',0x14c)),/not_amd64/);assert.throws(()=>peImports(Buffer.alloc(4)),/pe_invalid/);});
test('luac staging authenticates pinned manifest before copying executable',t=>{const root=mkdtempSync(join(tmpdir(),'lycheedev-luac-test-'));t.after(()=>rmSync(root,{recursive:true}));const runtime=join(root,'runtime');mkdirSync(runtime);writeFileSync(join(runtime,'runtime.json'),'{}');assert.throws(()=>stageLuac(join(root,'package'),[],runtime),/runtime_identity_mismatch/);});
test('pinned compiler record binds source, license and only system import',()=>{const manifest=JSON.parse(readFileSync(new URL('../release/tools/luac.json',import.meta.url)));assert.equal(manifest.runtime.schema,'lycheedev.luac-runtime.v1');assert.equal(manifest.runtime.platform,'windows-amd64');assert.equal(manifest.source.sha256,'2640fc56a795f29d28ef15e13c34a47e223960b0240e8cb0a82d9b0738695333');assert.deepEqual(manifest.imports,['kernel32.dll']);assert(manifest.arguments.includes('/MT'));assert.match(manifest.runtime.licenseSHA256,/^[a-f0-9]{64}$/);});

import {fixedToolchain, parseBuildOptions, selectCompiler} from './luac.mjs';
const fixed = {family: 'msvc', version: '14.51.36231', sdkVersion: '10.0.26100.0'};
test('luac CLI accepts a complete explicit toolchain without changing identity by default', () => {
  const options = parseBuildOptions(['build', 'candidate', '--msvc-version', fixed.version, '--sdk-version', fixed.sdkVersion]);
  assert.deepEqual(options, {output: 'candidate', sourceArchive: undefined, writeIdentity: false, toolchain: fixed});
  assert.deepEqual(parseBuildOptions(['build', 'candidate', 'official.tar.gz']), {output: 'candidate', sourceArchive: 'official.tar.gz', writeIdentity: false});
  assert.equal(parseBuildOptions(['build', '--source-archive', 'official.tar.gz', 'candidate', '--write-identity']).writeIdentity, true);
});
test('luac CLI rejects incomplete or ambiguous input instead of selecting latest', () => {
  for (const args of [
    ['build', 'out', '--msvc-version', fixed.version],
    ['build', 'out', '--sdk-version', fixed.sdkVersion],
    ['build', 'out', '--msvc-version'],
    ['build', 'out', '--write-identity', '--write-identity'],
    ['build', 'out', 'one.tar.gz', '--source-archive', 'two.tar.gz'],
    ['build', 'out', '--latest'],
    ['build', 'out', 'archive', 'surplus'],
  ]) assert.throws(() => parseBuildOptions(args), /luac\./);
  for (const value of [{...fixed, version: '../14.51.36231'}, {...fixed, sdkVersion: 'latest'}, {...fixed, family: 'gcc'}]) assert.throws(() => fixedToolchain(value), /invalid_fixed_toolchain/);
});
function toolFiles(installation, sdk, selected = fixed) {
  const tools = join(installation, 'VC/Tools/MSVC', selected.version);
  return [
    ...['cl.exe', 'link.exe'].map(name => join(tools, 'bin/Hostx64/x64', name)),
    join(tools, 'include/vcruntime.h'), join(tools, 'lib/x64/libcmt.lib'), join(tools, 'lib/x64/libvcruntime.lib'),
    ...['ucrt/errno.h', 'shared/sdkddkver.h', 'um/windows.h', 'winrt/winstring.h'].map(name => join(sdk, 'Windows Kits/10/Include', selected.sdkVersion, name)),
    join(sdk, 'Windows Kits/10/Lib', selected.sdkVersion, 'ucrt/x64/libucrt.lib'), join(sdk, 'Windows Kits/10/Lib', selected.sdkVersion, 'um/x64/kernel32.lib'),
  ];
}
test('compiler selection chooses only exact installed MSVC and SDK and removes injected flags', () => {
  const installation = '/official/VS', sdk = '/official/x86', files = new Set(toolFiles(installation, sdk));
  const env = {'ProgramFiles(x86)': sdk, Path: 'system-bin', CL: '/DUNREVIEWED', _CL_: '/link', LINK: '/DEBUG', _LINK_: '/DEBUG', INCLUDE: 'foreign', LIB: 'foreign', LIBPATH: 'foreign', KEEP: 'value'};
  const tool = selectCompiler(['/official/newer-VS', installation], fixed, env, path => files.has(path));
  assert.equal(tool.version, fixed.version); assert.equal(tool.sdkVersion, fixed.sdkVersion);
  assert.equal(tool.executable, join(installation, 'VC/Tools/MSVC', fixed.version, 'bin/Hostx64/x64/cl.exe'));
  assert.equal(tool.env.KEEP, 'value'); assert(tool.env.PATH.endsWith(';system-bin'));
  assert.equal(tool.env.Path, undefined);
  for (const name of ['CL', '_CL_', 'LINK', '_LINK_', 'LIBPATH']) assert.equal(tool.env[name], undefined);
  assert(!tool.env.INCLUDE.includes('foreign')); assert(!tool.env.LIB.includes('foreign'));
});
test('missing pinned compiler or SDK never falls back and duplicate installations are ambiguous', () => {
  const sdk = '/official/x86', first = '/official/one', second = '/official/two';
  const files = new Set(toolFiles(first, sdk)), env = {'ProgramFiles(x86)': sdk};
  assert.throws(() => selectCompiler([first], {...fixed, version: '14.50.35717'}, env, path => files.has(path)), /msvc_missing/);
  assert.throws(() => selectCompiler([first], {...fixed, sdkVersion: '10.0.99999.0'}, env, path => files.has(path)), /toolchain_incomplete/);
  for (const path of toolFiles(second, sdk)) files.add(path);
  assert.throws(() => selectCompiler([first, second], fixed, env, path => files.has(path)), /msvc_ambiguous/);
  assert.equal(selectCompiler([first, first], fixed, env, path => files.has(path)).version, fixed.version);
});
test('luac builds request only exact MSVC admission and never borrow Cargo helper approval', () => {
  const source = readFileSync(new URL('./luac.mjs', import.meta.url), 'utf8');
  assert(source.includes('ownedSpawnSync(command, args'));
  assert(source.includes('cargoBuild: false, msvcCompile'));
  assert(source.includes('env = process.env, msvcCompile = false'));
  assert(source.includes('run(toolchain.executable, args, sourceRoot, toolchain.env, true)'));
  assert.equal((source.match(/toolchain\.env, true/g)??[]).length,1);
  assert(!source.includes('cargoBuild: true'));
  assert(!source.includes("'-latest'"));
});
