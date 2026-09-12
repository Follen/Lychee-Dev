/** `lycheedev doctor`: report what is installed and what is missing. */

import fs from 'node:fs';
import path from 'node:path';

import { installedVersion, payloadVersion } from './addon.js';
import { loadConfig } from './config.js';
import { addonFolderName, configPath, dataDir, exists, pythonDepsDir, skillTargetDir } from './paths.js';
import { automationScript, findPython, pythonEnv } from './python.js';
import { findSavedVariablesCandidates, scanClients } from './wow.js';

const OK = 'ok  ';
const WARN = 'warn';
const BAD = 'fail';

function line(status, label, detail) {
  const prefix = status === OK ? '  ok  ' : status === WARN ? ' warn ' : ' fail ';
  console.log(`${prefix} ${label.padEnd(28)} ${detail}`);
  return status;
}

export function runDoctor({ flags }) {
  const config = loadConfig();
  const results = [];
  const python = findPython(flags.python || config.pythonBin);

  console.log('lycheedev doctor\n');

  results.push(line(OK, 'node', process.version));

  results.push(python
    ? line(OK, 'python', `${python.bin} (${python.version})`)
    : line(BAD, 'python', 'not found; install Python 3 and re-run'));

  if (python) {
    const missing = ['zxingcpp', 'windows_capture'].filter((name) => {
      const target = path.join(pythonDepsDir(), name);
      return !exists(target);
    });
    if (missing.length === 0) {
      results.push(line(OK, 'python dependencies', pythonDepsDir()));
    } else if (missing.length === 2) {
      results.push(line(WARN, 'python dependencies',
        'not installed (live capture/send unavailable); run `lycheedev install`'));
    } else {
      results.push(line(WARN, 'python dependencies',
        `partial, missing: ${missing.join(', ')}; run ` + '`lycheedev install --force`'));
    }
    results.push(exists(automationScript)
      ? line(OK, 'automation helper', automationScript)
      : line(BAD, 'automation helper', 'missing from the package; reinstall lycheedev'));
  } else {
    results.push(line(BAD, 'python dependencies', 'skipped (no python)'));
    results.push(line(BAD, 'automation helper', 'skipped (no python)'));
  }

  if (exists(skillTargetDir())) {
    const files = fs.readdirSync(skillTargetDir());
    results.push(line(OK, 'skill installed', `${skillTargetDir()} (${files.length} entries)`));
  } else {
    results.push(line(WARN, 'skill installed', `missing; run \`lycheedev install\``));
  }

  const roots = flags['wow-root'] ? [path.resolve(flags['wow-root'])] : (config.wowRoot ? [config.wowRoot] : []);
  if (roots.length === 0 || !exists(roots[0])) {
    results.push(line(WARN, 'wow root', 'not configured; run `lycheedev install --wow-root <path>`'));
  } else {
    results.push(line(OK, 'wow root', roots[0]));
    for (const client of scanClients(roots[0])) {
      if (!['retail', 'classic', 'titan'].includes(client.id)) continue;
      const target = path.join(client.addonsDir, addonFolderName);
      if (!exists(target)) {
        results.push(line(WARN, `addon (${client.id})`, `not installed in ${client.addonsDir}`));
        continue;
      }
      const current = installedVersion(client.addonsDir);
      const shipped = payloadVersion();
      const detail = `${target} (installed ${current || '?'}, package ${shipped || '?'})`;
      results.push(current && shipped && current !== shipped
        ? line(WARN, `addon (${client.id})`, `${detail} - run \`lycheedev update\``)
        : line(OK, `addon (${client.id})`, detail));

      const candidates = findSavedVariablesCandidates(roots[0], client.folder);
      results.push(candidates.length
        ? line(OK, `savedvariables (${client.id})`, `${candidates.length} account(s), newest: ${candidates[0].account}`)
        : line(WARN, `savedvariables (${client.id})`, 'none yet; log into the game once'));
    }
  }

  console.log(`\nconfig: ${configPath()}`);
  console.log(`data  : ${dataDir()}`);
  if (python) {
    console.log(`python env PYTHONPATH=${pythonEnv().PYTHONPATH}`);
  }

  const failures = results.filter((entry) => entry === BAD).length;
  const warnings = results.filter((entry) => entry === WARN).length;
  console.log(`\n${failures} failure(s), ${warnings} warning(s)`);
  return failures > 0 ? 1 : 0;
}
