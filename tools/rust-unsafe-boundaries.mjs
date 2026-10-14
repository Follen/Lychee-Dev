// Lexical inventory of owned Rust, not an AST, macro-expansion or dependency audit.
// The compiler's crate-level forbid remains authoritative for business code.
import {createHash} from 'node:crypto';
import {lstatSync, readFileSync, readdirSync} from 'node:fs';
import {join, resolve} from 'node:path';
import {fileURLToPath} from 'node:url';

const SAFE_CRATES = ['cli', 'core', 'data', 'delivery', 'live', 'source'];
const WINDOWS_FIXTURES = new Set([
  'crates/lycheedev-data/tests/assets_business.rs',
  'crates/lycheedev-source/tests/native_quota_peer.rs',
  'crates/lycheedev-live/tests/passive_windows.rs',
  'crates/lycheedev-live/tests/runtime_windows.rs',
  'crates/lycheedev-live/tests/sealed_memory.rs',
  'crates/lycheedev-live/tests/native_e2e.rs',
]);
const GUARD = ['#', '!', '[', 'forbid', '(', 'unsafe_code', ')', ']'];
const MAX_FILES = 4096, MAX_FILE_BYTES = 1 << 20, MAX_TOTAL_BYTES = 32 << 20;

export function tokens(source) {
  const result = [];
  let i = 0, line = 1;
  const skip = end => {
    for (; i < end; i++) if (source[i] === '\n') line++;
  };
  while (i < source.length) {
    if (/\s/u.test(source[i])) { skip(i + 1); continue; }
    if (source.startsWith('//', i)) {
      const end = source.indexOf('\n', i + 2);
      skip(end < 0 ? source.length : end); continue;
    }
    if (source.startsWith('/*', i)) {
      let end = i + 2, depth = 1;
      while (end < source.length && depth) {
        if (source.startsWith('/*', end)) { depth++; end += 2; }
        else if (source.startsWith('*/', end)) { depth--; end += 2; }
        else end++;
      }
      if (depth) throw new Error(`unterminated comment at line ${line}`);
      skip(end); continue;
    }
    const rest = source.slice(i);
    const raw = /^(?:br|cr|r)(#*)"/.exec(rest);
    if (raw) {
      const close = '"' + raw[1];
      const end = source.indexOf(close, i + raw[0].length);
      if (end < 0) throw new Error(`unterminated raw string at line ${line}`);
      skip(end + close.length); continue;
    }
    if (source[i] === '"') {
      let end = i + 1;
      while (end < source.length && source[end] !== '"') {
        end += source[end] === '\\' ? 2 : 1;
      }
      if (end >= source.length) throw new Error(`unterminated string at line ${line}`);
      skip(end + 1); continue;
    }
    // A lifetime has no closing apostrophe. Match character literals first.
    const character = /^'(?:\\(?:u\{[\da-fA-F_]+\}|x[\da-fA-F]{2}|[^\n])|[^'\\\n])'/u.exec(rest);
    if (character) { skip(i + character[0].length); continue; }
    const identifier = /^(?:r#)?[\p{ID_Start}_][\p{ID_Continue}]*/u.exec(rest);
    const value = identifier ? identifier[0] : source[i];
    result.push({value, line});
    i += value.length;
  }
  return result;
}

