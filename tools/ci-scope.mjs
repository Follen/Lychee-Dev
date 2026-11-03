// Only known development prose may bypass executable verification. Unknowns
// (including workflow_call, tags and protocol/Skill changes) use the full CI.
import { appendFileSync, existsSync, readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { spawnSync } from 'node:child_process';
import { repository } from './baseline-common.mjs';

const prose = new Set(['README.md', 'Design.md', 'Changelog.md', 'todo.md', 'handover.md',
  'docs/development.md', 'docs/rectification-plan.md', 'docs/Clean-repo-2026-10-14.md']);
export function classifyChanges(paths) {
  return paths.length > 0 && paths.every(path => prose.has(path)) ? 'docs' : 'full';
}
export function checkLinks(paths, root = repository) {
  let checked = 0;
  for (const path of paths) {
    const file = resolve(root, path);
    if (!existsSync(file)) continue; // A deleted prose file has no outgoing links.
    const body = readFileSync(file, 'utf8');
    for (const [, raw] of body.matchAll(/\]\(([^)]+)\)/g)) {
      const target = raw.replace(/^<|>$/g, '').split('#')[0];
      if (!target || /^[a-z][a-z\d+.-]*:/i.test(target)) continue;
      if (!existsSync(resolve(dirname(file), decodeURIComponent(target)))) throw new Error(`broken local link: ${path} -> ${raw}`);
      checked++;
    }
  }
  return checked;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const event = JSON.parse(readFileSync(process.env.GITHUB_EVENT_PATH, 'utf8'));
  const name = process.env.GITHUB_EVENT_NAME;
  const base = name === 'pull_request' ? event.pull_request?.base?.sha : name === 'push' && !process.env.GITHUB_REF?.startsWith('refs/tags/') ? event.before : null;
  let paths = [], scope = 'full';
  if (base && /^[a-f\d]{40}$/.test(base) && !/^0+$/.test(base)) {
    const diff = spawnSync('git', ['diff', '--name-only', '-z', base, 'HEAD'], { cwd: repository, encoding: 'utf8', windowsHide: true });
    if (diff.status === 0) {
      paths = diff.stdout.split('\0').filter(Boolean);
      scope = classifyChanges(paths);
    }
  }
  // Check all known prose when it is the only changed input, including incoming
  // links to deleted documents.
  const checkedLinks = scope === 'docs' ? checkLinks([...prose]) : 0;
  const result = { scope, paths, checkedLinks, reason: scope === 'docs' ? 'only known development prose changed' : 'executable, unknown or unclassified input' };
  if (process.env.GITHUB_OUTPUT) appendFileSync(process.env.GITHUB_OUTPUT, `scope=${scope}\n`);
  console.log(JSON.stringify(result, null, 2));
}
