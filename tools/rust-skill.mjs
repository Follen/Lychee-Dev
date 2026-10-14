// Stage the complete Go-aligned Skill, retaining the fixed target contract and
// a separate directory declared by the selected Rust candidate. Neither is
// behavioral acceptance evidence. This tool does not install the Skill.
import { cpSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

const repository = resolve(dirname(fileURLToPath(import.meta.url)), '..');

export function stageSkill(destination, commands) {
  if (!Array.isArray(commands) || !commands.length) throw new Error('rust-skill.invalid_contract');
  const seen = new Set();
  for (const command of commands) {
    if (!command || typeof command.path !== 'string' || !command.path.trim()
      || seen.has(command.path) || typeof command.summary !== 'string'
      || typeof command.mutates !== 'boolean'
      || !Array.isArray(command.flags) || command.flags.some(flag => typeof flag !== 'string')
      || (command.arguments !== undefined && (!Array.isArray(command.arguments)
        || command.arguments.some(argument => typeof argument !== 'string')))) {
      throw new Error('rust-skill.invalid_contract');
    }
    seen.add(command.path);
  }
  const source = join(repository, 'skills/lycheedev');
  // Require a new directory. Never overwrite an installed or prior staged Skill.
  mkdirSync(destination);
  cpSync(source, destination, { recursive: true, force: false, errorOnExist: true });
  const lines = [
    '# Selected candidate command directory', '',
    `${commands.length} commands declared by the selected candidate's describe output.`,
    'Declaration is not implementation, behavioral parity or acceptance evidence.',
    'The [fixed Go target](commands.md) remains the migration contract; missing',
    'commands are gaps, not removed requirements. See [candidate boundaries](source-candidate.md).', '',
  ];
  for (const command of commands) {
    const args = command.arguments?.length ? ` ${command.arguments.join(' ')}` : '';
    lines.push(`- \`${command.path}${args}\` — ${command.mutates ? 'mutates' : 'read-only'}. ${command.summary}`);
    if (command.flags.length) lines.push(`  Flags: ${command.flags.map(flag => `\`${flag}\``).join(', ')}.`);
  }
  writeFileSync(join(destination, 'references/candidate-commands.md'), `${lines.join('\n')}\n`);
  writeFileSync(join(destination, 'references/candidate-contract.json'), `${JSON.stringify({
    schema: 'lycheedev.skill-declared-contract.v1', commands,
  }, null, 2)}\n`);
  const entry = join(destination, 'SKILL.md');
  writeFileSync(entry, `${readFileSync(entry, 'utf8')}\nThe staged [candidate command directory](references/candidate-commands.md) records\nthe selected binary's declarations at staging time. Recheck describe/help if\nthe binary changes; the fixed target directory is not candidate availability.\n`);
  return destination;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [destinationArg, binaryArg, ...extra] = process.argv.slice(2);
  if (!destinationArg || !binaryArg || extra.length) {
    throw new Error('usage: node tools/rust-skill.mjs <new-skill-directory> <native-candidate>');
  }
  const result = spawnSync(resolve(binaryArg), ['describe', '--format=json'], {
    cwd: repository, encoding: 'utf8', windowsHide: true, shell: false, timeout: 120000,
  });
  if (result.error || result.status !== 0) {
    throw new Error(`rust-skill.contract_generation_failed:${result.error?.message ?? result.stderr}`);
  }
  const envelope = JSON.parse(result.stdout);
  if (!envelope.ok) throw new Error('rust-skill.invalid_contract');
  console.log(stageSkill(resolve(destinationArg), envelope.result?.commands));
}
