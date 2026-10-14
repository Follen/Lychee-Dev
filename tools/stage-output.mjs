import { existsSync, lstatSync, readdirSync, readFileSync, realpathSync, rmSync } from 'node:fs';
import { dirname, join, relative, resolve, sep } from 'node:path';
const marker = '.lycheedev-live-stage.json';
function canonical(path) {
  if (existsSync(path)) return realpathSync(path);
  const parent = dirname(path);
  return join(canonical(parent), relative(parent, path));
}
function within(parent, path) {
  const rel = relative(parent.toLowerCase(), path.toLowerCase());
  return rel === '' || rel !== '..' && !rel.startsWith(`..${sep}`) && !rel.includes(':');
}
export function prepareStageOutput(output, repository) {
  const path = resolve(output), actual = canonical(path), repo = canonical(resolve(repository));
  if (within(actual, repo) || ['addon', 'skills', 'internal', 'tools', 'packages', 'release', '.git'].some(name => {
    const source = join(repo, name); return within(actual, source) || within(source, actual);
  })) throw new Error('live-stage.source_overlap');
  if (!existsSync(path)) return path;
  if (!lstatSync(path).isDirectory() || lstatSync(path).isSymbolicLink() || actual.toLowerCase() !== path.toLowerCase()) throw new Error('live-stage.redirected_output');
  const entries = readdirSync(path);
  if (entries.length) {
    let identity;
    try { identity = JSON.parse(readFileSync(join(path, marker), 'utf8')); } catch {}
    if (identity?.schema !== 'lycheedev.live-stage.v1' || entries.some(name => ![marker, 'release.json', 'payload'].includes(name))) throw new Error('live-stage.unknown_output');
  }
  rmSync(path, { recursive: true });
  return path;
}
export const stageMarker = marker;
