// Sealed build-artifact manifests: component identity, self-manifest handling,
// tamper detection and reuse-only verification. No Cargo or product build runs.
import assert from 'node:assert/strict';
import { lstatSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { after, test } from 'node:test';
import { sealArtifacts, verifyArtifacts } from './ci-artifacts.mjs';
import { importLuaToolSupply } from './lua-tool-supply.mjs';

const root = resolve(import.meta.dirname, '..');
const bases = [];
function fixture() {
  const base = join(root, '.tmp', `ci-artifacts-test-${process.pid}-${Date.now()}-${bases.length}`);
  bases.push(base);
  const component = join(base, 'lua51');
  importLuaToolSupply(base);
  return { base, component, manifest: join(component, 'build-manifest.json') };
}
after(() => { for (const base of bases) rmSync(base, { recursive: true, force: true }); });

test('seal then verify accepts a manifest sealed inside its own component directory', () => {
  const { base, component, manifest } = fixture();
  const sealed = sealArtifacts({ output: manifest, components: [{ kind: 'lua51', directory: component }] });
  assert.deepEqual(sealed.components, ['lua51']);
  const verified = verifyArtifacts({ manifestPath: manifest, require: ['lua51'] });
  assert.equal(verified.ok, true);
  assert.equal(verified.files, sealed.files, 'the manifest itself must not count as a component file');
  assert(!JSON.parse(readFileSync(manifest)).files.some(row => row.path.endsWith('/build-manifest.json')));
  assert.equal(lstatSync(manifest).isFile(), true);
});

test('verify rejects a changed component byte instead of relabeling it', () => {
  const { component, manifest } = fixture();
  sealArtifacts({ output: manifest, components: [{ kind: 'lua51', directory: component }] });
  const executable = join(component, 'lua.exe');
  const changed = readFileSync(executable);
  changed[0] ^= 0xff;
  writeFileSync(executable, changed);
  assert.throws(() => verifyArtifacts({ manifestPath: manifest, require: ['lua51'] }), /import_integrity/);
});

test('verify rejects an extra file added after sealing', () => {
  const { component, manifest } = fixture();
  sealArtifacts({ output: manifest, components: [{ kind: 'lua51', directory: component }] });
  writeFileSync(join(component, 'extra.txt'), 'not in the sealed record');
  assert.throws(() => verifyArtifacts({ manifestPath: manifest, require: ['lua51'] }), /component_files_changed/);
});

test('verify requires every requested component and refuses an unknown kind', () => {
  const { component, manifest } = fixture();
  sealArtifacts({ output: manifest, components: [{ kind: 'lua51', directory: component }] });
  assert.throws(() => verifyArtifacts({ manifestPath: manifest, require: ['lua51', 'cli-release'] }), /required_component_missing/);
  assert.throws(() => sealArtifacts({ output: join(component, 'other.json'), components: [{ kind: 'not-a-kind', directory: component }] }), /unknown_component/);
});

test('Lua component rejects retired untyped report spelling',()=>{
 const {component,manifest}=fixture(),path=join(component,'build-report.json'),report=JSON.parse(readFileSync(path));
 report.version='Lua 5.1.5 Copyright';report.platform='win32';delete report.schema;writeFileSync(path,JSON.stringify(report));
 assert.throws(()=>sealArtifacts({output:manifest,components:[{kind:'lua51',directory:component}]}),/lycheedev.formal-lua-interpreter/);
});
