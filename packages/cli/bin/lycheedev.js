#!/usr/bin/env node
/** lycheedev command line entry. */

import { readFileSync } from 'node:fs';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { randomBytes } from 'node:crypto';

import { runDoctor } from '../src/doctor.js';
import { runInstall } from '../src/install.js';
import { loadConfig, updateConfig } from '../src/config.js';
import { runAutomation } from '../src/python.js';
import {
  describeAmbiguity,
  findInstances,
  findSavedVariablesCandidates,
  formatInstance,
  identityKey,
  identityLabel,
  matchIdentity,
  matchPinned,
  resolveInstance,
  scanClients,
} from '../src/wow.js';
import { addonFolderName, exists } from '../src/paths.js';

const HELP = `lycheedev - Lychee Dev installer and World of Warcraft automation driver

usage: lycheedev <command> [options]

commands
  install              install python dependencies, the skill and the addon
  update               refresh dependencies, skill and addon to this version
  doctor               report what is installed and what is missing
  clients              list every detected build and its addon state
  instances            list running game clients (build, pid, window)
  instances --identify also read each window's character and build marker
  use [index]          pin one running client as the default target
  use --character <n>  pin the window that reports character <n>
  use --clear          drop the pin
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
  --wow-root <path>    folder that contains _retail_/_classic_/_classic_titan_
  --client <id>        retail | classic | titan | forever (default: the configured one)
  --instance <index>   which running client to target when several are open
  --character <name>   target the window reporting that character (Name or Name-Realm)
  --identify           also read each window's character marker (instances, use)
  --hwnd <handle>      target one window directly, skipping instance lookup
  --pid <pid>          expected process id for the target window
  --python <bin>       python interpreter to use
  --force              redo work that is already present
  --reset-registry     install the addon with an empty task registry
  --json               machine-readable output where supported
  -h, --help           show this help
  -v, --version        show the version

Every non-retail client runs as WowClassic.exe, so a running client is matched
by its install path, not its process name. A folder is only a location: the CLI
also reads each client's own .flavor.info product code and version.txt build, so
a reused test folder (such as _classic_beta_ carrying WoW: Forever) is still
identified correctly. With several clients open, run "lycheedev instances" then
"lycheedev use <index>" to pin one as the target; the pin stores the build, so it
survives a restart even though the pid and window handle change. Pass
--instance <index> to override it for one command.

When several windows share one build, they are told apart by the character each
window reports: run "/dev auto identify" in each game window, then
"lycheedev instances --identify" to read them, and pin by name with
"lycheedev use --character <name>". Windows that report the same character, or
that show no marker, remain ambiguous and the CLI asks for one window to be
closed rather than risking a command in the wrong game.

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
  const exePath = client?.exePath || client?.instance?.exePath;
  if (exePath) args.push('--exe-path', exePath);
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
    const version = client.version ? `v${client.version}` : 'unknown version';
    // The product is the client's own identity; the folder is only where it
    // lives, and one folder can serve different products over time.
    const product = client.folderProduct && client.folderProduct !== client.product
      ? `[${client.folderProduct}] `
      : '';
    const state = !client.toc
      ? 'not served by the addon'
      : (client.installed ? 'addon installed' : 'addon missing');
    console.log(`  ${client.id.padEnd(12)} ${client.folder.padEnd(18)} `
      + `${(product + version).padEnd(18)} ${state}`);
  }
  return 0;
}

/** `lycheedev instances`: which game clients are running right now. */
function runInstances(flags) {
  const config = loadConfig();
  let instances = findInstances({ pythonBin: flags.python || null });
  if (instances.length === 0) {
    console.log('no World of Warcraft instance is running');
    return 0;
  }

  // `--identify` asks each window to report the character and build it holds.
  // Capture reads the window surface directly, so no window needs the
  // foreground and nothing is typed into the game.
  let identifyNote = null;
  if (flags.identify) {
    const supported = instances.filter((item) => item.supported);
    if (supported.length === 0) {
      identifyNote = 'no served build is running, so there is nothing to identify';
    } else {
      const probed = probeIdentities(supported, flags);
      if (probed.error) {
        identifyNote = probed.error;
      } else {
        const byHwnd = new Map(probed.list.map((entry) => [String(entry.hwnd), entry]));
        instances = instances.map((instance) => {
          const entry = byHwnd.get(String(instance.hwnd));
          if (!entry || !entry.identity) return instance;
          return { ...instance, identity: entry.identity };
        });
        const unidentified = probed.list.filter((entry) => !entry.identity);
        if (unidentified.length) {
          identifyNote = `${unidentified.length} window(s) show no identity marker; `
            + 'run `/dev auto identify` in each game window you care about, then retry';
        }
      }
    }
  }

  const pinned = matchPinned(instances, config.pinnedInstance);
  console.log('running instances:');
  instances.forEach((instance, index) => {
    console.log(formatInstance(instance, index));
  });
  const supported = instances.filter((item) => item.supported);
  console.log('');
  if (pinned) {
    const label = pinned.flavorLabel || pinned.flavorFolder;
    console.log(`pinned: ${label} (pid ${pinned.pid}), pid ${pinned.pid} - commands target it`);
  } else if (config.pinnedInstance) {
    console.log('pinned client is not running; run `lycheedev use` to repin');
  }
  if (supported.length === 0) {
    console.log('none of these builds are served by the addon');
  } else if (!pinned && supported.length > 1) {
    const shape = describeAmbiguity(instances);
    if (shape.sameBuildCollision) {
      // Do not suggest `use <index>` here: pinning is refused for windows that
      // cannot be told apart, so pointing at it would send the user in a circle.
      console.log('several clients are running and cannot be told apart: run '
        + '`/dev auto identify` in each game window, then `lycheedev use --character <name>`');
    } else {
      console.log('several clients are running: `lycheedev use <index>` pins one, '
        + 'or pass --instance <index> per command');
    }
  } else if (!pinned) {
    console.log('commands target this instance automatically');
  }
  if (identifyNote) console.log(identifyNote);
  return 0;
}

/**
 * Ask the Python helper to read the identity marker from each window.
 *
 * The instance list is handed over as a file rather than arguments, because
 * every field has to survive the trip and a command line is a fragile place to
 * put it. The name is unpredictable and the file is created exclusively, so a
 * pre-existing file at that path cannot be followed or silently overwritten.
 */
function probeIdentities(supported, flags) {
  const windowsFile = path.join(os.tmpdir(),
    `lycheedev-windows-${process.pid}-${randomBytes(8).toString('hex')}.json`);
  try {
    fs.writeFileSync(windowsFile, JSON.stringify(supported), { encoding: 'utf8', flag: 'wx' });
    const args = ['identify', '--windows-json', windowsFile];
    if (flags.timeout) args.push('--timeout', String(flags.timeout));
    const result = runAutomation(args, {
      pythonBin: flags.python || null,
      inherit: false,
    });
    if (!result.ok) {
      return { error: `could not read identity markers: ${result.stderr || 'helper failed'}` };
    }
    return { list: JSON.parse(result.stdout || '[]') };
  } catch (error) {
    return { error: `could not read identity markers: ${error.message}` };
  } finally {
    try {
      fs.unlinkSync(windowsFile);
    } catch {
      // A leftover temp file is harmless.
    }
  }
}

/**
 * Resolve the live game window a command should type into.
 *
 * Precedence: an explicit `--hwnd`/`--pid`, then `--instance <index>`, then
 * `--character <name>`, then the saved pin, then `--client`, and finally the only
 * running instance when there is exactly one. `WowClassic.exe` looks identical
 * for every classic build, so an ambiguous choice fails loudly instead of typing
 * into the wrong game.
 *
 * Windows of one build are distinguishable when each reports a different
 * character; only windows that report the same character (or no marker at all)
 * are genuinely ambiguous, and those still fail loudly.
 */
function resolveRuntime(flags) {
  if (flags.hwnd || flags.pid) {
    return { hwnd: flags.hwnd, pid: flags.pid };
  }
  const instances = findInstances({ pythonBin: flags.python || null });
  const supported = instances.filter((item) => item.supported);

  if (flags.instance !== undefined) {
    const chosen = supported[Number(flags.instance)];
    if (!chosen) {
      console.error(`error: --instance ${flags.instance} is out of range (0..${supported.length - 1})`);
      supported.forEach((item, index) => console.log(formatInstance(item, index)));
      return { error: true };
    }
    return toTarget(chosen);
  }

  if (flags.character) {
    // A named character is only findable if the window showed its marker, so
    // probe first; this is read-only capture and types nothing.
    const named = resolveByCharacter(instances, flags.character, flags);
    if (named.error) return { error: true };
    return toTarget(named.instance);
  }

  if (!flags.client) {
    const config = loadConfig();
    const pinned = matchPinned(instances, config.pinnedInstance);
    if (pinned) {
      return toTarget(pinned);
    }
    if (config.pinnedInstance) {
      console.error('error: the pinned client is not running; run `lycheedev use` again '
        + 'or pass --instance <index>');
    }
  }

  const resolved = resolveInstance(instances, { clientId: flags.client || null, index: null });
  if (resolved.error) {
    const shape = describeAmbiguity(instances);
    if (shape.sameBuildCollision) {
      // Same build and no way to separate the windows: no flag can pick the right
      // one reliably, so ask for a single window rather than inviting a wrong guess.
      for (const entry of shape.collisions) {
        for (const group of entry.sameCharacter) {
          const label = group[0].flavorLabel || entry.flavorId;
          console.error(`error: ${group.length} ${label} clients report the same character `
            + `and cannot be told apart`);
          console.error(`       close all but one, or close these pids: `
            + group.map((i) => i.pid).join(', '));
        }
      }
      if (!shape.sameBuildNamed) {
        console.error('       run `/dev auto identify` in each game window, then retry '
          + 'with --identify');
      }
    } else if (supported.length > 1) {
      // Several windows of one build, and the caller filtered to that build. They
      // may well be separable by character, so say how rather than just failing.
      console.error(`error: ${resolved.error}`);
      console.error('       if each window shows a different character, run '
        + '`/dev auto identify` in each, then pass --character <name>; '
        + 'otherwise pass --instance <index>');
    } else {
      console.error(`error: ${resolved.error}`);
    }
    if (resolved.candidates) {
      resolved.candidates.forEach((item, index) => console.log(formatInstance(item, index)));
    }
    if (supported.length === 0 && instances.length > 0) {
      instances.forEach((item, index) => console.log(formatInstance(item, index)));
    }
    return { error: true };
  }
  return toTarget(resolved.instance);
}

/**
 * Find the window reporting a named character, probing the markers first.
 *
 * Names only exist inside the marker, so read it when the caller has not already
 * supplied it. Returns `{ instance }` or `{ error: true }` after explaining why.
 */
function resolveByCharacter(instances, query, flags) {
  const supported = instances.filter((item) => item.supported);
  if (supported.length === 0) {
    console.error('error: no served build is running');
    return { error: true };
  }

  let probed = supported;
  if (!supported.some((item) => item.identity)) {
    const result = probeIdentities(supported, flags);
    if (result.error) {
      console.error('error: could not read identity markers; run '
        + '`lycheedev instances --identify` after `/dev auto identify` in each game window');
      return { error: true };
    }
    const byHwnd = new Map(result.list.map((entry) => [String(entry.hwnd), entry]));
    probed = supported.map((instance) => {
      const entry = byHwnd.get(String(instance.hwnd));
      return entry && entry.identity ? { ...instance, identity: entry.identity } : instance;
    });
  }

  const matched = matchIdentity(probed, query);
  if (matched.error) {
    console.error(`error: ${matched.error}`);
    (matched.candidates || probed).forEach((item, index) => console.log(formatInstance(item, index)));
    return { error: true };
  }
  return { instance: matched.instance };
}

function toTarget(instance) {
  return {
    hwnd: instance.hwnd ? `0x${instance.hwnd.toString(16)}` : null,
    pid: instance.pid ? String(instance.pid) : null,
    instance,
  };
}

/**
 * `lycheedev use [index|--character <name>]`: keep one running client as the
 * default target.
 *
 * The pin stores the build, the instance ordinal and — when the window showed
 * its identity marker — the character. The character is the durable half: pids
 * and window handles change every launch, and an ordinal shifts as soon as the
 * windows are opened in a different order.
 */
function runUse(flags, rest) {
  let instances = findInstances({ pythonBin: flags.python || null });
  let shape = describeAmbiguity(instances);
  let supported = shape.supported;

  if (flags.clear) {
    updateConfig({ pinnedInstance: null });
    console.log('cleared the pinned client');
    return 0;
  }
  if (supported.length === 0) {
    console.error('error: no supported World of Warcraft instance is running');
    if (instances.length > 0) {
      instances.forEach((item, index) => console.log(formatInstance(item, index)));
    }
    return 1;
  }

  // Probing is read-only capture that types nothing, so do it whenever it could
  // resolve an ambiguity. An explicit index or character already is the decision,
  // so probing then would only add seconds of waiting for no new information.
  const decided = Boolean(flags.character) || rest.length > 0 || flags.instance !== undefined;
  const wantsIdentity = Boolean(flags.character) || Boolean(flags.identify)
    || (shape.sameBuildCollision && !decided);
  if (wantsIdentity) {
    const probed = probeIdentities(supported, flags);
    if (probed.error) {
      if (flags.character) {
        console.error(`error: ${probed.error}`);
        return 1;
      }
      // Without an explicit name request the pin can still be taken by ordinal.
    } else {
      const byHwnd = new Map(probed.list.map((entry) => [String(entry.hwnd), entry]));
      instances = instances.map((instance) => {
        const entry = byHwnd.get(String(instance.hwnd));
        return entry && entry.identity ? { ...instance, identity: entry.identity } : instance;
      });
      supported = instances.filter((item) => item.supported);
      shape = describeAmbiguity(instances);
    }
  }

  // Pinning by name is the recommended path when several windows share a build.
  if (flags.character) {
    const matched = matchIdentity(supported, flags.character);
    if (matched.error) {
      console.error(`error: ${matched.error}`);
      (matched.candidates || supported).forEach((item, index) => console.log(formatInstance(item, index)));
      return 1;
    }
    return savePin(matched.instance);
  }

  // Only windows that report the same character (or no marker at all) are
  // genuinely indistinguishable; those still get asked for a single window.
  if (shape.sameBuildCollision) {
    for (const entry of shape.collisions) {
      for (const group of entry.sameCharacter) {
        const label = group[0].flavorLabel || entry.flavorId;
        console.error(`error: ${group.length} ${label} clients cannot be told apart`);
        console.error(`       close all but one, or close these pids: `
          + group.map((i) => i.pid).join(', '));
      }
    }
    console.error('       or run `/dev auto identify` in each game window and then '
      + '`lycheedev use --character <name>`');
    supported.forEach((item, index) => console.log(formatInstance(item, index)));
    return 1;
  }

  const requested = rest.length > 0
    ? Number(rest[0])
    : (flags.instance !== undefined ? Number(flags.instance) : null);
  let chosen;
  let ordinal;
  if (requested !== null) {
    chosen = supported[requested];
    if (!chosen) {
      console.error(`error: instance ${requested} is out of range (0..${supported.length - 1})`);
      supported.forEach((item, index) => console.log(formatInstance(item, index)));
      return 1;
    }
    ordinal = supported
      .slice(0, requested)
      .filter((item) => item.flavorId === chosen.flavorId)
      .length;
  } else {
    if (supported.length > 1) {
      console.error('error: several clients are running; pass an index, e.g. `lycheedev use 0`');
      supported.forEach((item, index) => console.log(formatInstance(item, index)));
      return 1;
    }
    [chosen] = supported;
    ordinal = 0;
  }

  return savePin(chosen, ordinal);
}

/** Persist a pin, preferring the character when the window reported one. */
function savePin(chosen, ordinal = 0) {
  const pin = { flavor: chosen.flavorId, ordinal };
  // The character is the durable half of the pin; the ordinal stays as a
  // fallback for a build whose windows never showed a marker.
  const character = identityKey(chosen);
  if (character) pin.character = character;

  updateConfig({ pinnedInstance: pin });
  const label = chosen.flavorLabel || chosen.flavorFolder;
  const name = identityLabel(chosen);
  console.log(`pinned ${label}${name ? ` (${name})` : ''} (pid ${chosen.pid})`);
  console.log('commands now target it; pass --instance <index> or --character <name> to override');
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
    case 'instances':
      return runInstances(flags);
    case 'use':
      return runUse(flags, rest);
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

  // Commands that type into the game must target a live window, and the
  // executable name cannot tell two instances apart, so resolve which running
  // instance to use before forwarding.
  const runtime = ['send', 'capture', 'run', 'bugs'].includes(command)
    ? resolveRuntime(flags)
    : null;
  if (runtime && runtime.error) return 1;
  const live = runtime && runtime.hwnd
    ? runtime
    : null;

  switch (command) {
    case 'send': {
      const text = rest.join(' ');
      if (!text) {
        console.error('error: send needs the command text, e.g. `lycheedev send "/dev auto status x"`');
        return 1;
      }
      if (!live) return 1;
      return forward(['send', ...targetArgs(live), '--text', text, ...passthrough(flags)]);
    }
    case 'capture':
      if (!live) return 1;
      return forward(['capture', ...targetArgs(live), ...passthrough(flags)]);
    case 'run':
      if (!flags.task) {
        console.error('error: run needs --task <id>');
        return 1;
      }
      if (!live) return 1;
      if (!client.svPath) {
        console.error('error: no SavedVariables path recorded for this client; re-run `lycheedev install`');
        return 1;
      }
      return forward(['run', ...forwarded, ...targetArgs(live), '--task', flags.task, '--sv', client.svPath,
        ...passthrough(flags)]);
    case 'bugs':
      if (!flags.count) {
        console.error('error: bugs needs --count <1-100>');
        return 1;
      }
      if (!live) return 1;
      if (!client.svPath) {
        console.error('error: no SavedVariables path recorded for this client; re-run `lycheedev install`');
        return 1;
      }
      return forward(['bugs', ...targetArgs(live), '--count', String(flags.count), '--sv', client.svPath,
        ...passthrough(flags)]);
    case 'ack': {
      if (!flags.ticket || !flags.status) {
        console.error('error: ack needs --ticket <t> --status received|failed');
        return 1;
      }
      // An acknowledgement also has to reach a live window.
      const ackTarget = resolveRuntime(flags);
      if (ackTarget.error) return 1;
      return forward(['ack', ...targetArgs(ackTarget), '--ticket', flags.ticket,
        '--status', flags.status, ...passthrough(flags)]);
    }
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
      'ticket', 'status', 'json', 'help', 'version', 'data-dir', 'installation', 'instance'];
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