export function inspectSource(source) {
  const lexed = tokens(source), sites = [];
  for (let i = 0; i < lexed.length; i++) {
    const {value, line} = lexed[i];
    const next = lexed[i + 1]?.value;
    if (value === 'unsafe') {
      let kind;
      if (next === '{') kind = 'block';
      else if (next === '(') kind = 'attribute';
      else if (['fn', 'impl', 'trait'].includes(next)) kind = next;
      else if (next === 'extern') kind = lexed[i + 2]?.value === 'fn' ? 'extern_fn' : 'extern_block';
      else kind = 'unclassified';
      sites.push({line, kind});
    } else if (['asm', 'global_asm', 'naked_asm'].includes(value) && next === '!') {
      sites.push({line, kind: 'assembly_macro'});
    } else if (['no_mangle', 'export_name', 'link_section'].includes(value)
        && lexed[i - 1]?.value === '[') {
      sites.push({line, kind: 'legacy_unsafe_attribute'});
    } else if (value === 'extern' && next === '{' && lexed[i - 1]?.value !== 'unsafe') {
      sites.push({line, kind: 'legacy_extern_block'});
    }
  }
  return {
    // Requiring this unconditional first attribute also rejects a cfg_attr guard.
    forbidden: GUARD.every((value, i) => lexed[i]?.value === value),
    sites,
  };
}

export function inventory(root) {
  const files = [], violations = [], guards = [];
  let count = 0, bytes = 0;
  function visit(relative) {
    if (++count > MAX_FILES) throw new Error('unsafe_inventory.file_budget');
    const path = join(root, relative), stat = lstatSync(path);
    if (stat.isSymbolicLink()) throw new Error(`unsafe_inventory.symlink:${relative}`);
    if (stat.isDirectory()) {
      for (const child of readdirSync(path).sort()) visit(`${relative}/${child}`);
      return;
    }
    if (!stat.isFile()) throw new Error(`unsafe_inventory.non_regular:${relative}`);
    if (!relative.endsWith('.rs')) return;
    if (stat.size > MAX_FILE_BYTES || (bytes += stat.size) > MAX_TOTAL_BYTES)
      throw new Error('unsafe_inventory.byte_budget');
    const source = readFileSync(path, 'utf8');
    const {forbidden, sites} = inspectSource(source);
    const businessRoot = SAFE_CRATES.some(name => relative ===
      `crates/lycheedev-${name}/src/${name === 'cli' ? 'main' : 'lib'}.rs`);
    if (businessRoot) {
      guards.push({path: relative, forbidUnsafeCode: forbidden});
      if (!forbidden) violations.push(`missing unconditional crate forbid: ${relative}`);
    }
    if (!sites.length) return;
    let scope = 'unapproved_owned_source';
    if (relative.startsWith('crates/lycheedev-windows/')) scope = 'windows_boundary_pending_review';
    else if (WINDOWS_FIXTURES.has(relative)) scope = 'windows_fixture_pending_review';
    else violations.push(`unsafe outside inventoried Windows boundary: ${relative}`);
    if (sites.some(site => site.kind === 'unclassified'))
      violations.push(`unclassified unsafe syntax: ${relative}`);
    files.push({path: relative, scope,
      sha256: createHash('sha256').update(source).digest('hex'), sites});
  }
  visit('crates');
  for (const name of SAFE_CRATES) {
    if (!guards.some(guard => guard.path.startsWith(`crates/lycheedev-${name}/`)))
      violations.push(`missing business crate root: ${name}`);
  }
  const counts = {};
  for (const file of files) for (const site of file.sites)
    counts[site.kind] = (counts[site.kind] || 0) + 1;
  return {
    schema: 'lycheedev.rust-unsafe-boundaries.v1',
    scope: 'owned crates only; all cfg branches; source tokens, not macro expansions',
    dependencyAudit: 'not_run; third_party, vendored and registry dependencies excluded',
    safetyReview: 'not_claimed; pending_review entries are inventory, never exemptions or approval',
    compilerAndWindowsTests: 'not_run_by_this_tool',
    guards, counts, files, violations,
  };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2);
  if (args.length > 1 || (args.length && args[0] !== '--check'))
    throw new Error('usage: node tools/rust-unsafe-boundaries.mjs [--check]');
  const report = inventory(resolve(fileURLToPath(new URL('..', import.meta.url))));
  console.log(JSON.stringify(report, null, 2));
  if (args[0] === '--check' && report.violations.length) process.exitCode = 1;
}
