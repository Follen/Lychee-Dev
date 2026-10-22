// Stage the exact Skill and an out-of-band machine declaration receipt.
// CLI describe/help owns discovery; staging never adds a prose command manual.
// Declaration and byte custody are not behavioral acceptance. This does not install.
import { cpSync, existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';
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
  const target = JSON.parse(readFileSync(join(repository, 'crates/lycheedev-cli/src/commands.json'), 'utf8'));
  const actual = new Map(commands.map(command => [command.path, command]));
  const missingCommands = target.filter(command => !actual.has(command.path)).map(command => command.path);
  const missingFlags = target.flatMap(command => {
    const selected = actual.get(command.path);
    if (!selected) return [];
    const names = new Set(selected.flags.map(flag => flag.split(/\s+/)[0]));
    return command.flags.filter(flag => !names.has(flag.split(/\s+/)[0]))
      .map(flag => ({path: command.path, flag: flag.split(/\s+/)[0]}));
  });
  // The receipt is a sibling artifact, not instructions or part of the Skill.
  const receipt = `${destination}.contract.json`;
  if (existsSync(receipt)) throw new Error('rust-skill.receipt_exists');
  // Require a new directory. Never overwrite an installed or prior staged Skill.
  mkdirSync(destination);
  // The exclusively created root is already ours. Copy each child into it;
  // Node on Windows rejects cpSync(source, existingRoot, errorOnExist: true).
  // Keep no-overwrite checks on every child rather than relaxing that policy.
  for (const name of readdirSync(source)) {
    cpSync(join(source, name), join(destination, name), {
      recursive: true, force: false, errorOnExist: true,
    });
  }
  writeFileSync(receipt, `${JSON.stringify({
    schema: 'lycheedev.skill-declared-contract.v1', commands,
    contractSource: 'crates/lycheedev-cli/src/commands.json', missingCommands, missingFlags,
    scope: 'Declared command/flag availability only; not handler or behavior acceptance',
  }, null, 2)}\n`, {flag: 'wx'});
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
