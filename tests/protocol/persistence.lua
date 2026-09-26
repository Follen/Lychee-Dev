local root = assert(arg[1])
local ns = { Release = "2.0.4", Startup = { ready = true,
    identity = { product = "retail", build = "12.1.0.69875" } } }
ns.Platform = { ObserveActor = function() return { character = "character", realm = "realm", guid = "Player-1-123" } end }
local secret = {}
issecretvalue = function(value) return rawequal(value, secret) end
CreateFrame = function() error("report store must not create frames") end
for _, source in ipairs({ "Core/Persistence.lua", "Bridge/Session.lua", "Bridge/CaptureWriter.lua", "Bridge/ReportStore.lua" }) do
    assert(loadfile(root .. "/" .. source))("Lychee Dev", ns)
end
assert(ns.Persistence.Load())
local requestId, nonce = "OP-persisted", string.rep("a", 32)
local code = "return 42"
local result, failure = ns.ReportStore.Commit(requestId, code, { answer = 42 })
assert(result == nil and failure == "bridge_disabled" and next(LycheeToolkitDB.reports) == nil)
LycheeToolkitDB.options.bridgeEnabled = true
result, failure = ns.ReportStore.Commit(requestId, code, { answer = 42 })
assert(result == nil and failure == "session_unbound")
assert(ns.Session.Bind(nonce))
result, failure = ns.ReportStore.Commit({ requestId = requestId, character = "forged", sequence = 99 }, code, { answer = 42 })
assert(result == nil and failure == "report_invalid_request" and ns.Session.Current().sequence == 0)
for sequence = 1, 3 do assert(ns.Session.NextIdentity().sequence == sequence) end
assert(ns.Session.Bind(nonce).sequence == 3, "idempotent binding reset sequence")
local receipt = assert(ns.ReportStore.Commit(requestId, code, { answer = 42, text = "\228\184\150\231\149\140" }))
assert(ns.Session.Current().sequence == 4)
local saved = LycheeToolkitDB.reports[requestId]
result, failure = ns.ReportStore.Commit(requestId, code, { answer = 99 })
assert(result == nil and failure == "report_request_exists" and LycheeToolkitDB.reports[requestId] == saved)
assert(ns.Session.Current().sequence == 4, "duplicate report consumed sequence")
local originalReceipt, originalBody = ns.ReportStore.Read(requestId)
assert(originalReceipt == receipt and originalBody == saved.body)
local function reject(id, value, expected)
    local success, reason = ns.ReportStore.Commit(id, code, value)
    assert(success == nil and reason == expected, tostring(reason))
    assert(LycheeToolkitDB.reports[id] == nil)
end
reject("OP-secret", secret, "report_secret_value")
reject("OP-large", string.rep("x", 512 * 1024 + 1), "report_byte_limit")
local cycle = {}; cycle.self = cycle
reject("OP-cycle", cycle, "report_cycle")
for index = 2, 100 do
    local previousSequence = ns.Session.Current().sequence
    assert(ns.ReportStore.Commit("OP-" .. index, "", { ordinal = index }))
    assert(ns.Session.Current().sequence == previousSequence + 1)
end
reject("OP-overflow", { answer = 1 }, "report_count_limit")
local retained = LycheeToolkitDB.reports
LycheeToolkitDB.reports = {}
local large = string.rep("x", 512 * 1024 - 4)
for index = 1, 31 do
    assert(ns.ReportStore.Commit("OP-budget-" .. index, "", large))
end
reject("OP-budget-overflow", large, "report_store_limit")
LycheeToolkitDB.reports = retained
local ackId = "OP-2"
local ackReceipt, ackBody = ns.ReportStore.Read(ackId)
local ackDigest = ns.CaptureWriter.DigestBytes(ackBody)
local original = { schema = "lycheedev.signal.v1", release = ns.Release, kind = "reported",
    requestId = ackId, sessionNonce = nonce, character = "character", realm = "realm",
    product = "retail", build = "12.1.0.69875", inputReady = false,
    sequence = tonumber(string.match(ackReceipt, '"sequence":(%d+)')),
    reportBytes = #ackBody, reportAdler32 = ackDigest }
assert(ns.CaptureWriter.EncodeSignal(original) == ackReceipt)
local function amended(field, value)
    local copy = {}
    for key, item in pairs(original) do copy[key] = item end
    copy[field] = value
    return copy
end
local function rejectAck(value, expected)
    local success, reason = ns.ReportStore.Acknowledge(value)
    assert(success == nil and reason == expected, tostring(reason))
    assert(LycheeToolkitDB.reports[ackId] ~= nil)
end
-- An added field is not part of the transmitted signal, so the canonical
-- projection ignores it and the stored receipt still matches; a field the wire
-- does carry cannot be changed without changing that projection.
assert(ns.ReportStore.Acknowledge(amended("extra", true)) ~= nil)
assert(LycheeToolkitDB.reports[ackId] == nil)
LycheeToolkitDB.reports[ackId] = { receipt = ackReceipt, body = ackBody }
rejectAck(amended("inputReady", true), "report_acknowledgement_mismatch")
rejectAck(amended("reportBytes", #ackBody + 1), "report_acknowledgement_mismatch")
rejectAck(amended("reportAdler32", "ffffffff"), "report_acknowledgement_mismatch")
rejectAck(amended("requestId", "OP-missing"), "report_unavailable")
rejectAck(secret, "report_invalid_acknowledgement")
rejectAck(amended("reportBytes", secret), "report_secret_value")
rejectAck(amended("reportAdler32", secret), "report_secret_value")
rejectAck(amended("reportBytes", 0/0), "report_nonfinite_number")
rejectAck(amended("sequence", 0), "report_invalid_acknowledgement")
rejectAck(amended("sequence", 1.5), "report_invalid_acknowledgement")
rejectAck(amended("sessionNonce", string.rep("b",32)), "report_acknowledgement_identity")
rejectAck(amended("character", "wrong"), "report_acknowledgement_identity")
rejectAck(amended("build", "12.1.0.99999"), "report_acknowledgement_identity")
LycheeToolkitDB.options.bridgeEnabled = false
rejectAck(original, "bridge_disabled")
LycheeToolkitDB.options.bridgeEnabled = true
local ackRecord = LycheeToolkitDB.reports[ackId]
ackRecord.body = ackBody .. " "
rejectAck(original, "report_acknowledgement_mismatch")
ackRecord.body = ackBody
ns.Session.Release()
rejectAck(original, "session_unbound")
assert(ns.Session.Bind(nonce).sequence == 0)
local acknowledgement = assert(ns.ReportStore.Acknowledge(original))
assert(ns.Session.Current().sequence == original.sequence + 1)
assert(string.find(acknowledgement, '"kind":"acknowledged"', 1, true))
assert(LycheeToolkitDB.reports[ackId] == nil)
assert(LycheeToolkitDB.reports["OP-persisted"] == saved)
local acknowledged, ackFailure = ns.ReportStore.Acknowledge(original)
assert(acknowledged == nil and ackFailure == "report_unavailable")
-- Emit a SavedVariables literal from the real committed record, not a JSON
-- surrogate. No user database or game file is read or written by this fixture.
io.write("LycheeToolkitDB = { schema = 1, reports = { [\"OP-persisted\"] = { receipt = ")
io.write(string.format("%q", originalReceipt))
io.write(", body = " .. string.format("%q", originalBody) .. " }, }, }\n")
