#!/usr/bin/env node
/** lycheedev command line entry. */

import { readFileSync } from 'node:fs';
import path from 'node:path';

import { runDoctor } from '../src/doctor.js';
import { runInstall } from '../src/install.js';
import { loadConfig } from '../src/config.js';
import { runAutomation } from '../src/python.js';
import { findSavedVariablesCandidates, scanClients } from '../src/wow.js';
import { addonFolderName, exists } from '../src/paths.js';

const HELP = `lycheedev - Lychee Dev installer and World of Warcraft automation driver

usage: lycheedev <command> [options]

commands
  install              install python dependencies, the skill and the addon
  update               refresh dependencies, skill and addon to this version
  doctor               report what is installed and what is missing
  clients              list detected clients and their addon directories
  send <text>          type one slash command into the running game
  capture              poll the game window and decode the completion notice
  run --task <id>      deliver, decode, reload once and read the result ticket
  bugs --count <n>     snapshot recent errors and read the result ticket
  ack --ticket <t>     report that a ticket was read (received|failed)
  task <sub>           manage the in-game task registry (upsert/list/remove)
  sv <sub>             discover SavedVariables and read a ticket (find/read)
  profile <sub>        bind a client install for direct automation.py use
  status               summarize the local session log
  recover              show reload intents that were never confirmed

options
  --wow-root <path>    folder that contains _retail_/_classic_/_classic_arena_
  --client <id>        retail | classic | titan (default: the only configured one)
  --python <bin>       python interpreter to use
  --force              redo work that is already present
  --reset-registry     install the addon with an empty task registry
  --json               machine-readable output where supported
  -h, --help           show this help
  -v, --version        show the version

The automation subcommands are thin wrappers over the bundled python helper, so
the same arguments it accepts are forwarded unchanged.`;

const SHORT_FLAGS = { '-h': 'help', '-v': 'version' };

function parse(argv) {
  const flags = {};
  const rest = [];
  for (let index = 0; index < argv.length; index += 1) {
    const token = argv[index];
    if (token === '--') {
      rest.push(...argv.slice(index + 1));
      break;
    }
    if (SHORT_FLAGS[token]) {
      flags[SHORT_FLAGS[token]] = true;
      continue;
    }
    if (token.startsWith('--')) {
      const [name, inline] = token.slice(2).split('=');
      if (inline !== undefined) {
        flags[name] = inline;
      } else if (argv[index + 1] && !argv[index + 1].startsWith('-')) {
        flags[name] = argv[index + 1];
        index += 1;
      } else {
        flags[name] = true;
      }
      continue;
    }
    rest.push(token);
  }
  return { flags, rest };
}

/** hwnd/pid/exe-path derived from config so the automation commands line up. */
function targetArgs(client) {
  const args = [];
  if (client && client.hwnd) args.push('--hwnd', String(client.hwnd));
  if (client && client.pid) args.push('--pid', String(client.pid));
  return args;
}

function requireClient(flags) {
  const config = loadConfig();
  const ids = Object.keys(config.clients);
  const requested = flags.client;
  if (requested) {
    if (!config.clients[requested]) {
      console.error(`error: unknown client '${requested}'; known: ${ids.join(', ') || 'none'}`);
      return { error: true };
    }
    return { config, client: config.clients[requested], id: requested };
  }
  if (ids.length === 0) {
    console.error('error: no client configured; run `lycheedev install --wow-root <path>`');
    return { error: true };
  }
  if (ids.length === 1) {
    return { config, client: config.clients[ids[0]], id: ids[0] };
  }
  // Several clients are installed; the one recorded at install time is the
  // default so routine commands stay short, and --client overrides it.
  const fallback = config.defaultClient;
  if (fallback && config.clients[fallback]) {
    return { config, client: config.clients[fallback], id: fallback };
  }
  console.error(`error: several clients configured (${ids.join(', ')}); pass --client <id>`);
  return { error: true };
}

function runClients(flags) {
  const config = loadConfig();
  const root = flags['wow-root'] || config.wowRoot;
  if (!root || !exists(root)) {
    console.error('error: no World of Warcraft root; run `lycheedev install --wow-root <path>`');
    return 1;
  }
  console.log(root);
  for (const client of scanClients(root)) {
    const supported = ['retail', 'classic', 'titan'].includes(client.id);
    const installed = exists(path.join(client.addonsDir, addonFolderName));
    console.log(`  ${client.id.padEnd(11)} ${client.folder.padEnd(16)} `
      + `${installed ? 'addon installed' : 'addon missing'}`.padEnd(18)
      + `${supported ? '' : '(not supported by the addon)'}`);
  }
  return 0;
}

