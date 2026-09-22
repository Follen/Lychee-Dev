// Naming/import constraints for the 2.0 toolkit (regression ARC-01/ARC-02).
// 1. Go import graph: stdlib + go.mod modules only (no new dependencies), no
//    legacy tool entrypoints, no reversed internal->cmd/tests edges, no os/user.
// 2. os.UserHomeDir stays in the single declared home-resolution file.
// 3. Shipped code (npm launcher, addon Lua) must not reference legacy homes,
//    legacy entrypoints, network downloads or shell-interpreted launches.
// This is a text-level contract check, not an AST parse; it fails closed on the
// patterns it claims to cover and its scope is listed in the output.
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { dirname, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const repository = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const modulePath = 'github.com/follenfang/lycheedev';
const goRoots = ['cmd', 'internal', 'tests', 'tools'];
const shippedRoots = [join('packages', 'npm', 'lycheedev', 'bin'), 'addon'];
// Slash-joined to match the normalized relativePath used in comparisons.
const homeResolutionFile = ['internal', 'command', 'entry.go'].join('/');
const forbiddenImportPatterns = [
  /spf13\/cobra/i, /\bcobra\b/, /automation(\.py)?/, /packages\/cli/, /Lychee Dev skill/, /python/i,
];
const forbiddenShippedPatterns = [
  { pattern: /~?\/?\.wowdoc|wowdoc/i, why: 'legacy wowdoc entrypoint or home' },
  { pattern: /~?\/?\.wowdata|wowdata/i, why: 'legacy wowdata entrypoint or home' },
  { pattern: /wowdump/i, why: 'legacy wowdump entrypoint' },
  { pattern: /automation\.py/i, why: 'legacy Python automation kernel' },
  { pattern: /pip install/i, why: 'runtime dependency installation' },
  { pattern: /\bfetch\s*\(|node:https?|require\(['"]https?['"]\)|from ['"]https?['"]/, why: 'network download path in shipped code' },
  { pattern: /shell:\s*true/, why: 'shell-interpreted process launch' },
];

function walk(root, filter) {
  const found = [];
  const stack = [root];
  while (stack.length) {
    const directory = stack.pop();
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const path = join(directory, entry.name);
      if (entry.isDirectory()) stack.push(path);
      else if (entry.isFile() && filter(path)) found.push(path);
    }
  }
  return found.sort();
}

function goFiles() {
  const files = [];
  for (const root of goRoots) {
    const directory = join(repository, root);
    try { if (!statSync(directory).isDirectory()) continue; } catch { continue; }
    files.push(...walk(directory, path => path.endsWith('.go')));
  }
  return files;
}

function shippedFiles() {
  const files = [];
  for (const root of shippedRoots) {
    const directory = join(repository, root);
    files.push(...walk(directory, path => /\.(?:mjs|js|lua)$/.test(path)));
  }
  return files;
}

function goModModules() {
  const text = readFileSync(join(repository, 'go.mod'), 'utf8');
  const modules = [];
  for (const match of text.matchAll(/^\t([a-z0-9.\-]+\/\S+)\s+v\S+/gm)) modules.push(match[1]);
  return modules;
}

function importsOf(text) {
  const imports = [];
  for (const match of text.matchAll(/^import\s+(?:"([^"]+)"|\(([\s\S]*?)\))/gm)) {
    if (match[1]) { imports.push(match[1]); continue; }
    for (const line of match[2].split('\n')) {
      const entry = line.trim().replace(/\/\/.*$/, '').trim();
      const quoted = entry.match(/^(?:[\w.]+\s+)?"([^"]+)"$/);
      if (quoted) imports.push(quoted[1]);
      else if (entry !== '') throw new Error(`import-contract: unparseable import line: ${line}`);
    }
  }
  return imports;
}

const violations = [];
const modules = goModModules();
const externalImports = new Set();
for (const file of goFiles()) {
  const relativePath = relative(repository, file).replaceAll(sep, '/');
  const text = readFileSync(file, 'utf8');
  for (const imported of importsOf(text)) {
    externalImports.add(imported);
    for (const pattern of forbiddenImportPatterns) {
      if (pattern.test(imported)) violations.push(`${relativePath}: forbidden import ${imported} (${pattern})`);
    }
    const isStandard = !imported.split('/')[0].includes('.');
    const isOwn = imported === modulePath || imported.startsWith(`${modulePath}/`);
    const allowedModule = modules.some(module => imported === module || imported.startsWith(`${module}/`));
    if (!isStandard && !isOwn && !allowedModule) {
      violations.push(`${relativePath}: import ${imported} is not stdlib and not declared in go.mod`);
    }
    if ((relativePath.startsWith('internal/') || relativePath.startsWith('tools/')) &&
        (imported === `${modulePath}/cmd` || imported.startsWith(`${modulePath}/cmd/`) ||
         imported === `${modulePath}/tests` || imported.startsWith(`${modulePath}/tests/`))) {
      violations.push(`${relativePath}: reversed dependency on ${imported}`);
    }
    if (imported === 'os/user') violations.push(`${relativePath}: os/user is forbidden (single home resolution owns user roots)`);
  }
  if (text.includes('os.UserHomeDir(') && relativePath !== homeResolutionFile) {
    violations.push(`${relativePath}: os.UserHomeDir outside ${homeResolutionFile}`);
  }
}

for (const file of shippedFiles()) {
  const relativePath = relative(repository, file).replaceAll(sep, '/');
  const text = readFileSync(file, 'utf8');
  for (const { pattern, why } of forbiddenShippedPatterns) {
    const match = text.match(pattern);
    if (match) violations.push(`${relativePath}: shipped code references ${why} (${JSON.stringify(match[0])})`);
  }
}

const summary = {
  scope: {
    go: goRoots.map(root => `${root}/**/*.go`),
    shipped: shippedRoots.map(root => `${root}/**/*.{mjs,js,lua}`),
    rules: [
      'Go imports: stdlib + go.mod modules + own module only',
      'no legacy entrypoint/Python/Cobra imports',
      'no internal/tools -> cmd or tests reverse edges',
      'os.UserHomeDir only in internal/command/entry.go',
      'shipped code: no legacy homes/entrypoints, no downloads, no shell:true',
    ],
  },
  goFiles: goRoots.length,
  modules,
  imports: [...externalImports].sort(),
  violations,
};
process.stdout.write(`${JSON.stringify(summary, null, 2)}\n`);
if (violations.length) {
  process.stderr.write(`import-contract: ${violations.length} violation(s)\n`);
  process.exitCode = 1;
}
