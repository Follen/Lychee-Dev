# Static compatibility audit

Lychee Dev uses a reproducible, evidence-first static audit for its three supported clients. The audit does not scan the repository as an undifferentiated folder and does not assume that a clean Retail result implies Classic compatibility.

## Method

1. Select the exact client TOC.
2. Resolve its ordered load list and recursively follow XML `file` references.
3. Stage only that load closure in an isolated temporary directory.
4. Run `wowdoc validate` against the exact official source Tag and immutable Commit.
5. Verify Interface metadata, client profile selection, compatibility-layer position, event catalog selection, and shared load order.
6. Compare literal `RegisterEvent` and `RegisterUnitEvent` calls with the exact generated event catalog for that build.
7. Report dynamic event names and product checks outside the compatibility boundary as manual-review evidence.

| Client | Product | Tag | Commit |
| --- | --- | --- | --- |
| Retail | `retail` | `12.1.0` | `31c7f7b9cc79e56c986b365c06a6afbcf3c9177b` |
| Classic | `classic` | `5.5.4` | `1028c1e687f721ba9d3af14d1b12a5745e4227c7` |
| Titan | `titan` | `3.80.2` | `825d29d3662b372f0bead725ee6abd339e4a77b5` |

## Run

```powershell
powershell -ExecutionPolicy Bypass -File tools/AuditCompatibility.ps1
```

Write a machine-readable report when an Agent or CI job needs durable evidence:

```powershell
powershell -ExecutionPolicy Bypass -File tools/AuditCompatibility.ps1 `
  -OutputPath artifacts/compatibility-audit.json
```

The report schema is `lychee.compatibility-audit.v1`. Errors fail the command. Warnings identify compatibility boundaries that require review but are not automatically treated as proof of failure.

Audit another addon that provides the same three client TOC suffixes:

```powershell
powershell -ExecutionPolicy Bypass -File tools/AuditCompatibility.ps1 `
  -AddonPath "D:\Games\World of Warcraft\_retail_\Interface\AddOns\TargetAddon"
```

Generic addon mode applies official API and event validation to `*_Mainline.toc`, `*_Mists.toc`, and `*_Wrath.toc`. Lychee Dev mode additionally enforces this repository's client-profile, compatibility-layer, generated-catalog, and shared-order invariants.

## Limits

Static analysis cannot prove dynamically constructed API names or events, runtime-only branches, combat behavior, taint safety, persistence behavior, or visual interaction. Those remain separate in-game verification requirements.