function main(argv) {
  const { flags, rest } = parse(argv);
  const command = rest.shift();

  // Help and version are answers, not errors: they must not depend on whether a
  // command was supplied.
  if (flags.help) {
    console.log(HELP);
    return 0;
  }
  if (flags.version) {
    const pkg = JSON.parse(
      readFileSync(new URL('../package.json', import.meta.url), 'utf8'),
    );
    console.log(pkg.version || 'unknown');
    return 0;
  }
  if (!command) {
    console.log(HELP);
    return 1;
  }

  switch (command) {
    case 'install':
      return runInstall({ flags });
    case 'update':
      return runInstall({ flags: { ...flags, force: true } });
    case 'doctor':
      return runDoctor({ flags });
    case 'clients':
      return runClients(flags);
    case 'help':
      console.log(HELP);
      return 0;
    default:
      break;
  }

  // Everything else is forwarded to the bundled python helper.
  const target = requireClient(flags);
  if (target.error) return 1;

  const forwarded = [];
  const client = target.client || {};
  // The python helper wants the installed addon directory itself, not the
  // AddOns folder that contains it.
  if (client.addonsDir) forwarded.push('--install-dir', path.join(client.addonsDir, addonFolderName));

  switch (command) {
    case 'send': {
      const text = rest.join(' ');
      if (!text) {
        console.error('error: send needs the command text, e.g. `lycheedev send "/dev auto status x"`');
        return 1;
      }
      return forward(['send', ...targetArgs(client), '--text', text, ...passthrough(flags)]);
    }
    case 'capture':
      return forward(['capture', ...targetArgs(client), ...passthrough(flags)]);
    case 'run':
      if (!flags.task) {
        console.error('error: run needs --task <id>');
        return 1;
      }
      if (!client.svPath) {
        console.error('error: no SavedVariables path recorded for this client; re-run `lycheedev install`');
        return 1;
      }
      return forward(['run', ...targetArgs(client), '--task', flags.task, '--sv', client.svPath,
        ...passthrough(flags)]);
    case 'bugs':
      if (!flags.count) {
        console.error('error: bugs needs --count <1-100>');
        return 1;
      }
      if (!client.svPath) {
        console.error('error: no SavedVariables path recorded for this client; re-run `lycheedev install`');
        return 1;
      }
      return forward(['bugs', ...targetArgs(client), '--count', String(flags.count), '--sv', client.svPath,
        ...passthrough(flags)]);
    case 'ack':
      if (!flags.ticket || !flags.status) {
        console.error('error: ack needs --ticket <t> --status received|failed');
        return 1;
      }
      return forward(['ack', ...targetArgs(client), '--ticket', flags.ticket,
        '--status', flags.status, ...passthrough(flags)]);
    case 'task': {
      const sub = rest.shift();
      if (!sub) {
        console.error('error: task needs a subcommand: upsert, list or remove');
        return 1;
      }
      return forward(['task', sub, ...forwarded, ...passthrough(flags)]);
    }
    case 'sv': {
      const sub = rest.shift();
      if (sub === 'find') {
        // The wrapper already knows the plaintext install and can enumerate the
        // account folders itself, so `sv find` needs no profile binding.
        const config = loadConfig();
        const root = flags['wow-root'] || config.wowRoot;
        if (!root || !exists(root)) {
          console.error('error: no World of Warcraft root; run `lycheedev install --wow-root <path>`');
          return 1;
        }
        const candidates = findSavedVariablesCandidates(root, client.flavorFolder);
        if (candidates.length === 0) {
          console.error(`no account-level '${addonFolderName}.lua' found under ${root}`);
          return 1;
        }
        for (const candidate of candidates) console.log(candidate.path);
        return 0;
      }
      if (sub === 'read') {
        if (!flags.ticket) {
          console.error('error: sv read needs --ticket <t>');
          return 1;
        }
        const svPath = flags.sv || client.svPath;
        if (!svPath) {
          console.error('error: no SavedVariables path known for this client; '
            + 'run `lycheedev install`, or pass --sv <path>');
          return 1;
        }
        return forward(['sv', 'read', '--sv', svPath, '--ticket', flags.ticket,
          ...passthrough(flags)]);
      }
      console.error('error: sv needs a subcommand: find or read');
      return 1;
    }
    case 'profile':
    case 'status':
    case 'recover':
      return forward([command, ...rest, ...passthrough(flags)]);
    default:
      console.error(`error: unknown command '${command}'`);
      console.log(HELP);
      return 1;
  }

  function passthrough(all) {
    const out = [];
    // These are either handled by the wrapper itself or are global python
    // options that `forward()` places before the subcommand.
    const handled = ['wow-root', 'client', 'python', 'force', 'reset-registry', 'task', 'count',
      'ticket', 'status', 'json', 'help', 'version', 'data-dir', 'installation'];
    for (const [name, value] of Object.entries(all)) {
      if (handled.includes(name)) continue;
      if (value === true) out.push(`--${name}`);
      else out.push(`--${name}`, String(value));
    }
    return out;
  }

  function forward(args) {
    // `--data-dir` and `--installation` are global options of the python helper,
    // so they must precede the subcommand or argparse rejects them. Everything
    // else stays where the caller put it.
    const globals = [];
    if (flags['data-dir']) globals.push('--data-dir', String(flags['data-dir']));
    if (flags.installation) globals.push('--installation', String(flags.installation));
    const finalArgs = [...globals, ...args];
    if (process.env.LYCHEEDEV_DEBUG) {
      console.error(`[debug] python automation.py ${finalArgs.join(' ')}`);
    }
    const result = runAutomation(finalArgs, {
      pythonBin: flags.python || (target.config || {}).pythonBin,
    });
    return typeof result.status === 'number' ? result.status : 1;
  }
}

process.exitCode = main(process.argv.slice(2));
