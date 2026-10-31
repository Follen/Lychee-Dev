// Exact, bounded custody for the two pinned Windows TLS name adaptations.
import assert from 'node:assert/strict';
import {lstatSync, readFileSync, readdirSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {join, resolve} from 'node:path';

const manifestPath = 'vendor/tls-name-adaptation.json';
const manifestSHA256 = 'dd90f94ff39806505d7b0067f4eff0f88604ead8d461940d4ab7f56304665cae';
const digest = bytes => createHash('sha256').update(bytes).digest('hex');
function regular(root, relative) {
  let path = root;
  assert(lstatSync(path).isDirectory() && !lstatSync(path).isSymbolicLink(), 'tls.root');
  for (const component of relative.split('/')) {
    assert(component && component !== '.' && component !== '..' && !component.includes('\\'), 'tls.path');
    path = join(path, component);
    assert(!lstatSync(path).isSymbolicLink(), 'tls.reparse');
  }
  const info = lstatSync(path);
  assert(info.isFile() && info.size <= 2 * 1024 * 1024, 'tls.file_budget');
  return readFileSync(path);
}
function inventory(root, relative) {
  const result = []; let entries = 0;
  function visit(path, name, depth) {
    const info = lstatSync(path);
    assert(!info.isSymbolicLink() && depth <= 12 && ++entries <= 128, 'tls.reparse_or_depth');
    if (info.isDirectory()) for (const child of readdirSync(path).sort()) visit(join(path, child), `${name}/${child}`, depth + 1);
    else { assert(info.isFile() && result.length < 128, 'tls.inventory'); result.push(name); }
  }
  visit(join(root, relative), '', 0);
  return result.map(path => path.slice(1)).sort();
}
export function inspectTlsNameAdaptation(root) {
  root = resolve(root);
  const raw = regular(root, manifestPath);
  assert.equal(digest(raw), manifestSHA256, 'tls.manifest_identity');
  const record = JSON.parse(raw);
  assert.equal(digest(regular(root, record.patch)), record.patchSHA256, 'tls.patch_identity');
  const packages = record.packages.map(pkg => {
    const path = `vendor/${pkg.name}-${pkg.version}`;
    assert.deepEqual(inventory(root, path), pkg.files.map(file => file.path).sort(), 'tls.source_inventory');
    for (const file of pkg.files) assert.equal(digest(regular(root, `${path}/${file.path}`)), file.candidateSHA256, `tls.source_identity:${file.path}`);
    return {...pkg, path, manifestPath: join(root, path, 'Cargo.toml')};
  });
  return {schema: record.schema, manifestPath, manifestSHA256, patch: record.patch, patchSHA256: record.patchSHA256, packages};
}
