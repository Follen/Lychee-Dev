// Static custody/contract checks only. Rust consumer execution remains separate.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import test from 'node:test';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const oracle = '3766f9cacb52e93a803a626187e2a1d54a445468';
const source = name => readFileSync(path.join(root, name), 'utf8');
// The Rust checkout need not contain historical Go objects. Source review is
// opt-in through an explicit repository; ordinary offline checks never call git.
const goCheckout = process.env.LYCHEEDEV_GO323_CHECKOUT;
const go = name => {
  const raw = execFileSync('git', ['show', `${oracle}:${name}`], {
    cwd: path.resolve(goCheckout), timeout: 10000, maxBuffer: 2 << 20,
  });
  const manifest = JSON.parse(source('tests/rust-parity/source-project-selection-test-map.json'));
  const frozen = manifest.evidence.find(item => item.path === name && item.commit === oracle);
  assert.ok(frozen, `missing fixed Go source identity: ${name}`);
  assert.equal(sha256(raw), frozen.sha256, name);
  return raw.toString('utf8');
};
const sha256 = raw => createHash('sha256').update(raw).digest('hex');

test('optional exact Go323 source review confirms project and numeric-error ordering', {
  skip: !goCheckout && 'Set LYCHEEDEV_GO323_CHECKOUT to a repository containing the exact fixed Go323 commit; source review is NOT_RUN without it',
}, () => {
  const entry = go('internal/command/entry.go');
  const project = go('internal/command/project.go');
  const research = go('internal/command/dispatch_source.go');
  assert.ok(entry.indexOf('opts, parseErr := parseOptions(args)') < entry.indexOf('project, err = selectProject(ctx, &opts)'));
  assert.ok(entry.indexOf('project, err = selectProject(ctx, &opts)') < entry.indexOf('switch route {'));
  assert.match(project, /opts\.snapshot = status\.Lock\.Selection\.ID/);
  assert.match(project, /opts\.project == "" && errors\.Is\(err, selection\.ErrProjectMissing\)/);
  assert.match(research, /opts\.snapshot == "" \|\| \(opts\.symbolID == ""\) == \(opts\.symbol == ""\)/);
  for (const flag of ['--max-bytes', '--max-lines', '--depth']) {
    assert.ok(entry.indexOf(`case "${flag}":`, entry.indexOf('func parseOptions(')) >= 0);
  }
  assert.match(entry, /case "--line", "--count", "--limit":/);
});

test('local advertised refs/context contract supports project fallback and explicit precedence', () => {
  const commands = JSON.parse(source('crates/lycheedev-cli/src/commands.json'));
  for (const command of ['source refs', 'source context']) {
    const contract = commands.find(item => item.path === command);
    assert.ok(contract.flags.includes('--project <directory>'));
    assert.match(contract.summary, /omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins/);
  }
});

test('project selection manifest seals the product slice and names every new Rust test', () => {
  const manifest = JSON.parse(source('tests/rust-parity/source-project-selection-test-map.json'));
  assert.equal(manifest.goOracle, oracle);
  for (const file of manifest.files) {
    const raw = readFileSync(path.join(root, file.path));
    assert.equal(raw.length, file.bytes, file.path);
    assert.equal(sha256(raw), file.sha256, file.path);
  }
  let count = 0;
  for (const suite of manifest.rustTests) {
    const actual = [...source(suite.path).matchAll(/#\[test\]\s*(?:#\[ignore[^\n]*\]\s*)?fn ([a-z0-9_]+)\(/g)].map(match => match[1]);
    assert.deepEqual(actual, suite.cases);
    count += actual.length;
  }
  assert.equal(count, 12);
});

test('new module resolves from relations.rs and concurrent fixtures never adopt an existing home', () => {
  const relations = source('crates/lycheedev-source/src/relations.rs');
  const relative = relations.match(/#\[path = "([^"]+)"\]\s*mod project_selection_tests;/)?.[1];
  assert.equal(relative, 'relations/project_selection_tests.rs');
  assert.ok(source(`crates/lycheedev-source/src/${relative}`).includes('fn context_selected_pin_handoff_'));
  for (const file of ['crates/lycheedev-source/src/relations/project_selection_tests.rs', 'crates/lycheedev-source/tests/refs_context.rs']) {
    const text = source(file);
    assert.match(text, /static NEXT: std::sync::atomic::AtomicU64/);
    assert.match(text, /NEXT\.fetch_add\(1, std::sync::atomic::Ordering::Relaxed\)/);
    assert.ok(text.indexOf('fs::create_dir(&home).expect(') < text.indexOf('workspace::initialize(&home)'));
  }
});
