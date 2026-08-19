local createdFrames = 0
local eventDriver
local clock = 100

local function NewDriver()
    local frame = {
        events = {},
        scripts = {},
    }

    function frame:SetScript(name, callback)
        self.scripts[name] = callback
    end

    function frame:RegisterEvent(eventName)
        if eventName == "UNKNOWN_EVENT" then
            return
        end
        self.events[eventName] = true
    end

    function frame:RegisterAllEvents()
        self.allEvents = true
    end

    function frame:IsEventRegistered(eventName)
        return self.events[eventName] and true or false
    end

    function frame:UnregisterAllEvents()
        wipe(self.events)
        self.allEvents = nil
    end

    return frame
end

function CreateFrame()
    createdFrames = createdFrames + 1
    eventDriver = NewDriver()
    return eventDriver
end

function GetTime()
    clock = clock + 0.1
    return clock
end

function issecretvalue()
    return false
end

function wipe(target)
    for key in pairs(target) do
        target[key] = nil
    end
end

local chunk, loadError = loadfile("EventMonitor.lua")
assert(chunk, loadError)
local ns = {}
local localeChunk, localeError = loadfile("Locale.lua")
assert(localeChunk, localeError)
localeChunk("Lychee Dev", ns)
local safetyChunk, safetyError = loadfile("Safety.lua")
assert(safetyChunk, safetyError)
safetyChunk("Lychee Dev", ns)
chunk("Lychee Dev", ns)

assert(createdFrames == 0, "event monitor created a frame before Start")

local callbackCount = 0
local succeeded, errorMessage = ns.EventMonitor.Start({ "PLAYER_TARGET_CHANGED", "PLAYER_REGEN_ENABLED" }, function()
    callbackCount = callbackCount + 1
end)
assert(succeeded, errorMessage)
assert(createdFrames == 1, "event monitor did not lazily create its driver")
assert(ns.EventMonitor.IsRunning(), "event monitor did not enter running state")
assert(ns.EventMonitor.GetActiveEventCount() == 2, "event monitor did not register every event")

eventDriver.scripts.OnEvent(eventDriver, "PLAYER_TARGET_CHANGED", "player", 7, true)
assert(callbackCount == 1, "event monitor did not notify its listener")
assert(ns.EventMonitor.GetCount() == 1, "event record was not stored")
local record = ns.EventMonitor.GetRecord(1)
assert(record.event == "PLAYER_TARGET_CHANGED", "wrong event was stored")
assert(record.elapsed >= 0 and record.elapsed < 1, "event elapsed time was not relative to Start")
assert(record.summary:find('"player"', 1, true), "event arguments were not formatted")

for index = 1, 510 do
    eventDriver.scripts.OnEvent(eventDriver, "PLAYER_REGEN_ENABLED", index)
end
assert(ns.EventMonitor.GetCount() == 500, "event ring buffer was not bounded")
assert(ns.EventMonitor.GetRecord(1).summary == "510", "event ring buffer lost newest record")

ns.EventMonitor.Stop()
assert(not ns.EventMonitor.IsRunning(), "event monitor did not stop")
assert(next(eventDriver.events) == nil, "event monitor left events registered")

ns.EventMonitor.Clear()
assert(ns.EventMonitor.GetCount() == 0, "event monitor did not clear records")

succeeded = ns.EventMonitor.Start({ "UNKNOWN_EVENT" })
assert(not succeeded, "event monitor accepted an unavailable event")

succeeded, errorMessage = ns.EventMonitor.Start({ "ALL", "UNIT_HEALTH" })
assert(succeeded, errorMessage)
assert(eventDriver.allEvents, "ALL mode did not call RegisterAllEvents")
assert(ns.EventMonitor.IsMonitoringAllEvents(), "ALL mode state was not exposed")
assert(ns.EventMonitor.GetActiveEventCount() == 1, "ALL mode was counted as individual events")
ns.EventMonitor.Stop()
assert(not eventDriver.allEvents, "ALL mode remained registered after Stop")
assert(not ns.EventMonitor.IsMonitoringAllEvents(), "ALL mode state remained active after Stop")

print("Lychee Dev event monitor tests passed")
