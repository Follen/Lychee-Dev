// Execute the actual SQL extracted from the fixed Go source and Rust constants.
// This proves the storage statements, not Rust compilation or CLI parity.
import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {spawnSync} from 'node:child_process';
import {DatabaseSync} from 'node:sqlite';
import {fileURLToPath} from 'node:url';

const root = fileURLToPath(new URL('../../', import.meta.url));
const GO = '3766f9cacb52e93a803a626187e2a1d54a445468';
const oracle = spawnSync('git', ['show', `${GO}:internal/vault/metadata.go`], {cwd: root, encoding: 'utf8'});
assert.equal(oracle.status, 0, `fixed Go oracle unavailable: ${oracle.stderr}`);
const source = path => readFileSync(new URL(`../../${path}`, import.meta.url), 'utf8');
const rust = source('crates/lycheedev-core/src/metadata.rs');
const sqlStrings = [...oracle.stdout.matchAll(/"((?:INSERT INTO|UPDATE documents|DELETE FROM|CREATE TABLE IF NOT EXISTS)[^"\n]+)"/g)].map(match => match[1]);
const goSQL = prefix => {
  const matches = sqlStrings.filter(sql => sql.startsWith(prefix));
  assert.equal(matches.length, 1, `expected one pinned Go SQL statement: ${prefix}`);
  return matches[0];
};
const constant = name => {
  const match = rust.match(new RegExp(`const ${name}: &str = "([^"\\n]+)";`));
  assert.ok(match, `missing production constant ${name}`);
  return match[1];
};
const schema = constant('DELETED_GENERATIONS_SCHEMA');
const insert = constant('INSERT_DOCUMENT');
const remember = rust.match(/"(INSERT INTO deleted_generations[^"\n]+)"/)[1];
const pinned = {
  schema: goSQL('CREATE TABLE IF NOT EXISTS deleted_generations'),
  insert: goSQL('INSERT INTO documents'),
  update: goSQL('UPDATE documents'),
  remember: goSQL('INSERT INTO deleted_generations'),
  delete: goSQL('DELETE FROM documents'),
};
const documents = oracle.stdout.match(/`(CREATE TABLE documents[^`]+)`/)[1];
const snapshot = db => JSON.parse(JSON.stringify({
  documents: db.prepare('SELECT key,generation,hex(value) AS value FROM documents ORDER BY key').all(),
  deleted: db.prepare('SELECT key,generation FROM deleted_generations ORDER BY key').all(),
}));
function database(candidate) {
  const db = new DatabaseSync(':memory:');
  db.exec(documents);
  db.exec(candidate ? schema : pinned.schema);
  return db;
}
function create(db, candidate, key, value) {
  const bytes = Buffer.from(JSON.stringify(value));
  return candidate
    ? db.prepare(insert).run(key, bytes).changes
    : db.prepare(pinned.insert).run(key, key, bytes).changes;
}
function remove(db, candidate, key, generation) {
  // The ordinary Rust API validates CAS while holding BEGIN IMMEDIATE before
  // its unqualified tombstone/DELETE; historical Go qualifies both statements.
  if (candidate) {
    const current = db.prepare('SELECT generation FROM documents WHERE key=?').get(key);
    if (current?.generation !== generation) throw new Error('generation conflict');
    db.prepare(remember).run(key);
    return db.prepare('DELETE FROM documents WHERE key=?1').run(key).changes;
  }
  db.prepare(pinned.remember).run(key, generation);
  const changed = db.prepare(pinned.delete).run(key, generation).changes;
  if (changed !== 1) throw new Error('generation conflict');
  return changed;
}
function transaction(db, run) {
  db.exec('BEGIN IMMEDIATE');
  try { run(); db.exec('COMMIT'); }
  catch (error) { db.exec('ROLLBACK'); throw error; }
}
function methodSource(text, name) {
  const start = text.search(new RegExp(`\\n    (?:pub(?:\\([^)]*\\))? )?fn ${name}(?:<|\\()`));
  assert.ok(start >= 0, `missing metadata method ${name}`);
  const end = text.indexOf('\n    }\n', start);
  assert.ok(end > start, `missing metadata method end ${name}`);
  return text.slice(start, end);
}

test('default writable schema is exactly fixed Go323 and upgrades without resetting rows', () => {
  assert.equal(schema, pinned.schema);
  const db = new DatabaseSync(':memory:');
  try {
    db.exec(documents);
    db.prepare('INSERT INTO documents VALUES(?,7,?)').run('retained', Buffer.from('true'));
    db.exec(schema); db.exec(schema);
    assert.equal(db.prepare('PRAGMA user_version').get().user_version, 1);
    assert.equal(db.prepare('SELECT generation FROM documents').get().generation, 7);
    assert.equal(db.prepare('SELECT count(*) AS n FROM deleted_generations').get().n, 0);
    assert.match(db.prepare("SELECT sql FROM sqlite_master WHERE name='deleted_generations'").get().sql, /WITHOUT ROWID$/);
  } finally { db.close(); }
});

test('actual insert and tombstone SQL match Go across repeated delete/recreate and stale CAS', () => {
  const histories = [];
  for (const candidate of [false, true]) {
    const db = database(candidate), history = [];
    try {
      transaction(db, () => assert.equal(create(db, candidate, 'key', true), 1));
      for (let generation = 1; generation <= 4; generation++) {
        transaction(db, () => assert.equal(remove(db, candidate, 'key', generation), 1));
        history.push(snapshot(db));
        transaction(db, () => assert.equal(create(db, candidate, 'key', true), 1));
        assert.equal(db.prepare(pinned.update).run(Buffer.from('false'), 'key', generation).changes, 0);
        assert.throws(() => transaction(db, () => remove(db, candidate, 'key', generation)), /generation conflict/);
        history.push(snapshot(db));
      }
      histories.push(history);
    } finally { db.close(); }
  }
  assert.deepEqual(histories[1], histories[0]);
  assert.equal(histories[1].at(-1).documents[0].generation, 5);
  assert.equal(histories[1].at(-1).deleted[0].generation, 4);
});

test('failed second deletion atomically restores rows and previous tombstone history', () => {
  const histories = [];
  for (const candidate of [false, true]) {
    const db = database(candidate);
    try {
      transaction(db, () => { create(db, candidate, 'a', true); create(db, candidate, 'b', true); });
      transaction(db, () => { remove(db, candidate, 'a', 1); create(db, candidate, 'a', true); });
      const before = snapshot(db);
      assert.throws(() => transaction(db, () => { remove(db, candidate, 'a', 2); remove(db, candidate, 'b', 9); }), /generation conflict/);
      assert.deepEqual(snapshot(db), before);
      histories.push(before);
    } finally { db.close(); }
  }
  assert.deepEqual(histories[1], histories[0]);
});

test('failed DELETE statement rolls back a tombstone overwrite together with document deletion', () => {
  const db = database(true);
  try {
    transaction(db, () => create(db, true, 'key', true));
    transaction(db, () => { remove(db, true, 'key', 1); create(db, true, 'key', true); });
    const before = snapshot(db);
    db.exec("CREATE TRIGGER reject_delete BEFORE DELETE ON documents BEGIN SELECT RAISE(ABORT,'injected delete failure'); END");
    assert.throws(() => transaction(db, () => remove(db, true, 'key', 2)), /injected delete failure/);
    assert.deepEqual(snapshot(db), before);
  } finally { db.close(); }
});

test('all default creation sites use the shared generation-history INSERT', () => {
  const capture = source('crates/lycheedev-core/src/metadata/capture_write.rs');
  const checked = source('crates/lycheedev-core/src/metadata/evidence_catalog/checked_delete.rs');
  // These are the actual owning transactions. The checked recovery INSERT
  // moved into the catalog; put_serialized_referencing is a test-only helper.
  for (const [name, body] of [
    ['controlled publication', methodSource(rust, 'put_controlled_policy_with_check_dyn')],
    ['serialized publication', methodSource(rust, 'put_serialized_policy_dyn')],
    ['capture publication', methodSource(capture, 'put_capture_serialized_observed')],
    ['checked recovery', checked],
  ]) {
    assert.match(body, /TransactionBehavior::Immediate/, name);
    assert.match(body, /transaction\.execute\(\s*INSERT_DOCUMENT,/, name);
    assert.doesNotMatch(body, /VALUES\(\?1,1,\?2\)/, name);
  }
  assert.match(source('crates/lycheedev-core/src/metadata/controlled_open.rs'), /checked_sql_with_check\([^;]+connection\.execute_batch\(DELETED_GENERATIONS_SCHEMA\)/s);
});

test('default removal sites save generation in their existing write transaction', () => {
  const consume = source('crates/lycheedev-core/src/metadata/source_controlled.rs');
  const checked = source('crates/lycheedev-core/src/metadata/evidence_catalog/checked_delete.rs');
  assert.match(methodSource(rust, 'consume'), /self\.consume_controlled\(/);
  assert.match(methodSource(rust, 'delete_checked_record_controlled_with_raw'), /evidence_catalog::checked_delete\(/);
  for (const [name, body] of [
    ['ordinary delete', methodSource(rust, 'delete')],
    ['controlled consume', methodSource(consume, 'consume_controlled_observed')],
    ['catalog checked delete', checked],
  ]) {
    const begin = body.indexOf('TransactionBehavior::Immediate');
    const remembered = body.indexOf('remember_deleted_generation(&transaction, key)');
    const deleted = body.indexOf('transaction.execute("DELETE FROM documents');
    const commit = body.search(/transaction\.(?:commit\(\)|execute_batch\("COMMIT"\))/);
    assert.ok(begin >= 0 && begin < remembered && remembered < deleted && deleted < commit,
      `${name}: BEGIN IMMEDIATE -> remember generation -> DELETE -> COMMIT`);
  }
  assert.match(consume, /checked_sql\(control, control\.deadline\(\), on_busy, \|\| \{\s*remember_deleted_generation\(&transaction, key\)/);
});
