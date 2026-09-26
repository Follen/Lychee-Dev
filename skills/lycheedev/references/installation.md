# Installation tasks

Read this reference only when the user explicitly asks to install, update, or
remove Lychee Dev. First inspect the installed CLI's top-level and relevant
subcommand `--help`; use only the commands and flags it exposes. If `describe`
is available, use it to confirm the supported capability. Do not substitute a
legacy installer or claim a planned release command exists.

Before a write, inspect the target's ownership, product/build identity, and
file integrity. An unmanaged, modified, ambiguous, or conflicting target is a
reportable stop condition; do not overwrite it, delete it, forge ownership, or
fall back to manual copying. Keep any recovery directory outside the live
installation and use only the CLI's documented recovery mechanism.

An installation result proves filesystem work only. It does not prove that the
running client loaded the release, that the addon is enabled, or that live
input is available. Installation/update/removal must not be used to import old
task definitions, queue state, bindings, or SavedVariables into a new run.

For an authorized running-client upgrade with a saved session, install the
current release through the managed installer, then use `live reload --session
<session-id> --request <stable-key>`. This controlled upgrade path can connect
the archived older runtime to the exact current CLI's managed addon; it checks
the target files again before refresh input. A modified or mismatched install
must be resolved, not bypassed. The new runtime requires correlated ready
evidence; ordinary live commands do not accept an old runtime merely because
its new files exist on disk. Recover an interrupted reload by its operation ID.
The original session remains constrained to the same process, window and actor
when reconnecting after upgrade. Dismiss a standalone reload's final receipt
with `live hide`; first activation without a usable session follows the startup
reference below.

If the authorized task includes using the running client, follow
[live-startup.md](live-startup.md) after deployment. Activation precedes a
bridge session when the addon is not running; do not require that session as
the prerequisite for its own first activation. Installation alone does not
authorize a game restart or a switch of character.
