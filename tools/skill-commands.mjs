// Render the current command catalog, or an explicitly selected installed CLI.
import {spawnSync} from 'node:child_process';
import {dirname, join, resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import {writeFileSync, readFileSync} from 'node:fs';

const repository = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const values = {}, argv = process.argv.slice(2);
let check = false;
for (let i = 0; i < argv.length; i++) {
  const name = argv[i];
  if (name === '--check' && !check) { check = true; continue; }
  if (!['--bin', '--output'].includes(name) || values[name] || !argv[i + 1] || argv[i + 1].startsWith('--')) throw new Error(`invalid option: ${name}`);
  values[name] = argv[++i];
}
if (!values['--output']) throw new Error('--output is required; CLI describe/help is the shipped command reference');
const output = resolve(values['--output']);
let commands = JSON.parse(readFileSync(join(repository, 'crates/lycheedev-cli/src/commands.json'), 'utf8'));
if (values['--bin']) {
  const run = spawnSync(resolve(values['--bin']), ['describe', '--format=json'], {encoding: 'utf8', shell: false, windowsHide: true, timeout: 120000});
  if (run.error) throw run.error;
  if (run.status !== 0) throw new Error(`describe failed: ${run.status}\n${run.stderr}`);
  const envelope = JSON.parse(run.stdout);
  if (!envelope.ok || !Array.isArray(envelope.result?.commands)) throw new Error('describe returned no command contracts');
  commands = envelope.result.commands;
}
const lines = ['<!-- Generated from the current command contract. -->', '', '# Command reference', '',
  'Use command help for details. Preserve capture IDs, coverage and completion status.'];
const groups = new Map();
for (const command of commands) {
  const group = command.path.split(' ')[0];
  if (!groups.has(group)) groups.set(group, []);
  groups.get(group).push(command);
}
for (const group of [...groups.keys()].sort()) {
  lines.push('', `## ${group}`, '');
  for (const command of groups.get(group)) {
    const args = command.arguments?.length ? ' ' + command.arguments.join(' ') : '';
    lines.push(`- \`${command.path}${args}\` — ${command.mutates ? 'mutates' : 'read-only'}. ${command.summary}`);
    if (command.flags?.length) lines.push(`  Flags: ${command.flags.map(flag => '\`' + flag + '\`').join(', ')}.`);
  }
}
const generated = lines.join('\n') + '\n';
if (check) {
  if (readFileSync(output, 'utf8').replaceAll('\r\n', '\n') !== generated) throw new Error('command reference is stale; regenerate with the same arguments without --check');
} else writeFileSync(output, generated);
process.stdout.write(output + '\n');
