# Whole-plan reconstruction ledger

Status: implementation in progress; no unified native, business or performance
pass. Original unpublished candidate was lost. This candidate has new source
identities; only surviving bytes with matching hashes are marked exact in the
recovery delivery. b42e5b676adc20b28ee435d1eb08cc6e552755db is the verified base.

- Home and disk: b42 contains decimal 5 GB Data policy, managed-spool lifecycle,
  retirement/recency, private Data metadata, Data evidence catalog/anchors,
  protected legacy inventory and durable staging. Preserve their source and
  check all reconstructed consumers use the same production APIs. Do not claim
  allocated filesystem blocks or external installation/output bytes are all
  represented by a logical file-length quota.
- Source: existing Postcard Facts and binary Index are preserved. Relevant
  modules include facts/binary.rs, index/persistence_wire.rs and index/facts_cache.rs.
  No new decoder, cache fallback or Emmy modification is introduced. Source
  CLI/harness discovery adapts to clap; its actual semantic/native paths remain.
- Live: existing go_mailbox_v1/connection/session.rs SessionOwner is preserved,
  including thread-affine handles, durable boundaries, exact error joins and
  protocol v1. No unrelated Live rewrite, installed daemon or uncertain replay.
- Data: reconstruct authenticated lazy source/owned IPC and ID caches, typed
  DataFusion plans, general compound/subquery/iteration capabilities, owned
  metadata/semantic discovery, complete CAP and twelve domain navigators.
  Core r1 through r9 are frozen and integrated, with independent source review
  closed against the consolidated 35-file hash set. This is reconstructed coverage-aware
  Arrow reuse, not the lost implementation identity. Public old query dispatch is
  retired; shared scalar/decoder traits remain. See business coverage matrix.
- CLI: new clap parser and pure local search/exact schema/bounded help derive
  from the command contract. 71 formal routes, eighteen retired old Data routes.
  Four pinned clap dependency edges come from official registry metadata, with
  no Cargo resolution/build run and no unrelated version drift intended.
- Skill: 25-file new source anchor is separate from unchanged Go and earlier
  Rust identities. Instructions use real typed/domain entry points and explicit
  result/CAP ownership, partial coverage, cancellation and resource boundaries.
- Current consumers: strict Agent numeric/frame client, owned paired/CSV/cache
  collectors, npm-installed Skill data workflow and Source discovery, supervised
  pipe/deadline consumers. Reconstructed source is independently reviewed before
  it may qualify as migration-complete. Fixed older trial/report tools remain
  historical and cannot be reused as new results.

Source reviews for the core ownership, paired collectors, installed Skill,
clap cutover and actual-binary CLI consumer declarations are closed. One exact
reconstructed candidate and source-only archive bind this implementation.
Unified migration-gate acceptance and publication/test authorization remain
separate from these source findings. No partial test dispatch or implicit CI trigger.
Final queue must enumerate migration-oracle feature targets as well as default
production targets; default-skipped tests are not passes. CLI oracle source
inheritance requires full Git history, including fixed Go 3.2.0/3.2.3 and earlier
Rust contract blobs; a shallow checkout cannot qualify by trusting fixture JSON.
Performance stays later.
