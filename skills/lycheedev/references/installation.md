# Installation tasks

Read for requested toolkit changes or managed deployment needed by an authorized
live task. Live scope can cover the selected client's necessary deployment/
activation; it does not imply global npm update, another client or another release.
Use installed help and supported distribution tools; do not invent release flags
or use an unsupported installer.

## Preserve release and targets

Update plan inspects current targets without game input; it does not discover a
new registry version. The npm update route selects latest with no version/channel
selector. Do not use it to refresh a selected next/exact/development candidate;
retain that selection and its matching release root. Installation is not registry
promotion. Native update with a release root applies that payload offline and
does not replace the native executable or fetch npm.

Explicit target selectors disable automatic discovery. Reuse verified paths;
resolve only remaining ambiguous clients. A targets file is a JSON array of
objects with component (skill/addon) and absolute path. An empty array intentionally
selects CLI/tools only. Missing explicit targets may be created when supported;
removed remembered targets are not an implicit reinstall request.

Inspect managed ownership, identity and integrity. Modified/unmanaged/conflicting
targets stop replacement; never delete them, forge receipts or overlay-copy files.
Preserve pending update transactions and retry the same release/targets through
supported recovery. Failure can leave CLI replacement complete but payload work
pending; read target states/updateRecovery, not just the version number.

## Distinguish files from activation

Filesystem completion is not loaded-runtime readiness or a refreshed agent
context. Necessary live activation follows [startup](live-startup.md). A disk-only
installation does not authorize restart or a character switch. Each authorized
PID sharing files needs its own runtime activation/verification.

Finish/disconnect owned compatible CON work before managed replacement; unresolved
or foreign ownership remains a blocker. Preserve journals and reservations from
another wire/identity version for matching-version diagnosis. Inert older managed
files can migrate via receipts; that does not make those journals driveable by
the current driver. Never rewrite their schema or clear their claims to permit
installation.

Lychee Dev ships its Source analysis tools; no separate analysis server must be
installed. Managed addon file checking needs no Git or analysis server on the
target machine. Do not import unrelated task/queue/binding/SavedVariables state
into a new run. Installed files, slot inventory and loaded readiness remain
separate evidence.
