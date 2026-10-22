// Static documentation check: every relative markdown link/image in the
// tracked markdown files must resolve on disk. External URLs and pure anchors
// are not fetched (CI is offline for this check); links inside fenced code
// blocks are examples, not links. Run by the static stage and by the
// documentation-only workflow path, never by a native stage.
import { existsSync, readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const repository = resolve(import.meta.dirname ?? fileURLToPath(new URL('.', import.meta.url)), '..');

export function trackedMarkdown(root = repository) {
  const result = spawnSync('git', ['ls-files', '-z', '--', '*.md', ':(exclude).tmp/**'], {
    cwd: root, encoding: 'utf8', windowsHide: true, shell: false, timeout: 120000,
  });
  if (result.error || result.status !== 0) throw new Error(`ci-docs.git_failed: ${result.error?.message ?? result.status}`);
  return result.stdout.split('\0').filter(Boolean).sort();
}

function withoutFences(markdown) {
  return markdown.replace(/```[^\n]*\n[\s\S]*?```/g, '').replace(/~~~[^\n]*\n[\s\S]*?~~~/g, '');
}

function linkTargets(markdown) {
  const targets = [];
  for (const match of markdown.matchAll(/!?\[[^\]]*\]\(\s*([^)\s]+)(?:\s+["'][^"']*["'])?\s*\)/g)) targets.push(match[1]);
  for (const match of markdown.matchAll(/^\[[^\]]+\]:\s*(\S+)/gm)) targets.push(match[1]);
  return targets;
}

export function checkDocumentation(root = repository, files = trackedMarkdown(root)) {
  const violations = [];
  for (const file of files) {
    const absolute = resolve(root, file);
    if (!existsSync(absolute)) { violations.push(`${file}: tracked file missing`); continue; }
    for (const raw of linkTargets(withoutFences(readFileSync(absolute, 'utf8')))) {
      let target = raw.replace(/^<|>$/g, '');
      if (!target || target.startsWith('#') || target.startsWith('/') || target.startsWith('//')) continue;
      if (/^[a-z][a-z0-9+.-]*:/i.test(target)) continue;
      const hash = target.indexOf('#');
      if (hash >= 0) target = target.slice(0, hash);
      const query = target.indexOf('?');
      if (query >= 0) target = target.slice(0, query);
      if (!target) continue;
      let decoded = target;
      try { decoded = decodeURIComponent(target); } catch {}
      const path = resolve(dirname(absolute), decoded);
      if (!existsSync(path)) violations.push(`${file}: missing link target ${raw}`);
    }
  }
  return { files: files.length, violations };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [command, ...extra] = process.argv.slice(2);
  try {
    if (command !== 'check' || extra.length) throw new Error('usage: node tools/ci-docs.mjs check');
    const result = checkDocumentation();
    process.stdout.write(`${JSON.stringify(result, null, 2)}\n`);
    if (result.violations.length) process.exitCode = 1;
  } catch (error) {
    process.stderr.write(`${error.message}\n`);
    process.exitCode = 1;
  }
}
