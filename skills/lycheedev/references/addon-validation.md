# Addon validation

Read for TOC/XML/Lua closure and client compatibility. Use the actual addon bytes
and intended relative TOC: the source snapshot supplies a fixed reference, not
replacement local bytes. Keep working changes/addon revision separately. Do not
substitute a recursive scan for a release's ordered load closure.

Use installed help for single validation or a matrix. The matrix file shape is:

```json
{"path":"./addon","targets":[{"id":"classic","toc":"Example.toc","product":"classic","ref":"<40-character-source-commit>"}]}
```

Path is relative to the config; TOC is relative to that addon root. Each target
needs a unique ID, TOC, product and ref; optional source defaults to wow-ui-source.
Use real inputs and a fixed commit, or retain the commit resolved from an explicit
full ref. A current branch does not identify a historical version. Unknown fields
are rejected; describe/help does not provide this JSON schema.

Single validation's loadValid covers paths/order/syntax; staticValid adds the
returned checks. A source name match does not prove binding identity or argument
types. Missing names can be local/dynamic. Inspect complete, unresolved,
interfaceStatus, checks and notChecked even after exit 0. Unknown interface commits
remain unresolved; never infer them from folder/version labels.

Matrix targets have resolvedCommit, toc, loadClosure, valid, diagnostics,
unresolved and coverage rather than the single-result shape. Keep each client's
gaps; aggregate success cannot erase them. Static/semantic success does not prove
runtime loading, combat, taint, visual behavior or performance.

Semantic analysis is optional evidence. A missing engine/tool leaves a capability
gap; the same static validation can still be useful without semantic/environment/
release options. Third-party source needs its fixed client API environment.
Matrix has no per-target environment field and cannot combine with a global
environment: use client API targets or separate validations for distinct source/
environment pairs. Preserve semantic input/report captures. Single semantic
validation intentionally retains incomplete coverage; do not raise budgets to
force complete=true. Identify actual clients from verified metadata, not folders.
