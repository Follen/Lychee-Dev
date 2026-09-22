// CI-only interpreter builder. No installed Lua, user PATH, or package is changed.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdirSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { resolve, join } from 'node:path';
import { spawnSync } from 'node:child_process';

const output = process.argv[2];
const compiler = process.argv[3] || 'gcc';
if (!output) throw new Error('Pass a new output directory and optional C compiler');
const root = resolve(output);
mkdirSync(root); // Exclusive creation; never reuse or erase another tool directory.
const sourceURL = 'https://www.lua.org/ftp/lua-5.1.5.tar.gz';
// Published SHA256: https://www.lua.org/ftp/
const sourceSHA256 = '2640fc56a795f29d28ef15e13c34a47e223960b0240e8cb0a82d9b0738695333';
const response = await fetch(sourceURL, { signal: AbortSignal.timeout(30000) });
assert(response.ok, `source download HTTP ${response.status}`);
const chunks = []; let count = 0;
for await (const chunk of response.body) {
  count += chunk.length;
  assert(count <= 1024 * 1024, 'source download exceeds budget');
  chunks.push(chunk);
}
const source = Buffer.concat(chunks);
assert.equal(createHash('sha256').update(source).digest('hex'), sourceSHA256);
writeFileSync(join(root, 'lua-5.1.5.tar.gz'), source, { flag: 'wx' });
function run(command, args, cwd = root) {
  const result = spawnSync(command, args, { cwd, shell: false, windowsHide: true, encoding: 'utf8', timeout: 120000, maxBuffer: 4 * 1024 * 1024 });
  if (result.error || result.status !== 0) throw new Error(`${command}: ${result.error || result.status}\n${result.stdout}\n${result.stderr}`);
  return result.stdout + result.stderr;
}
run('tar', ['-xzf', join(root, 'lua-5.1.5.tar.gz'), '-C', root]);
const sourceRoot = join(root, 'lua-5.1.5', 'src');
const sources = readdirSync(sourceRoot).filter(name => name.endsWith('.c') && !['luac.c', 'print.c'].includes(name)).sort();
assert(sources.includes('lua.c') && sources.includes('lvm.c'));
const executable = join(root, process.platform === 'win32' ? 'lua.exe' : 'lua');
const compilerVersion = run(compiler, ['--version']);
run(compiler, ['-O2', '-o', executable, ...sources, ...(process.platform === 'win32' ? [] : ['-lm'])], sourceRoot);
const version = run(executable, ['-v']).trim();
assert.match(version, /^Lua 5\.1\.5\s/);
const executableSHA256 = createHash('sha256').update(readFileSync(executable)).digest('hex');
const report = { sourceURL, sourceSHA256, compiler, compilerVersion, executable, executableSHA256, version, platform: process.platform, arch: process.arch };
writeFileSync(join(root, 'build-report.json'), JSON.stringify(report, null, 2), { flag: 'wx' });
process.stdout.write(JSON.stringify(report) + '\n');
