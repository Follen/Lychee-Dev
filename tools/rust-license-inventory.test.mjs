import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {spawnSync} from 'node:child_process';
import {tmpdir} from 'node:os';
import {join, resolve} from 'node:path';
import {licensedDependencies} from './rust-license-inventory.mjs';
import {sourceDigest} from './rust-build.mjs';
import {assertArtifactSource} from './rust-package-smoke.mjs';

const repository = resolve(import.meta.dirname, '..');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const catalog = JSON.parse(readFileSync(join(repository, 'release/licenses/catalog.json')));
const supplement = catalog.packages.find(row => row.name === 'flatbuffers' && row.version === '25.12.19');
const upstreamCommit = '7e163021e59cca4f8e1e35a7c828b5c6b7915953';
const licenseSHA256 = 'cfc7749b96f63bd31c3c42b5c471bf756814053e847c10f3eb003417bc523d30';
const crateSHA256 = '35f6839d7b3b98adde531effaf34f0c2badc6f4735d26fe74709d8e513a96ef3';
const license = readFileSync(join(repository, 'release/licenses/flatbuffers-25.12.19.LICENSE'));

function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), 'lycheedev-flatbuffers-license-'));
  t.after(() => rmSync(root, {recursive: true, force: true}));
  const put = (path, bytes) => { mkdirSync(join(root, path, '..'), {recursive: true}); writeFileSync(join(root, path), bytes); };
  // Cargo metadata and VCS fields transcribed from the checksum-verified official
  // 25.12.19 crate. It contains no LICENSE, NOTICE, COPYING or license_file.
  const directory = 'registry/flatbuffers-25.12.19';
  put(`${directory}/Cargo.toml`, '[package]\nname = "flatbuffers"\nversion = "25.12.19"\nlicense = "Apache-2.0"\n');
  put(`${directory}/.cargo_vcs_info.json`, JSON.stringify({git: {sha1: upstreamCommit}, path_in_vcs: 'rust/flatbuffers'}));
  const pkg = {id: 'registry+https://github.com/rust-lang/crates.io-index#flatbuffers@25.12.19', name: 'flatbuffers', version: '25.12.19', license: 'Apache-2.0', license_file: null, repository: 'https://github.com/google/flatbuffers', source: 'registry+https://github.com/rust-lang/crates.io-index', manifest_path: join(root, directory, 'Cargo.toml')};
  const metadata = {packages: [pkg], resolve: {nodes: [{id: pkg.id}]}};
  put('release/licenses/flatbuffers-25.12.19.LICENSE', license);
  put('release/licenses/catalog.json', JSON.stringify({schema: catalog.schema, packages: [supplement]}));
  return {root, put, directory, pkg, metadata};
}

test('flatbuffers supplement binds the locked official crate to exact upstream license bytes', () => {
  assert.equal(catalog.packages.filter(row => row.name === 'flatbuffers' && row.version === '25.12.19').length, 1);
  assert.equal(supplement.commit, upstreamCommit);
  assert.equal(supplement.repository, 'https://github.com/google/flatbuffers');
  assert.equal(supplement.source, `https://raw.githubusercontent.com/google/flatbuffers/${upstreamCommit}/LICENSE`);
  assert.equal(supplement.path, 'flatbuffers-25.12.19.LICENSE');
  assert.equal(supplement.sha256, licenseSHA256);
  assert.equal(license.length, 11358);
  assert.equal(hash(license), licenseSHA256);
  assert.match(readFileSync(join(repository, 'Cargo.lock'), 'utf8'), new RegExp(`name = "flatbuffers"\\nversion = "25\\.12\\.19"\\nsource = "registry\\+https://github\\.com/rust-lang/crates\\.io-index"\\nchecksum = "${crateSHA256}"`));
});

test('missing crate license remains fail-closed without the exact supplement', t => {
  const f = fixture(t);
  assert.throws(() => licensedDependencies(f.root, f.metadata, []), /dependencies\.license_text_missing: flatbuffers/);
  for (const change of [{name: 'other'}, {version: '25.12.20'}]) {
    assert.throws(() => licensedDependencies(f.root, f.metadata, [{...supplement, ...change}]), /dependencies\.license_text_missing: flatbuffers/);
  }
  f.pkg.license = null;
  assert.throws(() => licensedDependencies(f.root, f.metadata, [supplement]), /dependencies\.license_missing: flatbuffers/);
});

