local Env = ...
local frames = Env.framesCreated
local ns = Env.NewNamespace()
local oldReport = { receipt = "old evidence", body = "keep" }
local oldTicket = { requestId = "old work" }
LycheeToolkitDB = { schema = 1, options = { bridgeEnabled = true }, reports = { old = oldReport },
    reentry = oldTicket, runtimeEpoch = 92 }
LycheeToolkitBridgeDB = nil
Env.LoadAddon("Core/Persistence.lua", ns)
assert(ns.Persistence.Load())
local account = ns.Persistence.Current()
local first = ns.Persistence.Bridge()
assert(account == LycheeToolkitDB and first ~= account and first.schema == 1)
assert(next(first.reports) == nil and first.reentry == nil and first.runtimeEpoch == nil,
    "account execution state was imported into the character")
first.reports.new = { receipt = "new receipt", body = "new data" }
assert(account.reports.old == oldReport and account.reports.new == nil
        and account.reentry == oldTicket and account.runtimeEpoch == 92,
    "character writes changed account execution evidence")

-- A character switch must not share mutable reports or tickets.
LycheeToolkitBridgeDB = nil
assert(ns.Persistence.Load())
local second = ns.Persistence.Bridge()
assert(first ~= second and first.reports ~= second.reports and next(second.reports) == nil)
assert(first.reports.new.body == "new data")
local future = { schema = 2, keep = true }
LycheeToolkitBridgeDB = future
assert(ns.Persistence.Load() == nil and LycheeToolkitBridgeDB == future
        and account.reports.old == oldReport, "future character state was overwritten")
assert(ns.Persistence.Bridge() == nil, "failed load retained writable character state")
assert(Env.framesCreated == frames, "persistence allocated UI")
print("character storage: isolated reports and preserved account/future state")
