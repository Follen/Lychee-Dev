# Independent forward-test request

Use `$lycheedev` at this worktree's `.tmp/rust-skill-forward-candidate` to answer this user request using the actual candidate CLI. This staged Skill is generated from the versioned candidate entry and the actual Rust command directory. Work only on static source evidence. Do not send game input, change game installations or call a Go backend.

> 请查这个固定 Retail 源码版本里 C_Timer.After 的参数约束，以及 secret 参数有哪些限制。请给出准确源码位置，并说明静态证据能证明什么。不要在游戏里执行。

Raw task inputs:

- Candidate CLI: `.worktrees/rust-refactor/target/debug/lycheedev.exe` under the repository's parent project directory, or `target/debug/lycheedev.exe` from this worktree.
- Workspace: this worktree's `.tmp/rust-source-independent/home`.
- Fixed snapshot: `PIN-f5ca7507c049580ad68831744678771cc49525719fdc901a91c739db2871b38b`.
- The candidate uses the versioned Skill in this worktree. Inspect its current command directory before choosing an unsupported profile.
- Store new task notes, actual tool outputs and the final response in a fresh directory under this worktree's `.tmp/skill-forward/`. Do not read prior expected outputs or comparison reports.

The evaluator must return the actual commands/outputs used, the source-supported answer and any capability gap encountered. This request intentionally supplies no expected answer, suspected bug or proposed fix.