test('official flatbuffers supplement contributes the complete unchanged license text', t => {
  const f = fixture(t);
  const [record] = licensedDependencies(f.root, f.metadata, [supplement]);
  assert.equal(record.name, 'flatbuffers'); assert.equal(record.version, '25.12.19'); assert.equal(record.license, 'Apache-2.0');
  assert.deepEqual(record.licenseFiles, [{name: supplement.path, sha256: licenseSHA256, text: license.toString('utf8'), source: supplement.source}]);
});

test('supplement admission rejects changed repository, VCS revision and license bytes', t => {
  const f = fixture(t);
  for (const change of [{repository: 'https://github.com/other/flatbuffers'}, {commit: '0'.repeat(40)}]) {
    assert.throws(() => licensedDependencies(f.root, f.metadata, [{...supplement, ...change}]), /dependencies\.license_source_mismatch: flatbuffers/);
  }
  f.put(`${f.directory}/.cargo_vcs_info.json`, JSON.stringify({git: {sha1: '0'.repeat(40)}}));
  assert.throws(() => licensedDependencies(f.root, f.metadata, [supplement]), /dependencies\.license_source_mismatch: flatbuffers/);
  f.put(`${f.directory}/.cargo_vcs_info.json`, JSON.stringify({git: {sha1: upstreamCommit}}));
  assert.throws(() => licensedDependencies(f.root, f.metadata, [{...supplement, sha256: '0'.repeat(64)}]), /dependencies\.license_digest_mismatch: flatbuffers/);
  f.put(`release/licenses/${supplement.path}`, Buffer.concat([license, Buffer.from('\nchanged')]));
  assert.throws(() => licensedDependencies(f.root, f.metadata, [supplement]), /dependencies\.license_digest_mismatch: flatbuffers/);
});

test('actual dependency notice writer emits full upstream text and authenticates the closure', t => {
  const f = fixture(t);
  // Copy the production entry point and collector unchanged. Only Cargo process
  // execution is substituted with fixture metadata; this test does not build.
  for (const name of ['rust-dependencies.mjs', 'rust-license-inventory.mjs', 'rust-emmy-identity.mjs', 'rust-tls-identity.mjs']) {
    f.put(`tools/${name}`, readFileSync(join(repository, 'tools', name)));
  }
  f.put('metadata.json', JSON.stringify(f.metadata));
  f.put('Cargo.lock', readFileSync(join(repository, 'Cargo.lock')));
  f.put('tools/rust-build.mjs', `import assert from 'node:assert/strict'; import {readFileSync} from 'node:fs'; import {join} from 'node:path';
export function runCargo(root, args) { assert.deepEqual(args, ['metadata','--locked','--offline','--format-version','1','--filter-platform','x86_64-pc-windows-msvc']); return {stdout: readFileSync(join(root,'metadata.json'),'utf8'), buildEvidence: 'fixture-metadata-only'}; }\n`);
  const out = join(f.root, 'closure');
  const run = spawnSync(process.execPath, [join(f.root, 'tools/rust-dependencies.mjs'), out], {cwd: f.root, encoding: 'utf8', timeout: 30000});
  assert.equal(run.status, 0, run.stderr);
  const closure = JSON.parse(readFileSync(join(out, 'dependency-closure.json')));
  assert.equal(closure.cargoLockSHA256, hash(readFileSync(join(f.root, 'Cargo.lock'))));
  assert.equal(closure.packages[0].licenseFiles[0].sha256, licenseSHA256);
  assert.equal(closure.packages[0].licenseFiles[0].text, license.toString('utf8'));
  const notices = readFileSync(join(out, 'THIRD_PARTY_NOTICES'));
  assert.deepEqual(notices, Buffer.concat([Buffer.from('flatbuffers 25.12.19 (Apache-2.0)\nhttps://github.com/google/flatbuffers\n\n'), license]));
});

test('standard release supplement changes product identity and cannot relabel a sealed artifact', t => {
  const f = fixture(t);
  for (const directory of ['crates', '.cargo']) mkdirSync(join(f.root, directory));
  f.put('Cargo.toml', '[workspace]\n'); f.put('Cargo.lock', '# fixture\n');
  const artifact = {profile: 'release', inputHashes: {sourceTree: sourceDigest(f.root)}};
  assertArtifactSource(artifact, sourceDigest(f.root), 'release');
  f.put(`release/licenses/${supplement.path}`, Buffer.concat([license, Buffer.from('\nchanged')]));
  assert.throws(() => assertArtifactSource(artifact, sourceDigest(f.root), 'release'), /candidate\.artifact_source_mismatch/);
});
