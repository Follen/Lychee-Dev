-- Serializer bounds, markers, secrets and the streaming variant.
local Env, client, root = ...

local ns = Env.LoadWorkbench()
local S = ns.Serializer

-- Deterministic key ordering and scalar formatting.
local text = S.Serialize({ b = 2, a = 1, [10] = "ten", [2] = "two" })
assert(text:find('[2] = "two"', 1, true), "number keys are not ordered first")
assert(text:find('["a"] = 1', 1, true), "string keys are not serialized with quotes")
assert(text:find('["b"] = 2', 1, true), "table entries were lost")
assert(text:find("[10] = ", 1, true), "large number key was lost")

-- Scalars, strings and non-table userdata.
assert(S.Serialize(nil) == "nil" and S.Serialize(7) == "7" and S.Serialize(true) == "true",
    "scalar serialization changed")
assert(S.Serialize("x") == '"x"', "strings are not quoted")
local proxy = newproxy(true)
getmetatable(proxy).__tostring = function()
    return "thing"
end
assert(S.Serialize(proxy) == "<userdata: thing>", "non-table userdata marker changed")

-- Cycles.
local cyclic = {}
cyclic.self = cyclic
assert(S.Serialize(cyclic):find("<cycle>", 1, true), "cycles were not bounded")

-- Depth cap of 6 with the <max depth> marker.
local deep = { value = 1 }
local cursor = deep
for index = 1, 10 do
    cursor.child = {}
    cursor = cursor.child
end
local deepText = S.Serialize(deep)
assert(deepText:find("<max depth>", 1, true), "depth cap marker is missing")
local _, deepIncomplete = S.Serialize(deep)
assert(deepIncomplete == true, "depth truncation was not reported")

-- Entry cap of 200 per table with the omitted marker.
local wide = {}
for index = 1, 450 do
    wide["field" .. index] = index
end
local wideText, wideIncomplete = S.Serialize(wide)
assert(wideText:find("... <250 entries omitted>", 1, true), "entry cap marker is missing")
assert(wideIncomplete == true, "entry truncation was not reported")

-- Output cap of 44000 bytes with the truncated marker.
local floodText = S.Serialize(string.rep("x", 50000))
assert(#floodText < 45000 and floodText:find("<output truncated>", 1, true),
    "output cap did not bound a large string")

-- Secrets: values render as <secret>, keys are counted and skipped.
local secret = Env.MakeSecret()
assert(S.Serialize(secret) == "<secret>", "secret values were inspected")
local withSecretKeys = { visible = 1, [secret] = "hidden", [Env.MakeSecret()] = "also hidden" }
local secretText = S.Serialize(withSecretKeys)
assert(secretText:find('["visible"] = 1', 1, true), "visible keys were dropped next to secrets")
assert(secretText:find("... <2 secret keys omitted>", 1, true), "secret keys were not omitted visibly")

-- SerializeValues keeps the [i] = shape.
local values = { n = 3, "a", nil, 7 }
local valuesText = S.SerializeValues(values)
assert(valuesText:find('[1] = "a"', 1, true) and valuesText:find("[2] = nil", 1, true)
        and valuesText:find("[3] = 7", 1, true),
    "SerializeValues lost return positions")

-- Streaming: chunk cap 44000, resumable, no premature limit.
local streamedValue = {}
for index = 1, 3000 do
    streamedValue["streamField" .. index] = string.rep("v", 16)
end
local stream = S.CreateStream(streamedValue)
local firstChunk, firstFinished = stream:ReadChunk()
assert(#firstChunk <= 44000 and not firstFinished,
    "serialization stream did not stop after its first chunk")
local streamedParts = { firstChunk }
while not stream:IsFinished() do
    local chunk = stream:ReadChunk()
    streamedParts[#streamedParts + 1] = chunk
end
local streamedText = table.concat(streamedParts)
assert(streamedText:find('["streamField3000"]', 1, true),
    "serialization stream did not continue from its saved position")
assert(not stream:WasLimited(), "bounded stream unexpectedly hit its safety limit")

-- Streaming honors caller chunk sizes (yield granularity of 4096 bytes).
local smallStream = S.CreateStream(streamedValue)
local piece, done = smallStream:ReadChunk(64)
assert(#piece <= 64 and not done, "stream chunk size is not honored")

-- Stream depth cap of 32.
local deeper = {}
cursor = deeper
for index = 1, 40 do
    cursor.child = {}
    cursor = cursor.child
end
local deepStreamText, deepLimited = S.SerializeForExport(deeper)
assert(deepStreamText:find("<max depth>", 1, true) and deepLimited,
    "stream depth cap of 32 is not enforced")

-- Stream entry cap of 10000.
local huge = {}
for index = 1, 10001 do
    huge["k" .. index] = index
end
local hugeText, hugeLimited = S.SerializeForExport(huge)
assert(hugeText:find("... <1 entries omitted>", 1, true) and hugeLimited,
    "stream entry cap of 10000 is not enforced")

-- Stream total output cap of 16 MB with the limit marker and WasLimited.
local giantText, giantLimited = S.SerializeForExport(string.rep("x", 16 * 1024 * 1024))
assert(giantLimited and giantText:find("... <output limit reached>", 1, true),
    "stream output cap of 16 MB is not enforced")
assert(#giantText <= 16 * 1024 * 1024 + 32, "stream output cap was exceeded")

print("serializer ok")
