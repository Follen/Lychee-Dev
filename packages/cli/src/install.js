/** `lycheedev install` / `update`: dependency, skill and addon installation. */

import path from 'node:path';

import { installAddon, installedVersion, payloadVersion } from './addon.js';
import { loadConfig, setClient, updateConfig } from './config.js';
import { addonFolderName, exists } from './paths.js';
import { depsInstalled, findPython, installDeps } from './python.js';
import { installSkill } from './skill.js';
import { detectWowRoots, findSavedVariablesCandidates, scanClients } from './wow.js';

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
        error: `${resolved} has no _retail_/_classic_/_classic_arena_ folder; `
          + 'point --wow-root at the folder that contains them',
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
  let defaultClient = config.defaultClient || null;

  for (const client of resolved.clients) {
    if (!['retail', 'classic', 'titan'].includes(client.id)) {
      console.log(`\n${client.label}: skipped (unsupported by the addon)`);
      continue;
    }
    step(`${client.label}: installing addon into ${client.folder}`, () => {
      if (!exists(client.addonsDir)) {
        console.log(`  skipped: ${client.addonsDir} does not exist yet`);
        return { ok: false, skipped: true };
      }
      const before = installedVersion(client.addonsDir);
      const installed = installAddon(client.addonsDir, { preserveRegistry: !flags['reset-registry'] });
      if (!installed.ok) {
        console.log(`  error: ${installed.error}`);
        return installed;
      }
      console.log(`  ${addonFolderName} -> ${installed.target}`);
      console.log(`  files: ${installed.files}, version: ${before ? `${before} -> ` : ''}${installed.version}`);
      if (installed.keptRegistry) console.log('  kept the existing task registry (local task data)');
      addonResults.push({ client: client.id, ...installed });
      if (!defaultClient) defaultClient = client.id;
      return installed;
    });
    const candidates = findSavedVariablesCandidates(resolved.root, client.folder);
    setClient(nextConfig, client.id, {
      flavorFolder: client.folder,
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
