# Installation tasks

Read this reference only when the user explicitly asks to install, update, or
remove Lychee Dev. First inspect the installed CLI's top-level and relevant
subcommand `--help`; use only the commands and flags it exposes. If `describe`
is available, use it to confirm the supported capability. Do not substitute a
legacy installer or claim a planned release command exists.

## Update the installed toolkit

For npm installations with `update` in their installed command contract:

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

The npm launcher fetches one exact latest package into isolation, verifies it,
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
the native executable or fetch npm. Older CLIs without `update` must first be
upgraded using npm, then use the newly installed command. Do not call an
unimplemented update command merely because the Skill documents it.

`result.complete` confirms files and receipts. `activation: reload_required`
does not mean the game loaded them; `new_agent_context_required` does not mean
an existing Agent re-read the Skill. Continue runtime activation only when the
task includes it, using the verified selected client below.

## Install, remove and activate

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
your own native CON connections. The installer refuses unresolved host ownership;
do not clear a foreign project's claim. Deploy through the managed installer,
then activate the selected process using the startup workflow. All 64 slot files
are part of the managed installation. An installed CLI or Skill does not update
an already running client, and a disk-only result is not live readiness.
If the authorized task includes using the running client, follow
[live-startup.md](live-startup.md) after deployment. Activation precedes a
bridge session when the addon is not running; do not require that session as
the prerequisite for its own first activation. Installation alone does not
authorize a game restart or a switch of character.
