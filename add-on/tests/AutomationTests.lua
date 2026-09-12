local inCombat = false
SlashCmdList = {}

function InCombatLockdown()
    return inCombat
end

local frameStubs = 0
function CreateFrame()
    frameStubs = frameStubs + 1
    return {
        RegisterEvent = function() end,
        SetScript = function() end,
        Hide = function() end,
        Show = function() end,
    }
end

function wipe(target)
    for key in pairs(target) do
        target[key] = nil
    end
end

function time()
    return 1234567890
end

function date(_, value)
    return tostring(value)
end

function GetBuildInfo()
    return "12.1.0", "70000", "Aug 19 2026", 120100
end

function GetLocale()
    return "enUS"
end

function UnitName()
    return "TestChar"
end

function GetRealmName()
    return "TestRealm"
end

local secretTarget = nil
function issecretvalue(value)
    return secretTarget ~= nil and value == secretTarget
end

local function LoadAddonFile(path, namespace)
    local chunk, loadError = loadfile(path)
    assert(chunk, loadError)
    return chunk("Lychee Dev", namespace)
end

local ns = {}
local clientFiles = {
    retail = "Core/Clients/Mainline.lua",
    classic = "Core/Clients/Mists.lua",
    titan = "Core/Clients/Titan.lua",
}
local testClient = os.getenv("LYCHEE_TEST_CLIENT") or "retail"
LoadAddonFile(assert(clientFiles[testClient], "unknown test client: " .. testClient), ns)
LoadAddonFile("Core/Compatibility.lua", ns)
LoadAddonFile("Core/Locale.lua", ns)
LoadAddonFile("Core/Locale_enUS.lua", ns)
LoadAddonFile("Core/Database.lua", ns)
LoadAddonFile("Core/Serializer.lua", ns)
LoadAddonFile("Core/Safety.lua", ns)
LoadAddonFile("Modules/Diagnostics.lua", ns)
LoadAddonFile("Modules/Automation/auto/auto.lua", ns)
LoadAddonFile("Modules/Automation/Report.lua", ns)
LoadAddonFile("Libs/AutomationQR.lua", ns)
LoadAddonFile("Modules/Automation/Controller.lua", ns)
LoadAddonFile("Core/Bootstrap.lua", ns)

