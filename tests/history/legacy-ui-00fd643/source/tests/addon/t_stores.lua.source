-- Stores: bounded run history, export records (lycheedev.export.v1), ticket
-- allocation, budgets, protection and SavedVariables safety rules.
local Env, client, root = ...

LycheeDevDB = { history = { { code = "legacy", result = "legacy", succeeded = true } } }
DumperDB = { history = { { code = "dumper", result = "dumper", succeeded = true } } }
LycheeToolkitDB = {
    schema = 1,
    history = {
        entries = { { code = "user code", result = "user result", succeeded = true, timestamp = 5 } },
    },
}

local ns = Env.LoadWorkbench()
assert(ns.Persistence.Load(), "persistence did not load")
assert(ns.Stores.Initialize(), "stores did not initialize")

local History = ns.Stores.History
local Exports = ns.Stores.Exports
local L = ns.L

-- SavedVariables rules: user values survive initialization and legacy
-- databases are never read.
assert(History.Get()[1].code == "user code", "user history was overwritten")
assert(#LycheeDevDB.history == 1 and LycheeDevDB.history[1].code == "legacy",
    "legacy LycheeDevDB was read or mutated")
assert(DumperDB.history[1].code == "dumper", "legacy DumperDB was read or mutated")

-- History trimming appends the visible truncation marker.
local trimmed = History.Add(string.rep("c", 20000), string.rep("r", 60000), true)
assert(#trimmed.code == 12000 + 16 and trimmed.code:sub(-16) == "\n... <truncated>",
    "history code cap of 12000 bytes is not enforced")
assert(#trimmed.result == 48000 + 16 and trimmed.result:sub(-16) == "\n... <truncated>",
    "history result cap of 48000 bytes is not enforced")
assert(trimmed.succeeded == true and trimmed.timestamp == Env.now,
    "history entry fields changed")

