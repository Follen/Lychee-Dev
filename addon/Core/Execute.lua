local ADDON_NAME, ns = ...
local L = ns.L

-- Explicit /run executor. Compile and runtime errors are returned as data, the
-- sandbox captures print/dump output under a hard byte cap, and the captured
-- globals never leak into _G. Global reads and writes pass through to _G so a
-- run behaves exactly like a chat /run.
local MAX_CAPTURE_BYTES = 44000

local function AddOutput(output, text)
    if output.truncated then
        return
    end

    text = tostring(text or "")
    local prefix = #output.parts > 0 and "\n" or ""
    local remaining = MAX_CAPTURE_BYTES - output.length - #prefix
    if #text > remaining then
        local marker = "\n... <output truncated>"
        local captured = table.concat(output.parts) .. prefix
            .. text:sub(1, math.max(0, remaining))
        output.parts = { captured:sub(1, MAX_CAPTURE_BYTES - #marker) .. marker }
        output.truncated = true
        output.length = #output.parts[1]
        return
    end

    output.parts[#output.parts + 1] = prefix .. text
    output.length = output.length + #prefix + #text
end

local function NormalizeCode(code)
    code = type(code) == "string" and code or ""
    code = code:gsub("^%s+", ""):gsub("%s+$", "")

    local prefixEnd = code:match("^/[Rr][Uu][Nn]%s+()")
        or code:match("^/[Ss][Cc][Rr][Ii][Pp][Tt]%s+()")
    if prefixEnd then
        code = code:sub(prefixEnd)
    end

    return code
end

local function BuildEnvironment(output)
    local environment = {}

    environment.print = function(...)
        local values = { n = select("#", ...), ... }
        local parts = {}
        for index = 1, values.n do
            if issecretvalue and issecretvalue(values[index]) then
                parts[index] = "<secret>"
            else
                parts[index] = tostring(values[index])
            end
        end
        AddOutput(output, table.concat(parts, "  "))
    end

    environment.dump = function(value)
        AddOutput(output, ns.Serializer.Serialize(value))
        return value
    end

    setmetatable(environment, { __index = _G, __newindex = _G })
    return environment
end

local function Pack(...)
    return { n = select("#", ...), ... }
end

-- Returns: succeeded, resultText, normalizedCode, liveTree, storedTree.
-- Failure paths return (false, resultText, normalizedCode); the trees are only
-- built for successful return values.
function ns.Execute(code)
    if ns.Safety.IsCombatBlocked() then
        return false, ns.L.COMBAT_BLOCKED, ""
    end
    code = NormalizeCode(code)
    if code == "" then
        return false, L.ENTER_LUA, code
    end

    local output = {
        parts = {},
        length = 0,
        truncated = false,
    }
    local chunk, compileError = loadstring(code, "LycheeDevInput")
    if not chunk then
        return false, L.COMPILE_ERROR .. tostring(compileError), code
    end

    setfenv(chunk, BuildEnvironment(output))

    local packed = Pack(pcall(chunk))
    local succeeded = packed[1]

    local formatted, liveTree, storedTree = pcall(function()
        if not succeeded then
            local diagnostic = L.RUNTIME_ERROR .. tostring(packed[2])
            if #diagnostic >= MAX_CAPTURE_BYTES then
                local marker = "\n... <output truncated>"
                diagnostic = diagnostic:sub(1, MAX_CAPTURE_BYTES - 1 - #marker) .. marker
            end
            local captured = table.concat(output.parts)
            if #captured + 1 + #diagnostic > MAX_CAPTURE_BYTES then
                output.parts = { captured:sub(1, MAX_CAPTURE_BYTES - #diagnostic - 1)
                    .. "\n" .. diagnostic }
                output.length = #output.parts[1]
            else
                -- A previous truncation must not suppress the actual failure.
                output.truncated = false
                AddOutput(output, diagnostic)
            end
            return
        end
        local values = { n = packed.n - 1 }
        for index = 2, packed.n do values[index - 1] = packed[index] end
        if values.n > 0 then AddOutput(output, ns.Serializer.SerializeValues(values)) end
        if #output.parts == 0 then AddOutput(output, L.COMPLETED_NO_OUTPUT) end
        return ns.Inspector.CreateLiveTree(values), ns.Inspector.CreateStoredTree(values)
    end)
    if not formatted then
        -- The chunk already ran. Formatting is a separate boundary and must
        -- never replay it or stringify another untrusted error object.
        local diagnostic = L.RESULT_FORMAT_ERROR
        if type(liveTree) == "string" and not (issecretvalue and issecretvalue(liveTree)) then
            diagnostic = diagnostic .. liveTree:sub(1, 512)
        end
        local captured = table.concat(output.parts)
        local remaining = math.max(0, MAX_CAPTURE_BYTES - #diagnostic - 1)
        return false, captured:sub(1, remaining) .. "\n" .. diagnostic, code
    end
    return succeeded, table.concat(output.parts), code, liveTree, storedTree
end
