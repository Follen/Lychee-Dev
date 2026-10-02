# Lychee Dev Toolkit 3.1.1

On 2026-10-02 the owner authorized publishing the Retail Encoding budget fix.
3.1.0 is already published and immutable; this patch uses 3.1.1 under npm `next`,
preserving npm `latest=2.5.1` and the existing GitHub latest release.

The query now charges actual CKey/EKey directories and cumulative cache misses
before allocation rather than the whole logical Encoding extent. Default query
limits, checksums, BLTE/scratch/network accounting and Root retention remain.
See [the fixed Retail pin and verification](retail-encoding-budget-2026-10-02.md).
No live protocol or addon behavior changes are included; the synchronized
release identity advances with the CLI, runtime, package and fixtures.

Merge the reviewed patch after required Windows checks pass, then push the
immutable `v3.1.1` tag at the clean merged commit. Use the existing
`toolkit-release.yml` OIDC pipeline: identity, required CI, CGO verification,
single assembly, corresponding-source rebuild comparison, isolated install,
Windows run evidence, sealed digests, publication, registry read-back and
GitHub Release. No token fallback or local repacking of published bytes.

The source export retains the committed `docs/toolkit/research/` export-ignore
boundary. Raw investigation logs remain in Git history; product source,
tests, skills, licenses and acceptance summaries remain in corresponding source.

After publication, verify registry package identity and provenance, both npm
tags and the unchanged GitHub latest release. An isolated Windows install of
the actual registry package must also run the original fixed-pin offline query;
two returned rows are a successful bounded page, not a full-table scan or RSS
measurement. Publication does not automatically replace the owner's installed
CLI or managed game addons.
