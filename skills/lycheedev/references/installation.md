# Installation tasks

Read this for a requested installation/update/removal, or managed addon deployment
needed to complete an already authorized live task. That live authorization covers
the selected client's necessary deployment and activation; it does not imply a
global npm CLI update, updates to other clients or changing the selected release.
First inspect the installed CLI's top-level and relevant
subcommand `--help`; use only the commands and flags it exposes. If `describe`
is available, use it to confirm the supported capability. Do not substitute a
legacy installer or claim a planned release command exists.

## Update the installed toolkit

For an authorized npm update to the `latest` channel, with `update` in the
installed command contract:

```text
lycheedev update --plan --format json
lycheedev update --format json
```

The plan checks the current release and discovers remembered targets, existing
standard Agent skill directories and installed addons in configured local targets
or running clients without
game input. It does not check the registry for a newer version. To include an
offline/custom client or scope an isolated update, supply explicit targets;
when any target selector is present, automatic discovery is disabled:

```text
lycheedev update --path <parent/lycheedev> --installation <client-directory> --home <workspace> --format json
```

Repeat `--path` or `--installation` for multiple targets. Custom targets are
remembered after preflight for subsequent unscoped updates. An absent explicit
target can be installed when its parent already exists; an existing unmanaged
or edited target is a conflict, never permission to delete it.

For a saved explicit selection, `--file <targets.json>` accepts a JSON array of
objects with `component` (`skill` or `addon`) and absolute `path`. It cannot be
combined with the path selectors. An empty array intentionally updates only the
npm CLI and bundled tools; it never discovers or deploys other targets.

The npm launcher fetches `lycheedev@latest` into isolation and pins the exact
resolved package. This command has no version/channel selector: do not use it to
refresh a selected `next`, exact-version or development candidate. Keep that
selection and use its matching installed release root for payload deployment;
if CLI replacement is requested, use the explicitly chosen package/version through
the supported distribution route. Do not move registry tags to make an updater
select a candidate. Keep the owner's existing registry-channel policy separate
from installation; an update task is not a release-promotion request.

The launcher verifies the pinned package,
checks all targets, replaces the CLI (including its bundled LuaLS), then applies
that release's addon and Skill through native delivery. No running native process
holds the old executable during npm replacement. Successful replacement removes
old managed files; interrupted transactions retain recovery data. Retry with the
same arguments to use the pinned package and original targets. Do not remove a
transaction or choose another release to bypass a pending conflict. Failure can
leave the CLI updated while some target files still need recovery; inspect the
returned target states and `context.updateRecovery`, not only the CLI version.

For an already installed native release, or intentional offline payload sync:

```text
lycheedev update --release <distribution-root> --path <parent/lycheedev> --installation <client-directory> --format json
```

This verifies and applies that CLI version's payload; it does not self-replace
the native executable or fetch npm. Older CLIs without `update` need a supported
CLI upgrade only when an upgrade is in scope; select the requested release before
using its command. Do not call an
unimplemented update command merely because the Skill documents it.

`result.complete` confirms files and receipts. `activation: reload_required`
does not mean the game loaded them; `new_agent_context_required` does not mean
an existing Agent re-read the Skill. Continue runtime activation only when the
task includes it, using the verified selected client below.

## Install, remove and activate

Installation selection accepts a game root or an explicit client directory. Reuse
the returned verified client path for subsequent writes. A unique valid match needs
no question; resolve multiple candidates before deployment. Conflicting metadata
must be reported, never overwritten by a folder name or user-supplied product.
Use `context.installationSelection` to inspect candidates/issues. Do not copy the
target resolver's product flag onto addon/update commands that do not expose it.

Before a write, inspect the target's ownership, product/build identity, and
file integrity. An unmanaged, modified, ambiguous, or conflicting target is a
reportable stop condition; do not overwrite it, delete it, forge ownership, or
fall back to manual copying. Keep any recovery directory outside the live
installation and use only the CLI's documented recovery mechanism.

An installation result proves filesystem work only. It does not prove that the
running client loaded the release, that the addon is enabled, or that live
input is available. Installation/update/removal must not be used to import old
task definitions, queue state, bindings, or SavedVariables into a new run.

The complete CLI release carries its own pinned LuaLS runtime for source
semantics. Verify the release inventory through the normal installer and
`doctor`; do not ask the user to install a global language server. Addon file
installation uses the lightweight TOC/XML/Lua checker and does not require Git
or LuaLS on the target machine.

Before updating a managed installation, finish current operations and disconnect
your own compatible native CON connections. The installer refuses unresolved
ownership; do not clear a foreign project's claim. Deploy through the managed
installer, then activate the selected process using the startup workflow. This
candidate uses Lychee Dev mailbox protocol v1 (`lycheedev.mailbox.v1`) and creates
no LoadOnDemand input addon. Its one whole-command row is reused for successive
commands; the previous 4 KiB row and older duplex schemas are rejected.
All managed files and receipts must match the selected CLI release. An installed
CLI or Skill does not update an already running client, and a disk-only result
is not live readiness. Targeted doctor also checks the exact-image writer
profile. Current direct-writer evidence qualifies only the tested Retail
`12.1.0.69933` image and its recorded scenarios; other images and clients
remain read-only until separately qualified.

An inert older managed installation can be replaced using its installation
receipt. The installer can archive byte-verified, idle managed old slot files;
pending, modified or unknown content blocks migration. This filesystem operation
does not make old wire or old execution journals compatible. Preserve unresolved
records for their matching older CLI in the owning task; do not rewrite their
schema or delete claims to permit installation. With multiple instances, activate
and verify each authorized PID after the shared installation is updated. The
candidate sends no bootstrap keys and has no slot-loading restart fallback.
If the authorized task includes using the running client, follow
[live-startup.md](live-startup.md) after deployment. Activation precedes a
bridge session when the addon is not running; do not require that session as
the prerequisite for its own first activation. Installation alone does not
authorize a game restart or a switch of character.
