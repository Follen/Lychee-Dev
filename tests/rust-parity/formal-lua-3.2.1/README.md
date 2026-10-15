# Fixed Go 3.2.1 Lua acceptance

Exact oracle: `10b561db5706702cd42e047eaeed4ccb5e825351`. All 57 fixture/source records are unchanged bytes from that commit, including the Retail 12.1.5 harness and the explicit 12.1.0 compatibility assertions in `t_client_gate.lua`. The 35 addon suites run under six mocked profiles, plus four standalone protocol suites: 214 interpreter executions. A passing Node checker is not a Lua execution. Three Go/Rust host bridge tests remain deferred exactly as identified in the test map.

The historical `../formal-lua/` directory, 3057 manifest and recorded 214-test results remain unchanged. They are not relabeled as current acceptance. Current runner entry remains `tools/rust-formal-lua.mjs`, now importing this versioned checker.

The 55 original addon records in this manifest are exact 3.2.1 bytes. Two explicitly frozen release-label differences retain the current Rust branch's 3.2.0 text in `addon/Core/Runtime.lua` and `addon/Lychee Dev.toc`, matching the already reviewed 3.2.1 migration contract. Authentication requires both exact original and retained hashes and verifies replacing that single literal reproduces the entire original blob. Materialization copies the actual candidate bytes unchanged. No other difference is accepted. The Go Lua mock returns upstream release metadata; these tests do not certify actual package/release metadata alignment. No release version is changed by this candidate.

The current report separately records retained release fields and historical extra addon files. Those extras remain a separate failing delivery gate; this suite neither executes nor silently removes them. The build/receipt, source inventory, limits, cancellation, output checks and failed-process handling are unchanged from the historical runner.

Status: Node checker validation executed locally; actual Go 3.2.1 Lua/Windows acceptance NOT_RUN for this candidate.
