local root = assert(arg[1])
local ns = { Release = "2.0.2", Startup = { ready = true,
    identity = { product = "retail", build = "12.1.0.69875" } } }
ns.Platform = { ObserveActor = function() return { character = "Paladin", realm = "Realm", guid = "Player-1-123" } end }
local secret = {}
issecretvalue = function(value) return rawequal(value, secret) end
CreateFrame = function() error("acknowledgement must not create frames") end
for _, name in ipairs({"Core/Persistence.lua", "Bridge/Session.lua", "Bridge/CaptureWriter.lua", "Bridge/ReportStore.lua"}) do
    assert(loadfile(root .. "/" .. name))("Lychee Dev", ns)
end
assert(ns.Persistence.Load())
LycheeToolkitDB.options.bridgeEnabled = true
local nonce, code = string.rep("a",32), "return {answer=42}"
assert(ns.Session.Bind(nonce))
assert(ns.ReportStore.Acknowledged("OP-ack") == nil)
assert(ns.ReportStore.Acknowledged(secret) == nil)
for _, value in ipairs({secret, false, -1, 1.5, 0/0, 9007199254740992}) do
    assert(ns.Session.NextIdentity(value) == nil and ns.Session.Current().sequence == 0)
end
assert(ns.Session.NextIdentity(100).sequence == 101)
local receipt = assert(ns.ReportStore.Commit("OP-ack", code, {answer=42}))
local _, body = ns.ReportStore.Read("OP-ack")
assert(ns.ReportStore.Commit("OP-other", "", {keep=true}))
local original = {schema="lycheedev.signal.v1",release=ns.Release,kind="reported",sessionNonce=nonce,
    requestId="OP-ack",character="Paladin",realm="Realm",product="retail",build="12.1.0.69875",
    sequence=102,inputReady=false,codeBytes=#code,codeAdler32=ns.CaptureWriter.DigestBytes(code),
    reportBytes=#body,reportAdler32=ns.CaptureWriter.DigestBytes(body)}
assert(ns.CaptureWriter.Encode(original) == receipt)
-- Recreate the Session module, preserving only the persisted reports.
assert(loadfile(root .. "/Bridge/Session.lua"))("Lychee Dev",ns)
assert(ns.Session.Bind(string.rep("b",32)))
local value,reason = ns.ReportStore.Acknowledge(original)
assert(value == nil and reason == "report_acknowledgement_identity" and ns.ReportStore.Read("OP-ack") == receipt)
ns.Session.Release()
assert(ns.Session.Bind(nonce).sequence == 0)
local encoder = ns.CaptureWriter.Encode
ns.CaptureWriter.Encode = function(value,limit)
    if type(value) == "table" and value.kind == "acknowledged" then return nil,"fixture_encode_failure" end
    return encoder(value,limit)
end
value,reason = ns.ReportStore.Acknowledge(original)
assert(value == nil and reason == "fixture_encode_failure" and ns.ReportStore.Read("OP-ack") == receipt)
ns.CaptureWriter.Encode = encoder
local acknowledgement = assert(ns.ReportStore.Acknowledge(original))
assert(ns.ReportStore.Acknowledged("OP-ack") == acknowledgement)
assert(ns.ReportStore.Acknowledged("OP-other") == nil)
LycheeToolkitDB.reports["OP-ack"] = {receipt=receipt,body=body}
assert(ns.ReportStore.Acknowledged("OP-ack") == nil, "reappeared report granted cleanup")
LycheeToolkitDB.reports["OP-ack"] = nil
assert(ns.ReportStore.Acknowledged("OP-ack") == acknowledgement)
assert(ns.ReportStore.Read("OP-ack") == nil and ns.ReportStore.Read("OP-other"))
assert(original.kind == "reported" and original.sequence == 102, "caller receipt mutated")
value,reason = ns.ReportStore.Acknowledge(original)
assert(value == nil and reason == "report_unavailable")
local last = assert(ns.Session.NextIdentity(9007199254740990))
assert(last.sequence == 9007199254740991)
assert(ns.CaptureWriter.Encode(last.sequence) == "9007199254740991")
assert(ns.Session.NextIdentity() == nil)
local otherReceipt, otherBody = ns.ReportStore.Read("OP-other")
local other = {}
for key,item in pairs(original) do other[key] = item end
other.requestId, other.sequence, other.codeBytes, other.codeAdler32 = "OP-other", 103, nil, nil
other.reportBytes, other.reportAdler32 = #otherBody, ns.CaptureWriter.DigestBytes(otherBody)
assert(ns.CaptureWriter.Encode(other) == otherReceipt)
value,reason = ns.ReportStore.Acknowledge(other)
assert(value == nil and reason == "session_sequence_exhausted" and ns.ReportStore.Read("OP-other") == otherReceipt)
LycheeToolkitDB.options.bridgeEnabled = nil
assert(ns.ReportStore.Acknowledged("OP-ack") == nil)
LycheeToolkitDB.options.bridgeEnabled = true
assert(ns.Session.Bind(nonce))
assert(ns.ReportStore.Acknowledged("OP-ack") == nil, "rebind revived old acknowledgement")
ns.Session.Release()
assert(ns.Session.Bind(string.rep("f",32)))
assert(ns.ReportStore.Acknowledged("OP-ack") == nil, "foreign binding granted cleanup")
io.write(assert(ns.CaptureWriter.Encode({receipt=receipt,body=body,code=code,acknowledgement=acknowledgement})))
