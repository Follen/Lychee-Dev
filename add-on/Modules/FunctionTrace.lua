local ADDON_NAME, ns = ...

local trace = {}
local MAX_RECORDS = 300
local MAX_ARGUMENTS = 16
local MAX_ARGUMENT_BYTES = 180
local hooks = {}
local records = {}
local head, count = 0, 0
local activePath
local enabled = false
local changedCallback
local startedAt = 0

local function IsSecret(value)
    return issecretvalue and issecretvalue(value)
end

local function FormatArgument(value)
    if IsSecret(value) then return "<secret>" end
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
    return #text > MAX_ARGUMENT_BYTES and (text:sub(1, MAX_ARGUMENT_BYTES) .. "...") or text
end

local function OnCall(path, ...)
    if not enabled or activePath ~= path or ns.IsCombatBlocked() then return end
    local suppliedCount = select("#", ...)
    local arguments = {}
    for index = 1, math.min(suppliedCount, MAX_ARGUMENTS) do
        arguments[index] = FormatArgument(select(index, ...))
    end
    if suppliedCount > MAX_ARGUMENTS then
        arguments[#arguments + 1] = "<" .. (suppliedCount - MAX_ARGUMENTS) .. " more arguments>"
    end
    head = head % MAX_RECORDS + 1
    records[head] = { elapsed = math.max(0, GetTime() - startedAt), path = path, arguments = arguments, summary = table.concat(arguments, ", ") }
    count = math.min(count + 1, MAX_RECORDS)
    if changedCallback then changedCallback(records[head]) end
end

function trace.Start(path, callback)
    if ns.IsCombatBlocked() then return false, ns.L.COMBAT_BLOCKED end
    path = tostring(path or ""):match("^%s*(.-)%s*$")
    local succeeded, owner, functionName, errorMessage = ns.ObjectInspector.ResolveFunctionTarget(path)
    if not succeeded then return false, errorMessage end
    local target = owner[functionName]
    if IsSecret(target) or type(target) ~= "function" then return false, ns.L.FUNCTION_NOT_FOUND end
    if not hooks[path] then
        local hooked = pcall(hooksecurefunc, owner, functionName, function(...) OnCall(path, ...) end)
        if not hooked then return false, ns.L.FUNCTION_HOOK_FAILED end
        hooks[path] = true
    end
    activePath, enabled, changedCallback, startedAt = path, true, callback, GetTime()
    return true
end

function trace.Stop()
    enabled, activePath, changedCallback = false, nil, nil
end

function trace.Clear()
    wipe(records)
    head, count = 0, 0
end

function trace.GetCount() return count end
function trace.GetRecord(newestIndex)
    if type(newestIndex) ~= "number" or newestIndex < 1 or newestIndex > count then return nil end
    return records[(head - newestIndex) % MAX_RECORDS + 1]
end
function trace.IsRunning() return enabled end
function trace.GetActivePath() return activePath end

ns.RegisterCombatShutdown(trace.Stop)
ns.FunctionTrace = trace