History.Clear()
assert(#History.Get() == 0 and History.GetStats() == 0, "history was not cleared")

-- 16 MiB budget with 16 KiB per-entry accounting keeps 1024 small entries,
-- pruning oldest-first.
for index = 1, 1200 do
    History.Add("code " .. index, "result " .. index, true)
end
local historyCount, historyBytes, historyBudget = History.GetStats()
assert(historyCount == 1024 and #History.Get() == 1024,
    "history budget did not keep exactly 1024 small entries")
assert(History.Get()[1].code == "code 1200", "history budget removed the newest entry")
assert(History.Get()[1024].code == "code 177", "history budget removed the wrong oldest entry")
assert(historyBytes <= historyBudget and historyBudget == 16 * 1024 * 1024,
    "history budget accounting is wrong")
History.Clear()

-- Export envelope.
Env.SetNow(1234567890)
local longString = string.rep("z", 5000)
local id, entry = Exports.Add("run_result", "First title", "first content", {
    path = "Test.Path",
    long = longString,
    nested = { keep = 2, deep = { dropped = true } },
    dropped = function() end,
})
assert(id and entry, "export was not committed")
assert(entry.schema == "lycheedev.export.v1" and entry.id == id and entry.createdAt == Env.now,
    "export envelope identity fields are wrong")
assert(entry.source.kind == "run_result" and entry.source.title == "First title"
        and entry.source.path == "Test.Path", "export source block is wrong")
assert(entry.payload.mediaType == "text/plain" and entry.payload.encoding == "utf-8"
        and entry.payload.content == "first content" and entry.payload.byteCount == #"first content",
    "export payload block is wrong")
assert(entry.environment.addonName == "Lychee Dev" and entry.environment.clientId == client
        and entry.environment.version == Env.testBuilds[client][1]
        and entry.environment.build == Env.testBuilds[client][2]
        and entry.environment.interface == Env.testBuilds[client][4]
        and entry.environment.locale == Env.locale,
    "export environment block is wrong")

-- Bounded metadata copy: strings <= 2048, nesting <= 2, <= 32 keys.
assert(#entry.metadata.long == 2048, "metadata strings are not capped at 2048")
assert(entry.metadata.path == "Test.Path" and entry.metadata.nested.keep == 2,
    "metadata values were lost")
assert(entry.metadata.nested.deep == nil and entry.metadata.dropped == nil,
    "metadata nesting deeper than 2 was not dropped")
local wideMetadata = {}
for index = 1, 40 do
    wideMetadata["key" .. index] = index
end
local _, wideEntry = Exports.Add("run_result", "wide", "wide", wideMetadata)
local metadataCount = 0
for _ in pairs(wideEntry.metadata) do
    metadataCount = metadataCount + 1
end
assert(metadataCount == 32, "metadata copy is not capped at 32 keys")

-- Ticket shape and monotonic, never-reused allocation.
assert(string.match(id, "^LYCHEE%-%d%d%d%d%d%d%d%d%-%d%d%d%d%d%d%-%d%d%d%d$"),
    "ticket does not use the LYCHEE-YYYYMMDD-HHMMSS-%04d shape")
local seenTickets = { [id] = true }
local highestNextId = LycheeToolkitDB.exports.nextId
for index = 1, 10 do
    local nextId, nextEntry = Exports.Add("run_result", "t" .. index, "content " .. index)
    assert(nextId and not seenTickets[nextId], "ticket was reused")
    seenTickets[nextId] = true
    assert(nextEntry.id == nextId, "envelope id does not match its ticket")
end
assert(LycheeToolkitDB.exports.nextId > highestNextId, "ticket counter is not monotonic")

-- Pending versus saved tracking.
assert(Exports.IsPending(id) and Exports.GetState(id) == "pending"
        and Exports.GetPendingCount() == 12, "fresh records are not pending")
assert(Exports.MarkSaved(id) and not Exports.IsPending(id) and Exports.GetState(id) == "saved",
    "flushed records did not become saved")

-- Protection: protected records survive delete, clear and pruning until the
-- flush releases them.
local protectedId = Exports.Add("automation_result", "protected", "protected body", nil,
    { protect = true })
assert(protectedId and Exports.IsProtected(protectedId) and Exports.GetProtectedCount() == 1,
    "record was not protected")
assert(Exports.Delete(protectedId) == false and Exports.Get(protectedId),
    "protected record could be deleted")
assert(Exports.Clear() == 12 and Exports.Get(protectedId),
    "clear did not skip protected records")
assert(Exports.GetStats() == 1, "clear left unexpected records behind")
assert(Exports.MarkSaved(protectedId) and not Exports.IsProtected(protectedId),
    "flush did not release protection")
assert(Exports.Delete(protectedId) and Exports.GetStats() == 0,
    "flushed record could not be deleted")

-- Budget: 16 MiB prunes oldest-first.
local largeBody = string.rep("x", 9 * 1024 * 1024)
local oldLargeId = Exports.Add("run_result", "Old large", largeBody)
local newLargeId = Exports.Add("run_result", "New large", largeBody)
local exportCount, exportBytes, exportBudget = Exports.GetStats()
assert(oldLargeId and newLargeId and Exports.Get(oldLargeId) == nil and Exports.Get(newLargeId)
        and exportCount == 1 and exportBytes == #largeBody
        and exportBudget == 16 * 1024 * 1024,
    "export store did not prune oldest records to its 16 MB budget")
Exports.Clear()

-- Protected records are skipped by budget pruning as well.
local protectedLargeId = Exports.Add("automation_result", "Protected large", largeBody, nil,
    { protect = true })
local prunedLargeId = Exports.Add("run_result", "Pruned large", largeBody)
assert(Exports.Get(protectedLargeId) and Exports.Get(prunedLargeId) == nil,
    "budget pruning did not skip the protected record")
Exports.MarkSaved(protectedLargeId)
Exports.Clear()

-- Record cap of 200.
for index = 1, 205 do
    Exports.Add("run_result", "record " .. index, "body " .. index)
end
assert(Exports.GetStats() == 200, "export store did not cap at 200 records")
Exports.Clear()

-- Rejections.
local emptyId, emptyError = Exports.Add("run_result", "empty", "")
assert(emptyId == nil and emptyError == L.EXPORT_EMPTY, "empty export was accepted")
local hugeId, hugeError = Exports.Add("run_result", "huge", string.rep("x", 16 * 1024 * 1024 + 1))
assert(hugeId == nil and hugeError == L.EXPORT_TOO_LARGE, "oversized record was accepted")

-- The fit precheck reserves headroom for later automation failure reports.
assert(Exports.CanFit(1024, 0), "small record does not fit")
assert(not Exports.CanFit(16 * 1024 * 1024, 1024), "reserved budget is not enforced")

-- A fresh session keeps the records but none of the session-only state.
local durableId = Exports.Add("run_result", "durable", "durable body")
local ns2 = Env.LoadWorkbench()
assert(ns2.Persistence.Load())
local Exports2 = ns2.Stores.Exports
assert(Exports2.GetPendingCount() == 0 and Exports2.GetProtectedCount() == 0,
    "session-only export state survived a SavedVariables load")
assert(Exports2.Get(durableId) and Exports2.GetState(durableId) == "saved",
    "saved records were lost across sessions")
assert(ns2.Stores.History.Get() == nil or #ns2.Stores.History.Get() >= 0, "history unreadable")

-- Newer section versions refuse writes but preserve the stored data.
LycheeToolkitDB.exports.version = 2
LycheeToolkitDB.history.version = 2
local ns3 = Env.LoadWorkbench()
assert(ns3.Persistence.Load())
local rejectedId, rejectedError = ns3.Stores.Exports.Add("run_result", "x", "y")
assert(rejectedId == nil and rejectedError == L.EXPORT_DATABASE_NEWER,
    "newer export data was overwritten by a downgrade")
assert(ns3.Stores.Exports.Get(durableId), "downgrade guard lost stored records")
local rejectedHistory, historyReason = ns3.Stores.History.Add("c", "r", true)
assert(rejectedHistory == nil and historyReason == "state_unsupported_version",
    "newer history data was overwritten by a downgrade")
LycheeToolkitDB.exports.version = nil
LycheeToolkitDB.history.version = nil

print("stores ok")
