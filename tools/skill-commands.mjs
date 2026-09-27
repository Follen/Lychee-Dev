import { spawnSync } from 'node:child_process';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { writeFileSync, readFileSync } from 'node:fs';

const repository = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const outputIndex = process.argv.indexOf('--output');
const output = outputIndex >= 0 ? resolve(process.argv[outputIndex + 1]) : join(repository, 'skills', 'lycheedev', 'references', 'commands.md');
const run = spawnSync('go', ['run', './cmd/lycheedev', 'describe', '--format=json'], {
  cwd: repository, encoding: 'utf8', shell: false, windowsHide: true, timeout: 300000,
});
if (run.error) throw run.error;
if (run.status !== 0) throw new Error(`describe failed: ${run.status}\n${run.stdout}\n${run.stderr}`);
const envelope = JSON.parse(run.stdout);
if (!envelope.ok || !Array.isArray(envelope.result?.commands)) throw new Error('describe returned no command contracts');

const groups = new Map();
for (const command of envelope.result.commands) {
  const group = command.path.split(' ')[0];
  if (!groups.has(group)) groups.set(group, []);
  groups.get(group).push(command);
}
const lines = [
  '<!-- Generated from `lycheedev describe --format json` by',
  '     `node tools/skill-commands.mjs`. Do not edit by hand. -->',
  '', '# Command reference', '',
  `${envelope.result.commands.length} implemented commands. Every command accepts \`--format text|json|jsonl\`;`,
  '`--help` works on the root and on any command. Read the JSON envelope, not',
  'just the exit code; preserve capture IDs and partial/truncated warnings.',
];
for (const group of [...groups.keys()].sort()) {
  lines.push('', `## ${group}`, '');
  for (const command of groups.get(group)) {
    const args = command.arguments?.length ? ` ${command.arguments.join(' ')}` : '';
    lines.push(`- \`${command.path}${args}\` — ${command.mutates ? 'mutates' : 'read-only'}. ${command.summary}`);
    if (command.flags?.length) lines.push(`  Flags: ${command.flags.map(flag => `\`${flag}\``).join(', ')}.`);
  }
}
const generated = `${lines.join('\n')}\n`;
if (process.argv.includes('--check')) {
  if (readFileSync(output, 'utf8').replaceAll('\r\n', '\n') !== generated) {
    throw new Error('skill command reference is stale; run node tools/skill-commands.mjs');
  }
} else writeFileSync(output, generated, 'utf8');
process.stdout.write(`${output}\n`);
