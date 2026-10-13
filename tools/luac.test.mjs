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
