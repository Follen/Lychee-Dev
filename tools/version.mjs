import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const repository = resolve(dirname(fileURLToPath(import.meta.url)), '..');
function replaceOne(text, pattern, replacement, path) {
  if ([...text.matchAll(pattern)].length !== 1) throw new Error(`version.invalid_target: ${path}`);
  return text.replace(pattern, replacement);
}

// Plan every update before writing any file. --check never repairs drift.
export function synchronizeVersion(root, write = false) {
  const source = JSON.parse(readFileSync(join(root, 'release/version.json'), 'utf8'));
  if (source.schema !== 'lycheedev.version.v1' || Object.keys(source).sort().join(',') !== 'schema,version' ||
      typeof source.version !== 'string' || !/^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-(?:[A-Za-z][A-Za-z0-9-]*|0|[1-9][0-9]*)(?:\.(?:[A-Za-z][A-Za-z0-9-]*|0|[1-9][0-9]*))*)?$/.test(source.version)) {
    throw new Error('version.invalid_source');
  }
  const version = source.version;
  const plans = [];
  function target(path, transform) {
    const original = readFileSync(join(root, path), 'utf8');
    const normalized = original.replaceAll('\r\n', '\n');
    const expected = transform(normalized);
    if (normalized !== expected) plans.push({ path, original, expected });
  }
  if (existsSync(join(root, 'Cargo.toml'))) {
    target('Cargo.toml', text => replaceOne(text, /(\[workspace\.package\][\s\S]*?\nversion\s*=\s*)"[^"\n]*"/g, `$1"${version}"`, 'Cargo.toml'));
    const names = ['cli', 'core', 'source', 'emmy-adapter', 'binding-contracts', 'data', 'live', 'delivery', 'windows'].map(name => `lycheedev-${name}`);
    const synchronizeLock = (text, expectedNames, path) => {
      const seen = [];
      const result = text.replace(/\[\[package\]\]\n[\s\S]*?(?=\n\[\[package\]\]|$)/g, block => {
        const name = /^name = "([^"\n]+)"$/m.exec(block)?.[1];
        if (!name?.startsWith('lycheedev-') || name === 'lycheedev-local-pure') return block;
        if (!expectedNames.includes(name) || seen.includes(name) || /^(?:source|checksum) =/m.test(block)) throw new Error(`version.invalid_owned_lock: ${path}`);
        seen.push(name);
        return replaceOne(block, /^(version = )"[^"\n]+"$/gm, `$1"${version}"`, path);
      });
      if (seen.length !== expectedNames.length) throw new Error(`version.invalid_owned_lock: ${path}`);
      return result;
    };
    target('Cargo.lock', text => synchronizeLock(text, names, 'Cargo.lock'));
    target('crates/lycheedev-binding-contracts/Cargo.toml', text => replaceOne(text, /^version = "[^"\n]+"$/gm, `version = "${version}"`, 'binding-contracts'));
    target('crates/lycheedev-emmy-adapter/Cargo.toml', text => replaceOne(text, /^(lycheedev-binding-contracts = \{ path = "\.\.\/lycheedev-binding-contracts", version = )"=[^"\n]+"/gm, `$1"=${version}"`, 'adapter'));
  }
  if (existsSync(join(root, 'release/npm-channel.json'))) {
    target('release/npm-channel.json', text => replaceOne(text, /("version"\s*:\s*)"[^"\n]*"/g, `$1"${version}"`, 'npm-channel.json'));
  }
  target('packages/npm/lycheedev/package.json', text => {
    const pkg = JSON.parse(text);
    if (pkg.name !== 'lycheedev' || typeof pkg.version !== 'string') throw new Error('version.invalid_package');
    return replaceOne(text, /("version"\s*:\s*)"[^"\n]*"/g, `$1"${version}"`, 'package.json');
  });
  target('packages/npm/lycheedev/package-lock.json', text => {
    const lock = JSON.parse(text);
    if (lock.name !== 'lycheedev' || lock.lockfileVersion !== 3 || lock.packages?.['']?.name !== 'lycheedev') throw new Error('version.invalid_lockfile');
    if (lock.version === version && lock.packages[''].version === version) return text;
    lock.version = version;
    lock.packages[''].version = version;
    return JSON.stringify(lock, null, 2) + '\n';
  });
  target("addon/Lychee Dev.toc", text => replaceOne(text, /^## Version: .*$/gm, `## Version: ${version}`, "addon/Lychee Dev.toc"));
  target('addon/Core/Runtime.lua', text => replaceOne(text, /^ns\.Release = "[^"\n]*"$/gm, `ns.Release = "${version}"`, 'Runtime.lua'));
  target('tests/addon/env.lua', text => replaceOne(text,
    /(GetAddOnMetadata = function[\s\S]*?\breturn ")[^"]+(")/g,
    (_, prefix, suffix) => `${prefix}${version}${suffix}`, 'tests/addon/env.lua'));
  target('tests/addon/t_about.lua', text => replaceOne(text,
    /(aboutPage\.metaItems\.version\.value:GetText\(\) == ")[^"]+(")/g,
    (_, prefix, suffix) => `${prefix}${version}${suffix}`, 'tests/addon/t_about.lua'));
  if (write) for (const plan of plans) writeFileSync(join(root, plan.path), plan.expected);
  else if (plans.length) throw new Error(`version.drift: ${plans.map(plan => plan.path).join(', ')}`);
  return { version, changed: plans.map(plan => plan.path), mode: write ? 'write' : 'check' };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    if (process.argv.length !== 3 || !['--check', '--write'].includes(process.argv[2])) throw new Error('Usage: node tools/version.mjs --check|--write');
    process.stdout.write(JSON.stringify(synchronizeVersion(repository, process.argv[2] === '--write')) + '\n');
  } catch (error) { process.stderr.write(`${error.message}\n`); process.exitCode = 1; }
}
