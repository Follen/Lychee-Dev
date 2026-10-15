# Go 3.2.1 guidance increment, stage C

The only product-document change is the keyboard paragraph of
`skills/lycheedev/references/live-recovery.md`, copied byte-for-byte from the
[released 3.2.1 Skill](https://github.com/Follen/Lychee-Dev/blob/10b561db5706702cd42e047eaeed4ccb5e825351/skills/lycheedev/references/live-recovery.md).
All other bytes, including Rust/Source improvements, remain unchanged.

The paragraph states all-key release and 500ms event quiet, new fresh INPUT,
timestamp-only passive listener, completed/fully released active chords,
attempt-owned cancellation release, original receipt without replay and original
deadline. It does not describe typing capture or stopping an already-started
chord on new keyboard activity.

The corresponding [fixed protocol delta](https://github.com/Follen/Lychee-Dev/blob/10b561db5706702cd42e047eaeed4ccb5e825351/docs/live-protocol.md)
is documented in stage B's `release-321-keyboard-contract.md`, including no
Esc/Enter/chat/focus fallback and exact receipt authority. This candidate does not
add an entire `docs/live-protocol.md` to a tree that currently lacks that path.
Stage C is a documentation transplant, not a new protocol or release version.

Two Node document-custody tests pass. Windows implementation belongs to stage B;
its Rust compilation, hook and owned-window execution remain NOT_RUN. Do not use
this guidance patch as evidence that stage B passed its acceptance tests. Earlier
Go 3.2.0 test results remain tied to
`3057a9666ab078270b63c7f06de748069b0d1f4b`.
