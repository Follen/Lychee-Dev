/** `lycheedev install` / `update`: dependency, skill and addon installation. */

import path from 'node:path';

import { installAddon, installedVersion, payloadVersion } from './addon.js';
import { loadConfig, setClient, updateConfig } from './config.js';
import { exists } from './paths.js';
import { depsInstalled, findPython, installDeps } from './python.js';
import { installSkill } from './skill.js';
import { detectWowRoots, findSavedVariablesCandidates, knownFolders, scanClients } from './wow.js';

function step(label, fn) {
  process.stdout.write(`\n` + `${label}\n`);
  return fn();
}

function flag(name, value) {
  return value ? `[${name}] ${value}` : `[${name}] -`;
}

/**
 * Resolve the WoW root: an explicit flag wins, then a saved config value, then
 * auto-detection. Ambiguity is reported instead of guessed so we never write
 * into the wrong installation.
 */
export function resolveWowRoot({ wowRoot, config }) {
  if (wowRoot) {
    const resolved = path.resolve(wowRoot);
    if (!exists(resolved)) return { error: `path does not exist: ${resolved}` };
    const clients = scanClients(resolved);
    if (clients.length === 0) {
      return {
        // List what detection actually accepts, generated from the same table,
        // so the message cannot drift from the behavior it describes.
        error: `${resolved} has no known client folder:\n  `
          + knownFolders().join('\n  ')
          + '\npoint --wow-root at the folder that contains them',
      };
    }
    return { root: resolved, clients };
  }
  if (config.wowRoot && exists(config.wowRoot)) {
    const clients = scanClients(config.wowRoot);
    if (clients.length > 0) return { root: config.wowRoot, clients, fromConfig: true };
  }
  const detected = detectWowRoots();
  if (detected.length === 1) {
    return { root: detected[0], clients: scanClients(detected[0]), detected: true };
  }
  if (detected.length > 1) {
    return {
      error: `found several World of Warcraft folders:\n  ${detected.join('\n  ')}\n`
        + 'pass --wow-root to choose one',
    };
  }
  return { error: 'no World of Warcraft folder found; pass --wow-root <path>' };
}

export function runInstall({ flags }) {
  const config = loadConfig();
  const resolved = resolveWowRoot({ wowRoot: flags['wow-root'], config });
  if (resolved.error) {
    console.error(`error: ${resolved.error}`);
    return 1;
  }

  console.log(`world of warcraft: ${resolved.root}`
    + `${resolved.detected ? ' (auto-detected)' : ''}${resolved.fromConfig ? ' (from config)' : ''}`);

  const result = step('installing python dependencies', () => {
    const python = findPython(flags.python);
    if (!python) return { ok: false, error: 'no Python 3 interpreter found on PATH' };
    console.log(`  python: ${python.bin} (${python.version})`);
    if (depsInstalled() && !flags.force) {
      console.log('  already installed (use --force to refresh)');
      return { ok: true, skipped: true };
    }
    return installDeps({ pythonBin: python.bin, quiet: true, onLine: (line) => console.log(`  ${line}`) });
  });

  const skillResult = step('installing skill', () => installSkill());

  const nextConfig = { ...config, wowRoot: resolved.root, pythonBin: flags.python || config.pythonBin };
  const addonResults = [];
  // Prefer retail as the default target so a routine command stays predictable;
  // otherwise take the first supported client that was actually updated.
  const preferredDefault = resolved.clients.some((client) => client.toc && client.id === 'retail')
    ? 'retail'
    : null;
  let defaultClient = config.defaultClient || preferredDefault || null;

  for (const client of resolved.clients) {
    // A build without a catalog is reported, not silently dropped: the folder
    // still exists and the user should know why nothing was installed there.
    if (!client.toc) {
      console.log(`\n${client.label} (${client.folder}): not served by the addon, skipped`);
      continue;
    }
    step(`${client.label}: installing addon into ${client.folder}`, () => {
      if (!exists(client.addonsDir)) {
        console.log(`  skipped: ${client.addonsDir} does not exist yet`);
        return { ok: false, skipped: true };
      }
      const before = installedVersion(client.addonsDir);
      const installed = installAddon(client.addonsDir, {
        preserveRegistry: !flags['reset-registry'],
        toc: client.toc,
      });
      if (!installed.ok) {
        console.log(`  error: ${installed.error}`);
        return installed;
      }
      console.log(`  ${client.toc} -> ${installed.target}`);
      console.log(`  files: ${installed.files}, version: ${before ? `${before} -> ` : ''}${installed.version}`);
      if (installed.keptRegistry) console.log('  kept the existing task registry (local task data)');
      addonResults.push({ client: client.id, ...installed });
      if (!defaultClient) defaultClient = client.id;
      return installed;
    });
    const candidates = findSavedVariablesCandidates(resolved.root, client.folder);
    setClient(nextConfig, client.id, {
      flavorFolder: client.folder,
      toc: client.toc,
      addonsDir: client.addonsDir,
      svPath: candidates[0] ? candidates[0].path : null,
      installedAt: new Date().toISOString(),
    });
  }

  updateConfig({ ...nextConfig, defaultClient });

  console.log('\nresult');
  console.log(`  python deps : ${result.ok ? (result.skipped ? 'present' : 'installed') : `failed (${result.error})`}`);
  console.log(`  skill       : ${skillResult.ok ? `${skillResult.files} files -> ${skillResult.target}` : `failed (${skillResult.error})`}`);
  console.log(`  addon       : ${addonResults.length} client(s) updated`);
  console.log(`  payload     : version ${payloadVersion()}`);

  const failed = !result.ok || !skillResult.ok || addonResults.length === 0;
  if (failed) {
    console.log('\nnot fully installed; run `lycheedev doctor` for details');
    return 1;
  }
  console.log('\nrestart World of Warcraft (or /reload) to load the addon, then run `lycheedev doctor`');
  return 0;
}
