# Mailbox v1 startup

Read this for first installation or an unavailable runtime. Installation, addon
enablement, character entry and a fresh connection are separate facts.

Use the installed CLI's command contract. This source branch uses Lychee Dev
mailbox protocol v1 (`lycheedev.mailbox.v1`); an installed 3.1.1 CLI may still
use an older transport. Match release bytes and identity, not the version label
alone. Old duplex schemas/layouts, LoD slots and bootstrap keys are incompatible.

1. Select the authorized executable path and PID. Read installation flavor and
   build metadata; a filename or folder name is not identity. Preserve process
   creation time and actor identity. Same-build instances need separate CONs.
2. Inspect the managed addon with `addon status`. Install a sealed, matching
   candidate with `addon install`; never overlay-copy files. This branch creates
   no LoD slots. Exact inactive managed old slot files may be archived by an
   upgrade; unknown or pending files are preserved and block that migration.
3. Enter the authorized character and enable the addon using `/dev connect`.
   If a replaced addon is not loaded yet, a manual `/reload` is needed after
   request cleanup and with no active writer; see [reload](live-reload.md). There is
   no automatic key fallback or old protocol compatibility on this branch.
4. Run targeted [doctor](live-doctor.md), then `live connect` with the exact target
   and project. Preserve any returned CON, even when the host wait is pending.

If no character has entered the world, keep the selected installation/PID and
report that character entry is required; do not attempt a business write or
silently switch to another window. After entry, repeat targeted doctor before
the drive. A world-ready reading during reload is insufficient while native
reload diagnostics still show reload in progress.

An absent actor, disabled bridge, unsupported Lua layout, missing publication,
foreign owner or modified installation is a concrete blocker. Do not select a
different process, clear claims, scan the heap or guess offsets to get past it.
Resume retained pending work before opening another connection. A cold enabled
runtime and an idle connection do not mean any probe was executed.
