# Installed CLI contract

Search by task, then read the selected command's schema or help. The installed
catalog describes accepted inputs, effects and examples; Data Agent schema also
provides `requestSchema`, `responseSchema` and per-task `resultSchemas`. Follow
parser errors' command/schema pointers rather than guessing another command.
Recheck relevant metadata after a binary change.

JSON results use stdout and diagnostics stderr. Ordinary command JSONL requires
a valid end frame and exit 0. Data Agent uses complete matching-ID responses,
with terminal diagnostics described separately; it has no ordinary begin/end
wrapper. Process exit and result coverage are separate facts. Preserve exact
integer lexemes when decoding JSON.
