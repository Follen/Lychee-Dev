// Later Rust ownership revisions are not 'unchanged 3.2.1 production'.
// Keep the original map authenticated separately; permit exactly these pinned
// successor source revisions, never a moving HEAD or replacement historical hash.
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {inspectSource} from '../../tools/rust-unsafe-boundaries.mjs';
const prefix = 'crates/lycheedev-live/src/go_mailbox_v1/';
const sha256 = source => createHash('sha256').update(source).digest('hex');
export const RETAINED_SUCCESSORS = Object.freeze([
  Object.freeze({path: `${prefix}observations.rs`, commit: '4a1da276664f236b081ca53cfb411245edae43de',
    sha256: '2884d3bb4e4cdb1c421acc9e96ac472c82be350734eed4846fd913fdd42519ec',
    scope: 'Move the first authenticated body payload instead of cloning it.'}),
  Object.freeze({path: `${prefix}durable_journal.rs`, commit: '592ea6741b45e5aebf1d223d7244ca2568a9ec8b',
    sha256: '69cd83ed6fc01177f235be13234aa1ccaf2b4f539668cf35831350b7b7fa2e42',
    scope: 'Bounded complete-byte authenticated chain cache and call-local borrowed artifact persistence.'}),
  Object.freeze({path: `${prefix}journal.rs`, commit: '592ea6741b45e5aebf1d223d7244ca2568a9ec8b',
    sha256: '26a03b7c35bf469e34ed7e5b82ee92a36a10625205d0616a09eacc070ab7a4bb',
    scope: 'Owned metadata projection and borrowed snapshot artifacts with exact invalid-byte normalization.'}),
]);
export function assertSuccessorCustody(revision, current, historical, historicalHash, fixedSuccessor) {
  assert.ok(RETAINED_SUCCESSORS.includes(revision), 'exact reviewed successor entry required');
  assert.equal(sha256(historical), historicalHash, `${revision.path}: original retained source`);
  assert.equal(sha256(fixedSuccessor), revision.sha256, `${revision.path}: fixed successor source`);
  assert.equal(sha256(current), revision.sha256, `${revision.path}: candidate successor source`);
  if (revision.path.endsWith('/observations.rs')) {
    // A complete-byte reverse proof, not a claim based on two matching snippets.
    const text = current.toString();
    const changes = [
      ['let Some(record) = body.into_iter().next() else {', 'let Some(record) = body.first() else {'],
      ['payload: record.payload,', 'payload: record.payload.clone(),'],
    ];
    let original = text;
    for (const [after, before] of changes) {
      assert.equal(original.split(after).length, 2, 'one exact owned-projection change');
      original = original.replace(after, before);
    }
    assert.equal(sha256(original), historicalHash, 'observation controls/receipt validation unchanged');
  }
  assertSuccessorSafety(revision.path, current.toString());
}
// Lexical guards complement byte custody, not Rust execution or proof of parity.
// Tests mutate these conditions independently of the full-source hash guard.
export function assertSuccessorSafety(path, text) {
  assert.ok(RETAINED_SUCCESSORS.some(row => row.path === path), 'exact reviewed successor path required');
  assert.deepEqual(inspectSource(text).sites, [], 'no owned-business unsafe');
  if (path.endsWith('/observations.rs')) {
    assert.match(text, /let body = reader\.find\(control, &body_selector, None, true\)\?;\s*check_control\(control\)\?;\s*let Some\(record\) = body\.into_iter\(\)\.next\(\)/);
    assert.match(text, /if !valid\s*\|\| record\.payload\.len\(\) != receipt\.report_bytes as usize\s*\|\| checksum\(&record\.payload\) != receipt\.report_checksum/);
    assert.match(text, /if !lycheedev_core::raw_json::is_valid\(&record\.payload\)/);
  } else if (path.endsWith('/durable_journal.rs')) {
    const appendStart = text.indexOf('    fn append_inner('), appendEnd = text.indexOf('    fn checkpoint_inner(');
    assert.ok(appendStart >= 0 && appendEnd > appendStart);
    const append = text.slice(appendStart, appendEnd);
    assert.match(text, /const CHAIN_CACHE_BYTES: usize = CHECKPOINT_BYTES;/);
    assert.match(text, /self\.bytes\.capacity\(\) <= CHAIN_CACHE_BYTES && self\.count <= MAX_EVENTS/);
    assert.match(text, /pub fn close\(&mut self\) -> ChannelResult<\(\)> \{\s*drop\(self\.take_cache\(\)\);\s*let result = self\.lease\.take\(\)\.map_or\(Ok\(\(\)\), Lease::close\)/);
    assert.match(text, /if bytes == prior\.bytes \{\s*check_control\(control\)\?;/);
    assert.match(text, /drop\(prior\);\s*\}\s*let segment = parse_segment\(bytes, control\)\?;/);
    assert.match(text, /for \(digest, bytes\) in &snapshot\.artifacts \{\s*self\.check\(control\)\?;\s*write_artifact\(&self\.paths\.artifact\(digest\)\?, bytes, control\)\?;\s*self\.cut\(FaultPoint::ArtifactDurable, control\)\?;/);
    assert.match(text, /if result\.is_err\(\) \{\s*self\.poisoned = true;\s*drop\(self\.take_cache\(\)\);/);
    assert.match(text, /let segment = parse_segment\(bytes, control\)\?;\s*if segment\.events\.last\(\)\.map\(\|e\| &e\.checksum\) != Some\(&self\.head\)/);
    assert.match(append, /append_phase!\(self, "owner_initial", self\.check\(control\)\)\?;\s*let mut chain = self\.append_chain\(control\)\?;/);
    assert.match(append, /if !metadata\.is_file\(\) \|\| metadata\.len\(\) != chain\.bytes\.len\(\) as u64/);
    assert.match(append, /for chunk in bytes\.chunks\(65536\) \{\s*check_control\(control\)\?;\s*file\.write_all\(chunk\)/);
    assert.match(append, /let sync = file\.sync_all\(\)\.map_err\(io_error\);/);
    assert.match(append, /let close = close_file\(file\);/);
    assert.match(append, /joined\(\[\s*write,\s*sync,\s*sync_fault,\s*close,\s*close_fault,\s*check_control\(control\),\s*\]\)\?;\s*self\.head = prepared\.checksum\.clone\(\);/);
    assert.match(text, /if !quiet && !\(allow_pending_known_zero && known_zero\) \{/);
    assert.match(text, /if segment\.events\.len\(\) < CHECKPOINT_EVENTS && segment\.bytes\.len\(\) < CHECKPOINT_BYTES/);

  } else if (path.endsWith('/journal.rs')) {
    assert.match(text, /pub const CHECKPOINT_BYTES: usize = 8 << 20;/);
    assert.match(text, /pub\(super\) fn snapshot_borrowed<'a>\(state: &'a State\) -> Result<BorrowedSnapshot<'a>>/);
    assert.match(text, /let mut copy = state\.snapshot_metadata\(\)\?;/);
    assert.match(text, /artifacts\.entry\(digest\)\.or_insert\(bytes\);/);
    assert.match(text, /put\("result", Cow::Borrowed\(&o\.result_bytes\.0\)\);/);
    assert.match(text, /rest = &rest\[valid \+ 1\.\.\];/);
    assert.match(text, /if hash\(&bytes\) != digest \{/);
  } else assert.fail(`unreviewed successor path: ${path}`);
}
