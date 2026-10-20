# Command discovery

Use the selected executable's `lycheedev describe --format json` and each
command's `--help` for available commands, arguments and flags. Recheck when the
binary changes; a copied command catalog is not installed capability evidence.

Read the result envelope, captures and coverage as well as exit status. An absent
command or unsupported flag is a specific capability/version gap, not permission
to invent a substitute. See [candidate boundaries](source-candidate.md).

## Data Agent worker

Use `data agent schema` for the versioned task and result-handle contract, and
`data agent serve --help` for the installed worker flags. The schema is the
source of typed operation fields and resource limits; do not duplicate it here.
This worker does not accept SQL; the separate legacy SQL command is not unified
with this session yet.
