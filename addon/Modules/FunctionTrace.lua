local ADDON_NAME, ns = ...

-- Bounded function-call tracing. One hooksecurefunc hook is installed per path
-- and never removed; Stop only disables recording so the hook degrades to a
-- constant-time guard. Records live in a 300-entry ring buffer.
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
local combatShutdownRegistered = false

local function IsSecret(value)
    return issecretvalue and issecretvalue(value)
end

local function Trim(text)
    if IsSecret(text) then
        return ""
    end
    return tostring(text or ""):match("^%s*(.-)%s*$")
end

local function Wipe(target)
    if wipe then
        wipe(target)
        return
    end
    for key in pairs(target) do
        target[key] = nil
    end
end

-- Strings are %q-quoted, tables collapse to <table>, secrets to <secret>, and
-- every argument text is capped at 180 bytes with a visible "..." suffix.
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
    return #text > MAX_ARGUMENT_BYTES and (text:sub(1, MAX_ARGUMENT_BYTES) .. "...") or text
end

local function OnCall(path, ...)
    if not enabled or activePath ~= path or ns.Safety.IsCombatBlocked() then
        return
    end
    local suppliedCount = select("#", ...)
    local arguments = {}
    for index = 1, math.min(suppliedCount, MAX_ARGUMENTS) do
        arguments[index] = FormatArgument(select(index, ...))
    end
    if suppliedCount > MAX_ARGUMENTS then
        arguments[#arguments + 1] = "<" .. (suppliedCount - MAX_ARGUMENTS) .. " more arguments>"
    end
    head = head % MAX_RECORDS + 1
    records[head] = {
        elapsed = math.max(0, GetTime() - startedAt),
        path = path,
        arguments = arguments,
        summary = table.concat(arguments, ", "),
    }
    count = math.min(count + 1, MAX_RECORDS)
    if changedCallback then
        changedCallback(records[head])
    end
end

function trace.Start(path, callback)
    if ns.Safety.IsCombatBlocked() then
        return false, ns.L.COMBAT_BLOCKED
    end
    if IsSecret(path) then
        return false, ns.L.FUNCTION_PATH_INVALID
    end
    path = Trim(path)
    local succeeded, owner, functionName, errorMessage = ns.ObjectInspector.ResolveFunctionTarget(path)
    if not succeeded then
        return false, errorMessage
    end
    local target = owner[functionName]
    if IsSecret(target) or type(target) ~= "function" then
        return false, ns.L.FUNCTION_NOT_FOUND
    end
    -- Install once per path. A failed install leaves no partial state behind.
    if not hooks[path] then
        local hooked = pcall(hooksecurefunc, owner, functionName, function(...) OnCall(path, ...) end)
        if not hooked then
            return false, ns.L.FUNCTION_HOOK_FAILED
        end
        hooks[path] = true
    end
    -- The combat shutdown driver is registered lazily on the first trace so an
    -- addon session that never traces creates no frame.
    if not combatShutdownRegistered then
        combatShutdownRegistered = true
        ns.Safety.RegisterCombatShutdown(trace.Stop)
    end
    activePath, enabled, changedCallback, startedAt = path, true, callback, GetTime()
    return true
end

-- Hooks are never removed: Stop only disables recording and the hook stays a
-- constant-time guard for the rest of the session.
function trace.Stop()
    enabled, activePath, changedCallback = false, nil, nil
end

function trace.Clear()
    Wipe(records)
    head, count = 0, 0
end

function trace.GetCount()
    return count
end

-- newestIndex 1 is the newest record.
function trace.GetRecord(newestIndex)
    if type(newestIndex) ~= "number" or newestIndex < 1 or newestIndex > count then
        return nil
    end
    return records[(head - newestIndex) % MAX_RECORDS + 1]
end

function trace.IsRunning()
    return enabled
end

function trace.GetActivePath()
    return activePath
end

ns.FunctionTrace = trace
