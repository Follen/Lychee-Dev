local ADDON_NAME, ns = ...

-- Bounded event monitor. One lazy driver frame (created on the first Start),
-- per-event RegisterEvent + IsEventRegistered verification with atomic
-- failure, and a fixed-size record ring. Stop unregisters everything; the
-- combat shutdown callback tears the driver down with it.
local L = ns.L

local MAX_RECORDS = 500
local MAX_ARGUMENTS = 16
local MAX_ARGUMENT_BYTES = 256

local monitor = {}
local driver
local records = {}
local head = 0
local count = 0
local changedCallback
local running = false
local activeEventCount = 0
local monitoringAllEvents = false
local startedAt = 0
local combatShutdownRegistered = false

local function IsSecret(value)
    return issecretvalue and issecretvalue(value)
end

-- GetTime may be restricted in combat; a secret clock degrades to a zero
-- elapsed time instead of throwing on arithmetic.
local function SafeTime()
    local now = GetTime()
    if IsSecret(now) or type(now) ~= "number" then
        return nil
    end
    return now
end

local function FormatArgument(value)
    if IsSecret(value) then
        return "<secret>"
    end

    local valueType = type(value)
    local text
    if valueType == "string" then
        text = string.format("%q", value)
    elseif valueType == "number" or valueType == "boolean" or valueType == "nil" then
        text = tostring(value)
    elseif valueType == "table" then
        text = "<table>"
    else
        text = "<" .. valueType .. ">"
    end

    if #text > MAX_ARGUMENT_BYTES then
        return text:sub(1, MAX_ARGUMENT_BYTES) .. "..."
    end
    return text
end

local function AddRecord(event, ...)
    local suppliedCount = select("#", ...)
    local argumentCount = math.min(suppliedCount, MAX_ARGUMENTS)
    local arguments = {}
    for index = 1, argumentCount do
        arguments[index] = FormatArgument(select(index, ...))
    end
    if suppliedCount > MAX_ARGUMENTS then
        arguments[#arguments + 1] = "<" .. (suppliedCount - MAX_ARGUMENTS) .. " more arguments>"
    end

    local now = SafeTime()
    local elapsed = 0
    if now then
        elapsed = math.max(0, now - startedAt)
    end

    head = head % MAX_RECORDS + 1
    records[head] = {
        elapsed = elapsed,
        event = event,
        arguments = arguments,
        summary = table.concat(arguments, ", "),
    }
    count = math.min(count + 1, MAX_RECORDS)

    if changedCallback then
        changedCallback(records[head])
    end
end

local function EnsureDriver()
    if driver then
        return driver
    end

    driver = CreateFrame("Frame")
    driver:SetScript("OnEvent", function(_, event, ...)
        AddRecord(event, ...)
    end)
    return driver
end

-- Registered once on the first Start so a session that never monitors never
-- creates the combat shutdown driver frame.
local function EnsureCombatShutdown()
    if combatShutdownRegistered then
        return
    end
    combatShutdownRegistered = true
    if type(ns.RegisterCombatShutdown) == "function" then
        ns.RegisterCombatShutdown(monitor.Stop)
    end
end

function monitor.Start(eventNames, callback)
    if ns.IsCombatBlocked() then
        return false, L.COMBAT_BLOCKED
    end
    if type(eventNames) ~= "table" or #eventNames == 0 then
        return false, L.SELECT_ONE_EVENT
    end

    local uniqueNames = {}
    local names = {}
    for index = 1, #eventNames do
        local eventName = eventNames[index]
        if type(eventName) ~= "string" or not eventName:match("^[A-Z][A-Z0-9_]*$") then
            local shown = IsSecret(eventName) and "<secret>" or tostring(eventName)
            return false, L.INVALID_EVENT .. shown
        elseif not uniqueNames[eventName] then
            uniqueNames[eventName] = true
            names[#names + 1] = eventName
        end
    end

    local eventDriver = EnsureDriver()
    eventDriver:UnregisterAllEvents()
    running = false
    activeEventCount = 0
    monitoringAllEvents = false
    changedCallback = nil

    if uniqueNames.ALL then
        local succeeded = pcall(eventDriver.RegisterAllEvents, eventDriver)
        if not succeeded then
            return false, L.UNAVAILABLE_EVENT .. "ALL"
        end
        changedCallback = callback
        running = true
        monitoringAllEvents = true
        activeEventCount = 1
        local now = SafeTime()
        startedAt = now or 0
        EnsureCombatShutdown()
        return true
    end

    for index = 1, #names do
        local eventName = names[index]
        local succeeded = pcall(eventDriver.RegisterEvent, eventDriver, eventName)
        local registered = succeeded and eventDriver:IsEventRegistered(eventName)
        if not registered or IsSecret(registered) then
            eventDriver:UnregisterAllEvents()
            return false, L.UNAVAILABLE_EVENT .. eventName
        end
    end

    changedCallback = callback
    running = true
    activeEventCount = #names
    local now = SafeTime()
    startedAt = now or 0
    EnsureCombatShutdown()
    return true
end

function monitor.Stop()
    if driver then
        driver:UnregisterAllEvents()
    end
    changedCallback = nil
    running = false
    activeEventCount = 0
    monitoringAllEvents = false
end

function monitor.Clear()
    wipe(records)
    head = 0
    count = 0
end

function monitor.GetCount()
    return count
end

-- newestIndex 1 is the newest record. The ring keeps MAX_RECORDS entries and
-- always retains the newest record.
function monitor.GetRecord(newestIndex)
    if type(newestIndex) ~= "number" or newestIndex < 1 or newestIndex > count then
        return nil
    end
    local physicalIndex = (head - newestIndex) % MAX_RECORDS + 1
    return records[physicalIndex]
end

function monitor.IsRunning()
    return running
end

function monitor.GetActiveEventCount()
    return activeEventCount
end

function monitor.IsMonitoringAllEvents()
    return monitoringAllEvents
end

ns.EventMonitor = monitor
