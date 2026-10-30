// CI change classification: a documentation-only change set must not trigger
// the native build/validate stages (DESIGN §8 "仅文档变动...不触发无关的完整
// 原生构建"). Anything that cannot be classified, an empty change set, or an
// unresolvable git range defaults to native=true; the build stage is never
// skipped on a guess. The result is written to GITHUB_OUTPUT by the static job.
import { appendFileSync, readFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { isAbsolute, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const repository = resolve(import.meta.dirname ?? fileURLToPath(new URL('.', import.meta.url)), '..');
// Embedded fixtures and packaged Markdown are product inputs. Check their
// directories before file extensions so a README/SKILL edit cannot skip the
// build or package that actually contains it.
const productDirectory = /^(?:crates|release|vendor|tests|skills|packages|addon|third_party)\//i;
const documentation = path => !productDirectory.test(path) && (/\.md$/i.test(path) || /^docs\//i.test(path));

function parseArgs(argv) {
  const options = { event: process.env.GITHUB_EVENT_PATH, githubOutput: undefined, files: undefined, root: repository };
  for (let index = 0; index < argv.length; index++) {
    const arg = argv[index], value = argv[index + 1];
    if (arg === '--event') { options.event = value; index++; }
    else if (arg === '--github-output') { options.githubOutput = value; index++; }
    else if (arg === '--files') { options.files = value; index++; }
    else if (arg === '--root') { options.root = resolve(value); index++; }
    else throw new Error(`ci-changes.unknown_option: ${arg}`);
  }
  return options;
}

function git(root, args) {
  const result = spawnSync('git', args, { cwd: root, encoding: 'utf8', windowsHide: true, shell: false, timeout: 120000 });
  if (result.error || result.status !== 0) throw new Error(`ci-changes.git_failed: ${result.error?.message ?? result.status}`);
  return result.stdout;
}

function changedBetween(root, base, head) {
  if (!base || /^0+$/.test(base)) throw new Error('ci-changes.no_base_revision');
  return git(root, ['diff', '--name-only', '--no-renames', base, head])
    .split(/\r?\n/).map(line => line.trim()).filter(Boolean);
}

export function classify({ root = repository, event, files, name = eventName(event) } = {}) {
  if (Array.isArray(files)) return decide(files, 'explicit-file-list');
  if (!event || typeof event !== 'object') return { native: true, reason: 'missing-event', changedFiles: [] };
  if (name === 'workflow_dispatch' || name === 'workflow_call' || name === 'schedule') {
    return { native: true, reason: `event:${name}`, changedFiles: [] };
  }
  try {
    if (name === 'pull_request' || name === 'pull_request_target') {
      const base = event.pull_request?.base?.sha;
      return decide(changedBetween(root, base, 'HEAD'), 'pull-request-diff');
    }
    if (name === 'push') {
      const before = event.before, after = event.after || 'HEAD';
      return decide(changedBetween(root, before, after), 'push-diff');
    }
  } catch (error) {
    return { native: true, reason: `unavailable:${error.message}`, changedFiles: [] };
  }
  return { native: true, reason: `event:${name || 'unknown'}`, changedFiles: [] };
}

// GitHub exposes the event name through GITHUB_EVENT_NAME, not the payload;
// synthetic payloads used by tests may carry it as event_name instead.
function eventName(event) {
  const value = process.env.GITHUB_EVENT_NAME
    ?? (event && typeof event.event_name === 'string' ? event.event_name : undefined);
  return value;
}

function decide(changedFiles, reason) {
  const sorted = [...new Set(changedFiles)].sort();
  if (!sorted.length) return { native: true, reason: `${reason}:empty-change-set`, changedFiles: [] };
  const docsOnly = sorted.every(documentation);
  return { native: !docsOnly, reason: `${reason}:${docsOnly ? 'documentation-only' : 'native-inputs'}`, changedFiles: sorted };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const options = parseArgs(process.argv.slice(2));
    const explicit = options.files === undefined ? undefined : options.files.split(',').map(file => file.trim()).filter(Boolean);
    const event = options.event ? JSON.parse(readFileSync(isAbsolute(options.event) ? options.event : resolve(options.event), 'utf8')) : undefined;
    const decision = classify({ root: options.root, event, files: explicit });
    if (options.githubOutput) appendFileSync(options.githubOutput, `native=${decision.native}\n`);
    process.stdout.write(`${JSON.stringify(decision, null, 2)}\n`);
  } catch (error) {
    // Classification failure must never skip the build stage.
    if (process.argv.includes('--github-output')) {
      const index = process.argv.indexOf('--github-output');
      appendFileSync(process.argv[index + 1], 'native=true\n');
    }
    process.stderr.write(`${error.message}\n`);
    process.exitCode = 1;
  }
}
