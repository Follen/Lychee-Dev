# Frozen flow expectations

`frozen-go.json` contains 36 actual outputs from unchanged v3.1.2 Go flow
code, commit `9d3d3598b8bac7efabf0b937df30508075eaa841`. The isolated driver
was `.tmp/flow-oracle/main.go`, SHA256
`1aa60baefcc3c6801d20fcc1b77c74c932cee5a502ecb480e374c0c9b4d7e3cc`.
It used `go run ./.tmp/flow-oracle`; it is never a product backend.
Fixture SHA256: `f9cdd74d0688906fcb0b75bcf85344256b894fc6c0d8082bdf28eddeac12436f`.

The first Rust run matched all result fields in 35 cases. The remaining
`goto-identifier` case intentionally preserves the old parser rejection.
Lua 5.1 permits that identifier, so the Rust test separately checks this
owner-approved lexical improvement and rejects jump labels and Lua 5.2+
operations. It does not normalize the failure into an equivalence claim.

These synthetic frozen closures cover propagation and trace semantics;
they are not whole-source selection, public CLI, performance or runtime
secrecy/secure-execution acceptance evidence.
