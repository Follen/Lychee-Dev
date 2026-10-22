// Owned-process proof checks shared by the performance tools. These are the
// repository's current custody rules; they do not depend on any frozen product
// fixture or on the Go reference package.
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {lstatSync, readFileSync, readdirSync} from 'node:fs';
import {basename, dirname, join, relative, resolve} from 'node:path';

export const LIMITS = Object.freeze({file: 2 << 20, evidence: 64 << 20});
export const hash = bytes => createHash('sha256').update(bytes).digest('hex');

export function regular(path, maximum = LIMITS.file) {
  // Reject junction/symlink ancestors too, not only a symlink at the leaf.
  let cursor = resolve(path), depth = 0;
  for (;;) {
    assert(++depth <= 128 && !lstatSync(cursor).isSymbolicLink(), 'owned-proof.reparse_boundary');
    const parent = dirname(cursor); if (parent === cursor) break; cursor = parent;
  }
  const stat = lstatSync(path);
  assert(stat.isFile() && !stat.isSymbolicLink() && stat.size <= maximum, `owned-proof.invalid_file: ${path}`);
  const bytes = readFileSync(path);
  assert.equal(bytes.length, stat.size, `owned-proof.file_changed: ${path}`);
  return bytes;
}

export function processReaped(result) {
  const p = result.proof;
  return result.handlesReaped === true && p?.rootReaped === true && p.treeReaped === true
    && p.handlesClosed === true && p.toolIdentityLocksClosed === true && p.outputReadersComplete === true && p.activeProcesses === 0;
}

export function processPassed(result) {
  const p = result.proof;
  return result.status === 0 && !result.error && processReaped(result)
    && !p.forced && !p.error && !p.outputLimit && !p.deadlineHit && !p.cancelled && !p.canceled && !result.signal;
}

// Authenticate one sealed .tmp/rust-disk owner receipt and every sealed log it
// claims. The caller supplies the explicit repository root and expected owner.
export function verifyOwnerEvidence(repository, receiptPath, expectedOwner, {receiptMaximumBytes = LIMITS.file} = {}) {
  assert(Number.isSafeInteger(receiptMaximumBytes) && receiptMaximumBytes > 0 && receiptMaximumBytes <= 16 << 20, 'owned-proof.owner_receipt_budget');
  const path = resolve(receiptPath), bytes = regular(path, receiptMaximumBytes), receipt = JSON.parse(bytes);
  assert.equal(receipt.schema, 'lycheedev.disk-owner.v1');
  assert(/^[a-f0-9-]{36}$/.test(receipt.id ?? ''), 'owned-proof.owner_id');
  const directory = join(resolve(repository), '.tmp/rust-disk/evidence', receipt.id);
  assert.equal(path, join(directory, 'receipt.json'), 'owned-proof.owner_receipt_path');
  assert.equal(receipt.directory, join(resolve(repository), '.tmp/rust-scratch', receipt.id));
  if (expectedOwner) assert.equal(receipt.owner, expectedOwner);
  assert.equal(receipt.category, 'scratch'); assert.equal(receipt.activePid, null); assert.equal(receipt.handlesReaped, true);
  assert(['succeeded', 'retired'].includes(receipt.state) && typeof receipt.sealedAt === 'string', 'owned-proof.owner_not_sealed');
  assert.equal(hash(Buffer.from(JSON.stringify(receipt.command))), receipt.commandSHA256, 'owned-proof.owner_command_identity');
  assert(Array.isArray(receipt.sealedLogs) && receipt.sealedLogs.length > 0 && receipt.sealedLogs.length <= 2048, 'owned-proof.sealed_log_count');
  const logs = new Map(), hashes = {}; let total = 0;
  for (const row of receipt.sealedLogs) {
    assert(typeof row.path === 'string' && dirname(row.path) === directory && row.original === row.path && basename(row.path) !== 'receipt.json', 'owned-proof.foreign_owner_log');
    assert(!logs.has(row.path), 'owned-proof.duplicate_sealed_log');
    const length = lstatSync(row.path).size; total += length;
    assert(length <= 16 << 20 && total <= LIMITS.evidence, 'owned-proof.sealed_log_budget');
    const content = regular(row.path, 16 << 20); assert.equal(hash(content), row.sha256, 'owned-proof.sealed_log_identity');
    logs.set(row.path, content); hashes[relative(resolve(repository), row.path)] = row.sha256;
  }
  assert.deepEqual(receipt.logHashes, hashes, 'owned-proof.sealed_log_index');
  assert.deepEqual(readdirSync(directory).filter(name => name !== 'receipt.json').sort(), [...logs.keys()].map(path => basename(path)).sort(), 'owned-proof.sealed_log_set');
  const json = name => { const content = logs.get(join(directory, name)); assert(content, `owned-proof.sealed_log_missing: ${name}`); return JSON.parse(content); };
  const processes = [];
  for (const [requestPath, requestBytes] of logs) if (/process-[a-f0-9-]+\.request\.json$/.test(requestPath)) {
    const request = JSON.parse(requestBytes), stem = requestPath.slice(0, -'.request.json'.length);
    for (const [key, suffix] of [['stdout', '.stdout.bin'], ['stderr', '.stderr.bin'], ['receipt', '.receipt.json']]) {
      assert.equal(request[key], stem + suffix, 'owned-proof.process_log_target'); assert(logs.has(request[key]), 'owned-proof.process_log_missing');
    }
    const wrapper = json(basename(requestPath) + '.wrapper.json'), proof = JSON.parse(logs.get(request.receipt));
    assert.equal(wrapper.status, 0); assert(!wrapper.signal && !wrapper.error, 'owned-proof.wrapper_failed');
    assert.equal(proof.schema, 'lycheedev.owned-process.v1');
    assert(processReaped({handlesReaped: true, proof}), 'owned-proof.build_cleanup_unverified');
    assert.equal(proof.exitCode, 0);
    assert(!proof.error && !proof.deadlineHit && !proof.outputLimit && !proof.cancelled && !proof.canceled, 'owned-proof.build_process_failed');
    // Reuse, without broadening, the owned controller's reviewed compiler-only
    // helper exception. Fixture processes themselves never get this exception.
    assert(!proof.forced || proof.knownAuxiliaryCleanup === true && (request.msvcCompile === true || request.cargoBuild === true), 'owned-proof.build_process_forced');
    processes.push({request, proof});
  }
  assert(processes.length > 0, 'owned-proof.no_owned_build_process');
  return {receipt, receiptSHA256: hash(bytes), logs, json, processes};
}
