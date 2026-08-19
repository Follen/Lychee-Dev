local ADDON_NAME, ns = ...
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

local function IsSecret(value)
    return issecretvalue and issecretvalue(value)
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

    head = head % MAX_RECORDS + 1
    records[head] = {
        elapsed = math.max(0, GetTime() - startedAt),
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
            return false, L.INVALID_EVENT .. tostring(eventName)
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
        startedAt = GetTime()
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
    startedAt = GetTime()
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
ns.RegisterCombatShutdown(monitor.Stop)
