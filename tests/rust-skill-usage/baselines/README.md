# Fixed Skill source custody

These are test inputs, not execution results or a new migration status ledger.

`go3.2.0-skill.json` records all 23 files from fixed commit
`3057a9666ab078270b63c7f06de748069b0d1f4b`, Skill tree
`7145d71201e7b1b63a3fbcd880ec78d6a1589e26`.
`go3.2.3-skill.json` records all 23 files from fixed commit
`3766f9cacb52e93a803a626187e2a1d54a445468`, Skill tree
`b9e162ace7abe7ea4a2feecef822283767eb1210`.
They were obtained directly from the local exact Git objects with
`git ls-tree -r <commit> -- skills/lycheedev` and `git cat-file blob <blob>`.
Each row retains the original mode, path, Git blob, byte length and SHA256.
The test reconstructs the entire Git tree with Git's file/directory ordering.
No Go build, mutable branch, installed CLI or release binary is used as evidence.

There are exactly four changed Skill files between those commits, no additions
or removals. Their complete Go323 blobs are retained under `go3.2.3/references/`:

- `live-recovery.md`, introduced by Go321 `10b561db5706702cd42e047eaeed4ccb5e825351`
- `live-startup.md`, introduced by Go322 `0bec60e4406f5d5dc155ee25cef44f9a710e37e8`
- `data-tables.md` and `data-recovery.md`, introduced by Go323

The existing historical `../workflow.json` is unchanged, with SHA256
`ebf400880b52b7929e5614281b1ce90f77e5764bea09a8d9671daedeef6a2d9b`.
`../workflow-go323.json` keeps its three Source tasks and authenticated source
windows unchanged, adds the four cumulative representative tasks, and records
the exact missing inputs preventing their runtime execution here. The ordinary
tool and owned controller both use that new workflow. Old reports keep their
original Go320 identity and actual outcomes.

SHA256 plus original Git-blob checks reject even a self-consistent candidate
package whose current Data or Live guidance has regressed. Those checks prove
packaged reference identity only. They do not prove Windows adaptive allocation,
IN lookup behavior, actual pressure recovery, slot activation or keyboard safety.
The real Data and Live tasks remain explicitly unrun until their required
authenticated fixtures or authorized runtime evidence exist.
