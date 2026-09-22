// Skill/command reference consistency (regression CLI-04/SKL-11): every
// `lycheedev ...` invocation and every command-shaped code span documented in
// skills/lycheedev must resolve to an implemented command contract from
// `lycheedev describe --format json`, with only accepted flags. A schema change
// that leaves the skill behind fails here instead of shipping a stale skill.
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

const repository = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const argv = process.argv.slice(2);
function option(name) {
  const index = argv.indexOf(name);
  return index >= 0 ? argv[index + 1] : undefined;
}
const skillRoot = option('--skill-root') ?? join(repository, 'skills', 'lycheedev');
const binary = option('--bin');

function describe() {
  const result = binary
    ? spawnSync(binary, ['describe', '--format=json'], { encoding: 'utf8', shell: false, windowsHide: true, timeout: 120000 })
    : spawnSync('go', ['run', './cmd/lycheedev', 'describe', '--format=json'], { cwd: repository, encoding: 'utf8', shell: false, windowsHide: true, timeout: 300000 });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`describe failed: ${result.status}\n${result.stdout}\n${result.stderr}`);
  const envelope = JSON.parse(result.stdout);
  if (!envelope.ok || !Array.isArray(envelope.result?.commands)) throw new Error('describe returned no command contracts');
  return envelope.result.commands;
}

const contracts = new Map();
for (const command of describe()) {
  contracts.set(command.path, {
    path: command.path,
    flags: new Map(command.flags.map(display => [display.split(/\s+/)[0], display])),
  });
}
const oneTokenPaths = new Set([...contracts.keys()].filter(path => !path.includes(' ')));

function markdownFiles(root) {
  const files = [];
  const stack = [root];
  while (stack.length) {
    const directory = stack.pop();
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const path = join(directory, entry.name);
      if (entry.isDirectory()) stack.push(path);
      else if (entry.isFile() && path.endsWith('.md')) files.push(path);
    }
  }
  return files.sort();
}

function commandPieces(text) {
  const pieces = [];
  const withoutFences = text.replace(/```[^\n]*\n([\s\S]*?)```/g, (all, body) => {
    for (const line of body.split('\n')) pieces.push(line.trim());
    return '';
  });
  for (const match of withoutFences.matchAll(/`([^`]+)`/g)) {
    pieces.push(match[1].replace(/\s+/g, ' ').trim());
  }
  return pieces;
}

const topGroups = new Set(['project', 'data', 'live', 'skill', 'addon', 'evidence', 'source', 'target', 'asset', 'version', 'describe', 'init', 'doctor']);
const violations = [];
const references = [];

function checkInvocation(file, invocation) {
  const tokens = invocation.split(/\s+/).filter(Boolean);
  if (tokens[0] === 'lycheedev') tokens.shift();
  else if (!topGroups.has(tokens[0])) return;
  if (!tokens.length || tokens[0] === '--help' || (tokens[0] && tokens[0].startsWith('--'))) return;
  let path = tokens[0];
  let consumed = 1;
  // Prefer the longest matching command path (3-word rows like
  // "data decor list" have no 2-word contract for their prefix).
  for (const depth of [3, 2]) {
    if (tokens.length >= depth && contracts.has(tokens.slice(0, depth).join(' '))) {
      path = tokens.slice(0, depth).join(' ');
      consumed = depth;
      break;
    }
  }
  if (consumed === 1 && !contracts.has(tokens[0]) && !oneTokenPaths.has(tokens[0])) {
    // Unknown leading word pair: only report when it looks like a command.
    if (topGroups.has(tokens[0])) violations.push(`${file}: unknown command reference "${tokens.slice(0, 2).join(' ')}" in ${JSON.stringify(invocation)}`);
    return;
  }
  const contract = contracts.get(path);
  if (!contract) { violations.push(`${file}: command "${path}" is not in describe contracts`); return; }
  references.push({ file, path });
  const validFlags = new Set([...contract.flags.keys(), '--help']);
  for (let i = consumed; i < tokens.length; i++) {
    const token = tokens[i];
    if (!token.startsWith('--')) continue;
    const name = token.split('=')[0];
    const display = contract.flags.get(name);
    if (!validFlags.has(name)) {
      violations.push(`${file}: flag ${name} is not accepted by "${path}" in ${JSON.stringify(invocation)}`);
      continue;
    }
    if (display && /=|<|\|/.test(display.slice(name.length)) && !token.includes('=') && tokens[i + 1] && !tokens[i + 1].startsWith('--')) i++;
  }
}

if (!statSync(skillRoot).isDirectory()) throw new Error(`skill-contract: missing skill root ${skillRoot}`);
for (const file of markdownFiles(skillRoot)) {
  for (const piece of commandPieces(readFileSync(file, 'utf8'))) {
    for (const invocation of piece.split(/(?<=\S);\s*(?=lycheedev\s)/)) checkInvocation(file, invocation.replace(/[.;,]$/, ''));
  }
}

process.stdout.write(`${JSON.stringify({
  skillRoot,
  files: markdownFiles(skillRoot).length,
  contracts: contracts.size,
  references: references.length,
  violations,
}, null, 2)}\n`);
if (violations.length) {
  process.stderr.write(`skill-contract: ${violations.length} violation(s)\n`);
  process.exitCode = 1;
}
