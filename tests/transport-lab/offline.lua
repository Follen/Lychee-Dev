local output = assert(arg[1], "usage: lua offline.lua <output-dir>")
GetServerTime = function() return 1780000000 end
GetTime = function() return 1.234 end
print = function() end
local frame
CreateFrame = function()
    frame = {
        RegisterEvent = function() end,
        UnregisterAllEvents = function() end,
        SetScript = function(self, event, callback) self[event] = callback end,
    }
    return frame
end
SlashCmdList = {}
dofile("tests/transport-lab/addon/LycheeMemoryLab2/Lab.lua")
assert(frame.OnEvent)
frame:OnEvent()

local function write(name, bytes)
    local handle = assert(io.open(output .. "/" .. name, "wb"))
    assert(handle:write(bytes))
    assert(handle:close())
end
for generation = 0, 5 do
    local state = assert(_G.LycheeMemoryLab2)
    assert(state.epoch == generation)
    write("head-" .. generation .. ".bin", state.head)
    for id, body in pairs(state.reports) do
        write(id .. "-" .. generation .. ".bin", body)
    end
    if generation < 5 then SlashCmdList.LYCHEEMEMORYLABTWO("next") end
end
assert(next(_G.LycheeMemoryLab2.reports) == nil)
local scheduled = {}
C_Timer = { NewTimer = function(delay, callback)
    assert(delay == 3)
    local item = { callback = callback, cancelled = false }
    function item:Cancel() self.cancelled = true end
    scheduled[#scheduled + 1] = item
    return item
end }
SlashCmdList.LYCHEEMEMORYLABTWO("reset")
SlashCmdList.LYCHEEMEMORYLABTWO("cycle")
for generation = 1, 5 do
    local item = assert(table.remove(scheduled, 1))
    assert(not item.cancelled)
    item.callback()
    assert(_G.LycheeMemoryLab2.epoch == generation)
end
assert(#scheduled == 0)
SlashCmdList.LYCHEEMEMORYLABTWO("clear")
assert(_G.LycheeMemoryLab2 == nil)
