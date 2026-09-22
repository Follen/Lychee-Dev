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
