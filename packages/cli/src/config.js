/** Persistent CLI configuration (~/.lycheedev/config.json). */

import { configPath, readJson, writeJson } from './paths.js';

const EMPTY = {
  wowRoot: null,
  clients: {},
  pythonBin: null,
  installedAt: null,
  packageVersion: null,
};

export function loadConfig() {
  const stored = readJson(configPath(), null);
  if (!stored || typeof stored !== 'object') {
    return { ...EMPTY, clients: {} };
  }
  return {
    ...EMPTY,
    ...stored,
    clients: stored.clients && typeof stored.clients === 'object' ? stored.clients : {},
  };
}

export function saveConfig(config) {
  writeJson(configPath(), config);
  return config;
}

export function updateConfig(patch) {
  const config = loadConfig();
  const next = { ...config, ...patch };
  if (patch.clients) {
    next.clients = { ...config.clients, ...patch.clients };
  }
  return saveConfig(next);
}

export function setClient(config, clientId, value) {
  config.clients = { ...config.clients, [clientId]: value };
  return config;
}

/**
 * Resolve which WoW client to act on. An explicit id wins; otherwise the only
 * installed client is used, and an ambiguous set is reported instead of guessed.
 */
export function resolveClient(config, requested) {
  const ids = Object.keys(config.clients).sort();
  if (requested) {
    const client = config.clients[requested];
    if (!client) {
      return { error: `unknown client '${requested}'; known: ${ids.join(', ') || 'none'}` };
    }
    return { id: requested, client };
  }
  if (ids.length === 0) {
    return { error: 'no World of Warcraft client is configured yet; run `lycheedev install`' };
  }
  if (ids.length > 1) {
    return { error: `several clients are configured (${ids.join(', ')}); pass --client <id>` };
  }
  return { id: ids[0], client: config.clients[ids[0]] };
}
