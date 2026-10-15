-- R4-07: unprintable stream errors must reach the actual Object page as data.
local Env = ...
local ns = Env.LoadWorkbench()
Env.LoadAddon("Modules/ObjectInspector.lua", ns)
Env.LoadAddon("UI/Pages/Object.lua", ns)
assert(ns.Persistence.Load())
assert(Env.framesCreated == 0 and #Env.pendingTimers == 0, "unused inspection created runtime work")

local unprintable = setmetatable({}, { __tostring = function() error("error formatting failed", 0) end })
local function BrokenObject(failure)
    local key = setmetatable({}, { __tostring = function() error(failure, 0) end })
    return { [key] = true, [{}] = false }
end
assert(ns.Workbench.ShowPage("objects"))
local page = assert(ns.Workbench.GetPage("objects"))
local function HasStatus(message)
    for _, child in ipairs(page.children) do
        if child:GetObjectType() == "FontString" and child:GetText() == message then return true end
    end
    return false
end

for _, case in ipairs({
    { failure = unprintable, message = "serialization_failed" },
    { failure = "ordinary serialization error", message = "ordinary serialization error" },
}) do
    ObjectStreamFailure = BrokenObject(case.failure)
    page.pathPanel.editBox:SetText("ObjectStreamFailure")
    local clicked, failure = pcall(function() page.inspectButton:Click() end)
    assert(clicked, "Object click leaked a stream error: " .. tostring(failure))
    assert(HasStatus(case.message), "stream error did not reach the page failure status")
    assert(page:GetSelection() == nil and not page.selectSnapshot:IsEnabled()
        and not page.exportSnapshot:IsEnabled(), "failed inspection published a successful snapshot")
    local ok, inspection, message = ns.ObjectInspector.InspectPath("ObjectStreamFailure")
    assert(not ok and inspection and message == case.message and inspection.textStream:IsFinished()
        and inspection.textStream:WasLimited() and inspection.textStream.errorMessage == case.message
        and inspection.tree == nil, "inspection lost its structured failure or partial stream")
end

-- Keep output already yielded before the later error, both within a ReadChunk
-- call and between calls; the terminal limited state and diagnostic persist.
for _, maxBytes in ipairs({ 44000, 64 }) do
    local stream = ns.Serializer.CreateStream({ a = string.rep("x", 5000), z = BrokenObject(unprintable) })
    local parts = {}
    repeat
        local chunk = stream:ReadChunk(maxBytes)
        parts[#parts + 1] = chunk
    until stream:IsFinished()
    assert(table.concat(parts):find(string.rep("x", 5000), 1, true), "stream discarded already yielded output")
    assert(stream:WasLimited() and stream.errorMessage == "serialization_failed",
        "stream failure lost its limited state or stable diagnostic")
    local chunk, finished = stream:ReadChunk(maxBytes)
    assert(chunk == "" and finished and stream:WasLimited() and stream.errorMessage == "serialization_failed",
        "reading a failed stream changed its terminal state")
end
local ok, partial, message = ns.ObjectInspector.InspectValue(
    { a = string.rep("x", 5000), z = BrokenObject(unprintable) }, "partial")
assert(not ok and message == "serialization_failed" and partial.text:find(string.rep("x", 5000), 1, true)
    and partial.textStream:WasLimited(), "inspection discarded its caller-visible partial text")
ObjectStreamHealthy = { answer = 42 }
page.pathPanel.editBox:SetText("ObjectStreamHealthy")
page.inspectButton:Click()
assert(page:GetSelection() == ObjectStreamHealthy and page.selectSnapshot:IsEnabled()
    and page.exportSnapshot:IsEnabled(), "Object page did not recover after a serialization failure")
print("Object stream errors through real page clicks and partial output ok")
