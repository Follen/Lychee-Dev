local namespace = {}
assert(loadfile(arg[1]))("Lychee Dev", namespace)
local writer = assert(namespace.CaptureWriter)
local function expectError(value, expected, limit)
    local result, err = writer.Encode(value, limit)
    assert(result == nil, "expected Encode to fail")
    assert(err == expected, "unexpected Encode error: " .. tostring(err))
end

local function expectDigestError(value, expected)
    local result, err = writer.DigestBytes(value)
    assert(result == nil, "expected DigestBytes to fail")
    assert(err == expected, "unexpected DigestBytes error: " .. tostring(err))
end

local function nested(depth)
    local root, current = {}, nil
    current = root
    for _ = 1, depth do
        current.child = {}
        current = current.child
    end
    return root
end

local function countedTree(leftCount, rightCount)
    local root = {left = {}, right = {}}
    for index = 1, leftCount do root.left[index] = false end
    for index = 1, rightCount do root.right[index] = false end
    return root
end

local text = assert(writer.Encode({z = "line\n", a = {true, 3, "quote\""}}))
assert(text == '{"a":[true,3,"quote\\\""],"z":"line\\n"}', text)
assert(writer.Encode({}) == "{}")
assert(writer.Encode({"first", "second"}) == '["first","second"]')
assert(writer.Encode({only = "value"}) == '{"only":"value"}')
assert(writer.DigestBytes("Wikipedia") == "11e60398")
assert(writer.DigestBytes("") == "00000001")

local checksumLimit = 512 * 1024
local checksumExact = assert(writer.DigestBytes(string.rep("a", checksumLimit)))
assert(#checksumExact == 8)
expectDigestError(string.rep("a", checksumLimit + 1), "checksum_byte_limit")
expectDigestError(42, "checksum_requires_bytes")

local exactReport = assert(writer.Encode(string.rep("a", checksumLimit - 2)))
assert(#exactReport == checksumLimit)
expectError(string.rep("a", checksumLimit - 1), "report_byte_limit")

local cyclic = {}
cyclic.child = cyclic
expectError(cyclic, "report_cycle")
expectError({mixed = function() end}, "report_unsupported_value")
expectError({[1] = "array", named = "object"}, "report_nonstring_key")
expectError({[2] = "hole"}, "report_nonstring_key")
expectError({[0] = "zero"}, "report_nonstring_key")
expectError(setmetatable({}, {}), "report_metatable")
expectError(string.char(255), "report_invalid_utf8")
expectError(string.char(128), "report_invalid_utf8")
expectError(string.char(194), "report_invalid_utf8")
expectError(string.char(192, 128), "report_invalid_utf8")
expectError(string.char(224, 128, 128), "report_invalid_utf8")
expectError(string.char(237, 160, 128), "report_invalid_utf8")
expectError(string.char(226, 130, 65), "report_invalid_utf8")
expectError(string.char(244, 144, 128, 128), "report_invalid_utf8")
local utf8 = string.char(228, 184, 150)
assert(writer.Encode(utf8) == "\"" .. utf8 .. "\"")
expectError({[string.char(255)] = true}, "report_invalid_utf8")
assert(writer.Encode(string.rep("a", 10), 11) == nil)
assert(writer.Encode(string.rep("a", 10), 12) == '"aaaaaaaaaa"')
expectError(math.huge, "report_nonfinite_number")
expectError(0 / 0, "report_nonfinite_number")
expectError({}, "report_invalid_budget", 0)
expectError({}, "report_invalid_budget", checksumLimit + 1)
expectError({}, "report_invalid_budget", 1.5)
expectError({}, "report_invalid_budget", "12")

assert(writer.Encode(nested(32)) ~= nil)
expectError(nested(33), "report_depth_limit")
assert(writer.Encode(countedTree(16383, 16383)) ~= nil)
expectError(countedTree(16384, 16383), "report_entry_limit")
expectError(countedTree(16384, 16384), "report_entry_limit")

local secret = {}
issecretvalue = function(value) return rawequal(value, secret) end
expectError({hidden = secret}, "report_secret_value")
expectError({[secret] = true}, "report_secret_value")
local digestOK, digestError = pcall(writer.DigestBytes, secret)
assert(not digestOK and digestError == "report_secret_value")
expectError({}, "report_invalid_budget", secret)
issecretvalue = nil

-- EncodeSignal projects a receipt onto its transmitted form. The identity the
-- host already proved is omitted to save QR modules, but the session nonce must
-- survive on the receipts that ESTABLISH it: ready and loaded are read before
-- the host has a nonce to fill from, so dropping it there makes every later
-- comparison fail with an empty nonce. Only receipts that echo an established
-- session may omit it.
local nonce = string.rep("a", 32)
local binding = '"sessionNonce":"' .. nonce .. '"'
-- Identity, reset and cleared markers re-establish the actor, so they are the
-- only kinds allowed to repeat it.
local actorBearing = { identity = true, reset = true, cleared = true, ready = true }
local function projected(kind)
    local text = assert(writer.EncodeSignal({
        schema = "lycheedev.signal.v1", kind = kind, sessionNonce = nonce,
        release = "2.0.2", product = "classic", build = "5.5.4.69934",
        sequence = 1, character = "次年雪", realm = "祈福", guid = "Player-1-2",
        inputReady = true,
    }, 4096))
    if actorBearing[kind] then
        assert(text:find('"character"', 1, true) ~= nil, kind .. " dropped the actor it establishes")
    else
        assert(text:find('"character"', 1, true) == nil and text:find('"realm"', 1, true) == nil,
            kind .. " repeated the actor the host already holds")
    end
    return text:find(binding, 1, true) ~= nil
end
for _, kind in ipairs({"ready", "loaded", "identity", "cleared", "reset"}) do
    assert(projected(kind), kind .. " dropped the session nonce it establishes")
end
for _, kind in ipairs({"reported", "acknowledged", "cancelled"}) do
    assert(not projected(kind), kind .. " repeated a session nonce the host holds")
end
-- The actor is never repeated on a session-shaped receipt.
local reported = assert(writer.EncodeSignal({schema = "lycheedev.signal.v1", kind = "reported",
    sessionNonce = nonce, release = "2.0.2", product = "classic", build = "5.5.4.69934",
    sequence = 1, requestId = "REQ-x", character = "次年雪", realm = "祈福", inputReady = false}, 4096))
assert(not reported:find("character", 1, true) and not reported:find("realm", 1, true))
assert(reported:find('"kind":"reported"', 1, true) and reported:find('"requestId":"REQ-x"', 1, true))

io.write("capture-writer: passed\n")