local originalPrint = print
local messages = {}
print = function(message)
    messages[#messages + 1] = tostring(message)
end

local overlayState = {
    running = 0,
    errors = 0,
    hides = 0,
    notices = {},
}
ns.AutomationOverlay = {
    ShowRunning = function() overlayState.running = overlayState.running + 1 end,
    ShowError = function() overlayState.errors = overlayState.errors + 1 end,
    HideStatus = function() overlayState.hides = overlayState.hides + 1 end,
    ShowNotice = function(json)
        overlayState.notices[#overlayState.notices + 1] = json
        return true
    end,
    HideNotice = function() overlayState.hides = overlayState.hides + 1 end,
}

local Controller = ns.Automation

-- One automation session allows a single pending result; reset the module and
-- database state the same way an output-side reload would.
local function NewAutomationSession()
    LoadAddonFile("Core/Database.lua", ns)
    ns.InitializeDatabase()
    LoadAddonFile("Modules/Automation/Controller.lua", ns)
    Controller = ns.Automation
end

local function ReloadDatabase()
    LoadAddonFile("Core/Database.lua", ns)
    ns.InitializeDatabase()
end

local function DefineTask(id, requestId, source, extra)
    ns.AutomationTaskDefinitions[id] = {
        schema = 1,
        requestId = requestId,
        revision = "rev-" .. id,
        kind = "lua",
        createdAt = 1234567880,
        expiresAt = 0,
        sourceBytes = #source,
        sourceChecksum = ns.AutomationReport.Adler32(source),
        source = source,
    }
    if extra then
        for key, value in pairs(extra) do
            ns.AutomationTaskDefinitions[id][key] = value
        end
    end
end

local function LastMessage()
    return messages[#messages] or ""
end

local function ResetOverlayState()
    overlayState.running = 0
    overlayState.errors = 0
    overlayState.notices = {}
end

-- --- registry file is data only ----------------------------------------------

assert(type(ns.AutomationTaskDefinitions) == "table"
        and next(ns.AutomationTaskDefinitions) == nil,
    "release task registry is not an empty table")
assert(frameStubs == 0, "loading the addon files created frames")

-- --- checksum, JSON encoder and QR encoding ----------------------------------

assert(ns.AutomationReport.Adler32("123456789") == "091e01de",
    "adler32 checksum diverged from the reference vector")

local encoded = ns.AutomationReport.Encode
assert(encoded({ "text" }):sub(1, 1) == "[", "array values did not stay arrays")
local objectJson = encoded({ nested = { flag = true, count = 3 } })
assert(objectJson:find('"flag":true', 1, true) and objectJson:find('"count":3', 1, true),
    "object encoding lost its entries")

local escapeJson = encoded("line\nquote\"back\\tab\tcontrol\1")
assert(escapeJson:find("\\n", 1, true) and escapeJson:find('\\"', 1, true)
        and escapeJson:find("\\\\", 1, true) and escapeJson:find("\\t", 1, true)
        and escapeJson:find("\\u0001", 1, true),
    "json string escaping was incomplete")
assert(encoded("héllo") == '"héllo"', "valid utf-8 was not passed through")

-- UTF-8 validation must inspect every continuation byte, not only the second.
local function AssertBinaryString(text, label)
    local json, complete = encoded(text)
    assert(json and json:find("$base64", 1, true) and not complete,
        label .. " was accepted as valid UTF-8")
end
AssertBinaryString("\224\160A", "a 3-byte sequence with an ASCII third byte")
AssertBinaryString("\240\144AA", "a 4-byte sequence with ASCII tail bytes")
AssertBinaryString("\226\130", "a truncated 3-byte sequence")
AssertBinaryString("\237\160\128", "a UTF-8 surrogate")
AssertBinaryString("\192\175", "an overlong 2-byte sequence")
assert(encoded("\240\159\152\128") == '"\240\159\152\128"',
    "a valid 4-byte utf-8 sequence was rejected")
assert(encoded("\226\130\172") == '"\226\130\172"',
    "a valid 3-byte utf-8 sequence was rejected")
assert(encoded("\255\254"):find("$base64", 1, true), "binary bytes were not base64 marked")
assert(encoded(0 / 0) == "null", "non-finite numbers were not normalized")
assert(encoded({}) == "{}", "empty tables did not encode as empty objects")

local cyclic = { name = "root" }
cyclic.self = cyclic
local cycleJson, cycleComplete, cycleReasons = encoded(cyclic)
assert(cycleJson:find("<cycle>", 1, true) and not cycleComplete
        and cycleReasons[1] == "cycle",
    "cyclic references were not marked incomplete")

secretTarget = { hidden = true }
local secretJson, secretComplete = encoded({ data = secretTarget })
assert(secretJson:find("<secret>", 1, true) and not secretComplete,
    "secret values were inspected")
secretTarget = nil

local mixed = { "first", key = "value" }
assert(encoded(mixed):find('"key"', 1, true) and encoded(mixed):find('"first"', 1, true),
    "mixed tables lost their entries")

local qrNotice = '{"v":1,"ticket":"LYCHEE-20260912-180000-0001","task":"t1","run":"r1","ts":1234567890}'
local qrMatrix = assert(ns.AutomationQR.Encode(qrNotice, 2))
assert(#qrMatrix >= 21 and qrMatrix[1][1] > 0, "qr encoding produced an unexpected matrix")
assert(ns.AutomationQR.Encode("") == nil, "qr encoding accepted an empty payload")

-- --- automation database lifecycle -------------------------------------------

LycheeDevDB = {
    schemaVersion = 7,
    history = {},
    exports = {
        version = 2,
        nextId = 4,
        records = {},
        order = {},
    },
}
ns.InitializeDatabase()
assert(ns.db.schemaVersion == 8, "database did not upgrade to schema 8")
assert(ns.db.automation and ns.db.automation.version == 1
        and ns.db.automation.nextLocalRequestId == 1,
    "automation index was not initialized")

local record = assert(ns.BeginAutomationExecution("req-sync-1", {
    taskId = "t1", revision = "rev-1", kind = "lua",
}), "automation execution did not begin")
assert(record.status == "running" and record.resultAvailable == false,
    "new automation record was not active and unresulted")
assert(not ns.BeginAutomationExecution("req-sync-1", {}),
    "duplicate execution id was accepted")

local reportContent = ns.AutomationReportBuild({
    taskId = "t1",
    executionId = "req-sync-1",
    revision = "rev-1",
    kind = "lua",
    status = "succeeded",
    startedAt = 1234567890,
    finishedAt = 1234567890,
    requestType = "task",
    stdout = "hello",
    returns = { done = true },
    environment = { clientId = testClient, character = "TestChar", realm = "TestRealm" },
    complete = true,
    lifecycleLog = {},
})
assert(reportContent and reportContent:find('"schema":"lychee.automation.result.v1"', 1, true),
    "report body did not carry its schema version")

local ticket, entry = ns.CommitAutomationResult("req-sync-1", reportContent, {
    status = "succeeded",
    complete = true,
    resultSchema = "lychee.automation.result.v1",
    checksumAlgorithm = "adler32",
    contentChecksum = ns.AutomationReport.Adler32(reportContent),
    title = "t1",
    evidenceKind = "automation_result",
})
assert(ticket and entry, "automation result commit failed")
assert(entry and entry.source.kind == "automation_result"
        and entry.payload.mediaType == "application/json"
        and entry.metadata.taskId == "t1" and entry.metadata.executionId == "req-sync-1"
        and entry.metadata.status == "succeeded" and entry.metadata.complete == true
        and entry.metadata.contentChecksum == ns.AutomationReport.Adler32(reportContent),
    "automation evidence record lost its metadata")
assert(ns.GetAutomationRecord("req-sync-1").ticket == ticket
        and ns.GetAutomationRecord("req-sync-1").resultAvailable == true
        and ns.IsExportProtected(ticket) and ns.GetProtectedExportCount() == 1
        and ns.IsExportPending(ticket),
    "committed result was not protected against disk-write loss")
assert(ns.CommitAutomationResult("req-sync-1", reportContent, { status = "succeeded" }) == nil,
    "a terminal execution accepted a second commit")

assert(ns.CommitAutomationResult("req-missing", "{}", { status = "succeeded" }) == nil,
    "commit accepted an unknown execution")
assert(ns.CommitAutomationResult("req-empty", "", { status = "succeeded" }) == nil,
    "commit accepted empty content")

assert(ns.BeginAutomationExecution("req-oversize", { taskId = "t2" }))
assert(ns.CommitAutomationResult("req-oversize", string.rep("x", 1024 * 1024 + 1),
        { status = "succeeded" }) == nil,
    "commit accepted an oversize report")

-- Protected records survive pruning, deletion and cache clears.
for index = 1, 30 do
    ns.AddExport("test", "Bulk " .. index, string.rep("y", 900 * 1024))
end
assert(ns.GetExport(ticket), "protected automation result was pruned")
assert(not ns.DeleteExport(ticket), "protected result was deletable")
local cleared = ns.ClearExports()
assert(ns.GetExport(ticket) and ns.GetProtectedExportCount() == 1,
    "cache clear removed a protected pending result")
assert(cleared > 0, "cache clear did not remove the unprotected records")

-- Reload clears session protection; the loaded record reports saved data.
ReloadDatabase()
assert(ns.GetProtectedExportCount() == 0, "reload protection survived a database reload")
local reloadedRecord = ns.GetAutomationRecord("req-sync-1")
assert(reloadedRecord and reloadedRecord.status == "succeeded"
        and reloadedRecord.resultAvailable == true,
    "committed automation record did not survive a reload")

-- Losing the evidence must mark the index result unavailable.
assert(ns.BeginAutomationExecution("req-lost", { taskId = "t3" }))
ns.CommitAutomationResult("req-lost", '{"schema":"lychee.automation.result.v1"}', {
    status = "succeeded", complete = true, resultSchema = "lychee.automation.result.v1",
    checksumAlgorithm = "adler32", contentChecksum = "00000000",
})
assert(ns.IsExportProtected(ns.GetAutomationRecord("req-lost").ticket),
    "a fresh automation result was not protected")
ReloadDatabase()
local lostTicket = ns.GetAutomationRecord("req-lost").ticket
assert(ns.DeleteExport(lostTicket), "unprotected evidence could not be deleted")
assert(ns.GetAutomationRecord("req-lost").resultAvailable == false,
    "index did not mark its result unavailable after deletion")
assert(ns.FailAutomationExecution("req-lost", "failed", "late") == false,
    "a terminal execution accepted FailAutomationExecution")

assert(ns.BeginAutomationExecution("req-interrupted", { taskId = "t4" }))
ns.db.automation.records["req-interrupted"].status = "running"
ReloadDatabase()
assert(ns.GetAutomationRecord("req-interrupted").status == "interrupted",
    "a non-terminal loaded record was not marked interrupted")

assert(ns.ClearAutomationHistory() >= 3, "automation history clear removed nothing")
assert(ns.GetAutomationRecord("req-sync-1") == nil,
    "cleared history kept terminal records")

-- --- diagnostics error snapshot ----------------------------------------------

local providerDatabase = {
    { message = "oldest error", stack = "stack:0", locals = "", time = 50, session = 3, counter = 9 },
    { message = "first error", stack = "stack:1", locals = "a = 1",
        time = 100, session = 7, counter = 1 },
    { message = "second error", stack = "stack:2", time = 200, session = 7, counter = 1 },
}
BugGrabber = {
    GetDB = function() return providerDatabase end,
    GetSessionId = function() return 7 end,
    version = "9.2.3",
}

local snapshot, snapshotError = ns.Diagnostics.SnapshotRecentErrors(2, "all")
assert(snapshot and not snapshotError, "snapshot failed unexpectedly")
assert(snapshot.ordering == "provider_storage_reverse"
        and snapshot.requestedCount == 2 and snapshot.returnedCount == 2
        and snapshot.availableCount == 3 and snapshot.session == 7
        and snapshot.providerVersion == "9.2.3"
        and snapshot.errors[1].message == "second error"
        and snapshot.errors[2].message == "first error",
    "error snapshot did not use provider storage in reverse order")

providerDatabase[1].message = "mutated after snapshot"
assert(snapshot.errors[2].message == "first error",
    "snapshot did not copy error fields immediately")

assert(ns.Diagnostics.SnapshotRecentErrors(0) == nil, "zero count was accepted")
assert(ns.Diagnostics.SnapshotRecentErrors(1.5) == nil, "fractional count was accepted")
assert(ns.Diagnostics.SnapshotRecentErrors(101) == nil, "oversized count was accepted")

BugGrabber = nil
assert(select(2, ns.Diagnostics.SnapshotRecentErrors(1)) == "provider_unavailable",
    "missing provider did not fail explicitly")

secretTarget = "sensitive"
BugGrabber = {
    GetDB = function()
        return { { message = "secret locals error", locals = secretTarget,
            time = 1, session = 7, counter = 1 } }
    end,
    GetSessionId = function() return 7 end,
}
local secretSnapshot = assert(ns.Diagnostics.SnapshotRecentErrors(1, "all"))
local secretLocalsMissing = false
for index = 1, #secretSnapshot.errors[1].missingFields do
    if secretSnapshot.errors[1].missingFields[index] == "locals" then
        secretLocalsMissing = true
    end
end
assert(not secretSnapshot.complete and secretLocalsMissing
        and secretSnapshot.errors[1].locals == nil,
    "secret snapshot fields were not marked missing")
secretTarget = nil
BugGrabber = nil

-- --- controller: task execution ----------------------------------------------

-- invalid task ids and unloaded blocks fail without side effects
Controller.Run("bad id!")
assert(LastMessage():find("task_id", 1, true), "invalid task id was not rejected")
Controller.Run("missing-task")
assert(LastMessage():find("missing-task", 1, true),
    "unloaded task was not reported: " .. LastMessage())

DefineTask("expired-task", "req-expired", "return 1", { expiresAt = 1234567870 })
Controller.Run("expired-task")
assert(LastMessage():find("expired", 1, true), "expired task was not rejected")

DefineTask("client-task", "req-client", "return 1",
    { expectedClient = { interface = 99999 } })
Controller.Run("client-task")
assert(LastMessage():find("client_mismatch", 1, true), "client mismatch was not rejected")

-- synchronous success commits a complete report and shows the notice
DefineTask("sync-task", "req-run-sync", 'print("auto hello"); return { ok = true }')
DefineTask("second-task", "req-run-busy", "return 2")
ResetOverlayState()
NewAutomationSession()
Controller.Run("sync-task")
local syncRecord = ns.GetAutomationRecord("req-run-sync")
assert(syncRecord and syncRecord.status == "succeeded" and syncRecord.resultAvailable,
    "sync task did not commit a succeeded record")
assert(overlayState.running == 1 and #overlayState.notices == 1,
    "sync task did not drive the status and notice overlays")
local syncNotice = overlayState.notices[1]
assert(syncNotice:find('"v":1', 1, true) and syncNotice:find('"task":"sync-task"', 1, true)
        and syncNotice:find('"run":"req-run-sync"', 1, true)
        and syncNotice:find('"ticket":"LYCHEE-', 1, true)
        and #syncNotice <= 512,
    "completion notice did not carry the expected identity fields")
local syncEvidence = ns.GetExport(syncRecord.ticket)
assert(syncEvidence and syncEvidence.source.kind == "automation_result"
        and syncEvidence.payload.content:find("auto hello", 1, true)
        and syncEvidence.payload.content:find('"ok":true', 1, true)
        and syncEvidence.payload.content:find('"character":"TestChar"', 1, true)
        and syncEvidence.payload.content:find('"realm":"TestRealm"', 1, true)
        and syncEvidence.environment.clientId == testClient,
    "sync task report lost its output, returns or environment")

-- repeating the same request re-shows the notice without re-executing
local noticeCountBeforeRepeat = #overlayState.notices
Controller.Run("sync-task")
assert(#ns.db.automation.order == 1, "repeated request executed the source again")
assert(#overlayState.notices == noticeCountBeforeRepeat + 1,
    "repeated request did not re-show its completion notice")

-- A small backlog of unacknowledged results is allowed; only the cap blocks.
local backlogCap = Controller.MaxUnacknowledgedResults()
assert(backlogCap >= 1, "unacknowledged result cap must allow at least one result")
DefineTask("cap-1", "req-run-cap-1", "return 1")
DefineTask("cap-2", "req-run-cap-2", "return 2")
DefineTask("cap-3", "req-run-cap-3", "return 3")
DefineTask("cap-4", "req-run-cap-4", "return 4")
NewAutomationSession()
for index = 1, backlogCap do
    Controller.Run("cap-" .. index)
end
assert(Controller.GetUnacknowledgedCount() == backlogCap,
    "the backlog did not reach the cap: " .. tostring(Controller.GetUnacknowledgedCount()))
assert(Controller.IsBusy(), "reaching the cap did not report busy")
messages = {}
Controller.Run("cap-4")
assert(LastMessage():find("disk write", 1, true),
    "the cap did not reject a further request: " .. LastMessage())

-- Acknowledging one result frees exactly one slot.
local freedTicket = ns.GetAutomationRecord("req-run-cap-1").ticket
Controller.HandleCommand("ack " .. freedTicket .. " received")
assert(Controller.GetUnacknowledgedCount() == backlogCap - 1,
    "ack did not free a backlog slot")
assert(not Controller.IsBusy() or backlogCap > 2,
    "ack left the controller busy below the cap")
Controller.Run("cap-4")
assert(ns.GetAutomationRecord("req-run-cap-4") ~= nil,
    "the freed slot did not accept a new run")

-- runtime errors commit a failed report with the error snapshot
DefineTask("boom-task", "req-run-boom", 'error("boom expected")')
NewAutomationSession()
Controller.Run("boom-task")
local boomRecord = ns.GetAutomationRecord("req-run-boom")
assert(boomRecord.status == "failed" and boomRecord.errorCode == "runtime_error",
    "runtime failure did not commit a failed record")
local boomEvidence = ns.GetExport(boomRecord.ticket)
assert(boomEvidence and boomEvidence.payload.content:find("boom expected", 1, true)
        and boomEvidence.payload.content:find('"stage":"runtime"', 1, true),
    "failed report lost the runtime error details")

-- compile errors fail before any task source runs
DefineTask("broken-task", "req-run-broken", "this is not lua")
NewAutomationSession()
Controller.Run("broken-task")
assert(ns.GetAutomationRecord("req-run-broken").errorCode == "compile_error",
    "compile failure did not record its error code")

-- async tasks stay active until Finish; cleanup handlers run at finalization
DefineTask("async-task", "req-run-async", [[
    lycheeAsyncFinish = Finish
    lycheeAsyncCleanup = OnCleanup
    SetAsync()
]])
NewAutomationSession()
Controller.Run("async-task")
assert(Controller.IsBusy(), "async task did not stay active")
local cleanupRan = false
lycheeAsyncCleanup(function() cleanupRan = true end)
lycheeAsyncFinish({ done = true })
assert(cleanupRan, "cleanup handlers did not run on finalize")
local asyncRecord = ns.GetAutomationRecord("req-run-async")
assert(asyncRecord.status == "succeeded" and asyncRecord.ticket,
    "async finish did not complete the execution")
assert(Controller.GetUnacknowledgedCount() >= 1,
    "a finished result was not counted as awaiting acknowledgement")
local asyncEvidence = ns.GetExport(asyncRecord.ticket)
assert(asyncEvidence.payload.content:find('"done":true', 1, true),
    "async returns were not serialized into the report")

-- cooperative cancel saves a cancelled report and runs cleanup
DefineTask("cancel-task", "req-run-cancel", [[
    lycheeCancelCleanup = OnCleanup
    SetAsync()
]])
NewAutomationSession()
Controller.Run("cancel-task")
local cancelCleanupRan = false
lycheeCancelCleanup(function() cancelCleanupRan = true end)
Controller.Cancel("cancel-task")
assert(cancelCleanupRan, "cancel did not run cleanup handlers")
local cancelRecord = ns.GetAutomationRecord("req-run-cancel")
assert(cancelRecord.status == "cancelled" and cancelRecord.ticket,
    "cancelled execution did not save its report")
messages = {}
Controller.Cancel("no-such-task")
assert(LastMessage():find("no-such-task", 1, true),
    "cancel for an unknown task did not report clearly")

-- report overflow commits an explicit failure report, never a fake success
DefineTask("overflow-task", "req-run-overflow", 'return string.rep("\\n", 700000)')
NewAutomationSession()
Controller.Run("overflow-task")
local overflowRecord = ns.GetAutomationRecord("req-run-overflow")
assert(overflowRecord.status == "failed"
        and overflowRecord.errorCode == "report_build_failed",
    "report overflow did not commit an explicit failure report")
local overflowEvidence = ns.GetExport(overflowRecord.ticket)
assert(overflowEvidence and #overflowEvidence.payload.content < 64 * 1024,
    "the minimal failure report was not small")

-- Stop hides the notice but keeps the pending record
Controller.Stop()
assert(ns.GetPendingExportCount() > 0, "stop discarded pending records")

-- --- controller: host read acknowledgement (ack) -------------------------------

-- The plugin cannot observe whether the host read a result, so the host reports
-- it back as `/dev auto ack <ticket> received|failed`. That must free the
-- backlog slot immediately and record the outcome on the index record.
DefineTask("ack-task", "req-run-ack", "return 'acked'")
NewAutomationSession()
messages = {}
Controller.Run("ack-task")
local ackTicket = ns.GetAutomationRecord("req-run-ack").ticket
assert(Controller.GetUnacknowledgedCount() == 1,
    "a fresh result was not counted as awaiting acknowledgement")

Controller.HandleCommand("ack " .. ackTicket .. " received")
local ackRecord = ns.GetAutomationRecord("req-run-ack")
assert(ackRecord.receivedStatus == "received",
    "ack did not record the read outcome: " .. tostring(ackRecord.receivedStatus))
assert(type(ackRecord.receivedAt) == "number" and ackRecord.receivedAt > 0,
    "ack did not record when the result was read")
assert(Controller.GetUnacknowledgedCount() == 0,
    "ack did not free the backlog slot")
assert(not Controller.IsBusy(), "ack left the controller busy below the cap")
assert(ns.AutomationResultState(ackRecord) == "received",
    "the result life cycle does not report the successful read")

-- A repeat ack must not free a second slot.
DefineTask("ack-task-2", "req-run-ack-2", "return 'acked twice'")
Controller.Run("ack-task-2")
local ackTicket2 = ns.GetAutomationRecord("req-run-ack-2").ticket
local countAfterRun = Controller.GetUnacknowledgedCount()
Controller.HandleCommand("ack " .. ackTicket .. " received")
assert(Controller.GetUnacknowledgedCount() == countAfterRun,
    "a repeat ack freed an extra backlog slot")

-- A failed read is recorded distinctly and must not be mistaken for success.
Controller.HandleCommand("ack " .. ackTicket2 .. " failed")
assert(ns.GetAutomationRecord("req-run-ack-2").receivedStatus == "failed",
    "ack failed status was not recorded")
assert(ns.AutomationResultState(ns.GetAutomationRecord("req-run-ack-2")) == "failed",
    "the result life cycle does not report the failed read")

-- Invalid input is rejected without touching the record.
Controller.HandleCommand("ack " .. ackTicket .. " maybe")
assert(LastMessage():find("received", 1, true),
    "invalid ack status did not report the accepted values: " .. LastMessage())
Controller.HandleCommand("ack LYCHEE-20200101-000000-9999 received")
assert(LastMessage():find("9999", 1, true),
    "ack for an unknown ticket did not report the ticket: " .. LastMessage())
Controller.HandleCommand("ack " .. ackTicket)
assert(LastMessage():find("run", 1, true),
    "ack without a status did not print usage: " .. LastMessage())

-- --- controller: bug snapshots -------------------------------------------------

local bugDatabase = {
    { message = "bug one", stack = "s1", locals = "x = 1", time = 10, session = 7, counter = 1 },
    { message = "bug two", stack = "s2", locals = "y = 2", time = 20, session = 7, counter = 1 },
}
BugGrabber = {
    GetDB = function() return bugDatabase end,
    GetSessionId = function() return 7 end,
}
NewAutomationSession()
local bugNoticeCount = #overlayState.notices
Controller.Bug("2")
local bugExecutionId = ns.db.automation.order[1]
local bugRecord = ns.GetAutomationRecord(bugExecutionId)
assert(bugRecord and bugRecord.taskId == "bug" and bugRecord.status == "succeeded",
    "bug snapshot did not commit a succeeded record")
local bugEvidence = ns.GetExport(bugRecord.ticket)
assert(bugEvidence.source.kind == "error_log"
        and bugEvidence.payload.content:find('"bug two"', 1, true)
        and bugEvidence.payload.content:find('"provider_storage_reverse"', 1, true),
    "bug snapshot report lost the error data or its ordering semantics")
assert(#overlayState.notices == bugNoticeCount + 1,
    "bug snapshot did not show its completion notice")

Controller.Bug("0")
assert(LastMessage():find("between 1 and 100", 1, true),
    "invalid bug count was not rejected")

NewAutomationSession()
BugGrabber = nil
Controller.Bug("5")
assert(LastMessage():find("provider_unavailable", 1, true),
    "missing error provider was not reported")

-- --- slash routing -------------------------------------------------------------

messages = {}
SlashCmdList.LYCHEEDEV("auto nonsense")
assert(#messages > 0 and messages[1]:find("run", 1, true),
    "/dev auto with an unknown action did not print usage")
messages = {}
SlashCmdList.LYCHEEDEV("auto bug")
assert(messages[1] ~= nil, "/dev auto bug without count did not answer")

inCombat = true
messages = {}
SlashCmdList.LYCHEEDEV("auto bug 1")
assert(messages[1]:find("combat", 1, true),
    "automation commands ran during combat")
inCombat = false

-- --- automation database: atomic commit, protection and capacity -------------

-- Isolated database so these fault-injection tests cannot disturb the
-- persisted-state assertions above.
LycheeDevDB = {
    schemaVersion = 8,
    history = {},
    exports = { version = 2, nextId = 1000, records = {}, order = {} },
}
NewAutomationSession()

assert(ns.BeginAutomationExecution("req-protected-keep", { taskId = "t-keep" }))
local keepTicket = assert(ns.CommitAutomationResult("req-protected-keep", '{"keep":true}', {
    status = "succeeded", complete = true,
}))
assert(ns.IsExportProtected(keepTicket), "the protected index fixture was not protected")

-- PruneAutomationIndex must skip a protected execution even when the index is
-- over its record budget.
for index = 1, 140 do
    local executionId = "req-index-fill-" .. index
    assert(ns.BeginAutomationExecution(executionId, { taskId = "t-index" }))
    assert(ns.FailAutomationExecution(executionId, "failed", "index_fill"))
end
assert(#ns.db.automation.order <= 100, "the automation index did not prune at its record limit")
assert(ns.GetAutomationRecord("req-index-fill-1") == nil,
    "an unprotected terminal record survived index pruning")
assert(ns.GetAutomationRecord("req-protected-keep")
        and ns.GetAutomationRecord("req-protected-keep").ticket == keepTicket,
    "index pruning removed a protected execution")

-- ClearAutomationHistory keeps protected executions and clears terminal ones.
local indexFillCount = #ns.db.automation.order
assert(indexFillCount > 1, "the index fixture did not build")
assert(ns.ClearAutomationHistory() >= indexFillCount - 1,
    "automation history clear skipped unprotected terminal records")
assert(ns.GetAutomationRecord("req-protected-keep")
        and ns.GetAutomationRecord("req-protected-keep").ticket == keepTicket,
    "automation history clear removed a protected execution")
assert(ns.GetAutomationRecord("req-index-fill-140") == nil,
    "automation history clear kept an unprotected terminal record")

-- A commit that cannot confirm its evidence insert must roll the partial record
-- back instead of leaving an index ticket without a payload (spec 5.4 step 3).
assert(ns.BeginAutomationExecution("req-atomic-fail", { taskId = "t-atomic" }))
local exportOrderBefore = #ns.db.exports.order
local protectedBefore = ns.GetProtectedExportCount()
local pendingBefore = ns.GetPendingExportCount()
local realExportRecords = ns.db.exports.records
ns.db.exports.records = setmetatable({}, {
    __index = function(_, key) return realExportRecords[key] end,
    __newindex = function() end, -- drop the evidence insert
})
local atomicTicket, atomicError = ns.CommitAutomationResult("req-atomic-fail",
    '{"atomic":true}', { status = "succeeded", complete = true })
ns.db.exports.records = realExportRecords
assert(atomicTicket == nil and atomicError == ns.L.EXPORT_FAILED,
    "the forced post-commit failure did not surface as EXPORT_FAILED")
local atomicRecord = ns.GetAutomationRecord("req-atomic-fail")
assert(atomicRecord and atomicRecord.status == "running" and atomicRecord.ticket == nil
        and atomicRecord.finishedAt == nil and atomicRecord.resultAvailable == false,
    "a failed commit left a half-committed index record")
assert(#ns.db.exports.order == exportOrderBefore, "a failed commit left an export order entry")
assert(ns.GetProtectedExportCount() == protectedBefore
        and ns.GetPendingExportCount() == pendingBefore,
    "a failed commit left a protection or pending mark behind")
assert(ns.FailAutomationExecution("req-atomic-fail", "failed", "commit_failed"),
    "the rolled-back record was not left active for finalization")

-- The Controller must finalize the index record when a commit fails: the
-- request id is already spent, so a non-terminal record would strand the index
-- until the next reload.
DefineTask("commit-fail-task", "req-run-commit-fail", "return 1")
messages = {}
local errorsBefore = overlayState.errors
local taskExportRecords = ns.db.exports.records
ns.db.exports.records = setmetatable({}, {
    __index = function(_, key) return taskExportRecords[key] end,
    __newindex = function() end,
})
Controller.Run("commit-fail-task")
ns.db.exports.records = taskExportRecords
local commitFailRecord = ns.GetAutomationRecord("req-run-commit-fail")
assert(commitFailRecord and commitFailRecord.status == "failed"
        and commitFailRecord.errorCode == "commit_failed"
        and commitFailRecord.ticket == nil and commitFailRecord.resultAvailable == false,
    "a failed commit left the execution non-terminal")
assert(LastMessage():find("automation result", 1, true),
    "a failed commit did not report the commit failure")
assert(overlayState.errors == errorsBefore + 1, "a failed commit did not show the error state")
assert(not Controller.IsBusy(), "a failed commit kept the controller busy")

-- CanFitExport must not prune protected records: when the export budget is held
-- entirely by protected pending results, further commits fail explicitly and
-- leave their index record untouched.
local protectedBeforeFill = ns.GetProtectedExportCount()
local filledTickets = 0
local capacityFailureId
for index = 1, 40 do
    local executionId = "req-capacity-" .. index
    assert(ns.BeginAutomationExecution(executionId, { taskId = "t-capacity" }))
    local capacityTicket, capacityError = ns.CommitAutomationResult(executionId,
        string.rep("c", 1024 * 1024), { status = "succeeded", complete = true })
    if capacityTicket then
        filledTickets = filledTickets + 1
    else
        capacityFailureId = executionId
        assert(capacityError == ns.L.EXPORT_TOO_LARGE,
            "an over-budget commit failed for an unexpected reason")
        local capacityRecord = ns.GetAutomationRecord(executionId)
        assert(capacityRecord and capacityRecord.status == "running"
                and capacityRecord.ticket == nil and capacityRecord.resultAvailable == false,
            "a capacity-exhausted commit mutated its index record")
        break
    end
end
assert(filledTickets > 0 and capacityFailureId,
    "the export budget was never exhausted by protected records")
assert(ns.GetProtectedExportCount() == protectedBeforeFill + filledTickets,
    "a protected record was pruned to admit an over-budget commit")
assert(ns.FailAutomationExecution(capacityFailureId, "failed", "capacity_exhausted"))

print = originalPrint
print("Lychee Dev automation tests passed")
