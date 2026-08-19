local ADDON_NAME, ns = ...

local inspector = {}
local MAX_SEARCH_RESULTS = 200
local MAX_SEARCH_SCANS = 10000
local MAX_SEARCH_NODES = 2000
local MAX_SEARCH_DEPTH = 6
local MAX_PREVIEW_BYTES = 120

local function IsSecret(value)
    return issecretvalue and issecretvalue(value)
end

local function Trim(text)
    return tostring(text or ""):match("^%s*(.-)%s*$")
end

local function ParsePath(path)
    path = Trim(path)
    if path == "" then
        return nil, ns.L.OBJECT_PATH_REQUIRED
    elseif path == "_G" then
        return {}
    elseif path:sub(1, 3) == "_G." then
        path = path:sub(4)
    end

    local tokens = {}
    local remaining = path
    while remaining ~= "" do
        if remaining:sub(1, 1) == "." then
            remaining = remaining:sub(2)
        end
        local identifier, tail = remaining:match("^([_%a][_%w]*)(.*)$")
        if identifier then
            tokens[#tokens + 1] = identifier
            remaining = tail
        else
            local numberKey
            numberKey, tail = remaining:match("^%[(-?%d+)%](.*)$")
            if numberKey then
                tokens[#tokens + 1] = tonumber(numberKey)
                remaining = tail
            else
                local stringKey
                stringKey, tail = remaining:match('^%["([^"%]]+)"%](.*)$')
                if not stringKey then
                    stringKey, tail = remaining:match("^%['([^'%]]+)'%](.*)$")
                end
                if not stringKey then
                    return nil, ns.L.OBJECT_PATH_INVALID
                end
                tokens[#tokens + 1] = stringKey
                remaining = tail
            end
        end
        if remaining ~= "" and remaining:sub(1, 1) ~= "." and remaining:sub(1, 1) ~= "[" then
            return nil, ns.L.OBJECT_PATH_INVALID
        end
    end
    return tokens
end

local function ReadMember(value, key)
    if IsSecret(value) or IsSecret(key) then
        return false, nil, ns.L.SECRET_VALUE_BLOCKED
    elseif type(value) == "table" then
        return true, rawget(value, key)
    end
    local succeeded, result = pcall(function() return value[key] end)
    if not succeeded then
        return false, nil, ns.L.OBJECT_PATH_UNREADABLE
    end
    return true, result
end

local function ResolveTokens(tokens, count)
    local value = _G
    for index = 1, count or #tokens do
        local succeeded, nextValue, errorMessage = ReadMember(value, tokens[index])
        if not succeeded then
            return false, nil, errorMessage
        elseif nextValue == nil then
            return false, nil, ns.L.OBJECT_NOT_FOUND
        end
        value = nextValue
    end
    return true, value
end

local function SafePreview(value)
    if IsSecret(value) then return "<secret>" end
    local valueType = type(value)
    local text
    if valueType == "string" then
        text = string.format("%q", value)
    elseif valueType == "number" or valueType == "boolean" or valueType == "nil" then
        text = tostring(value)
    elseif valueType == "table" then
        text = "table"
    else
        local succeeded, result = pcall(tostring, value)
        text = succeeded and result or ("<" .. valueType .. ">")
    end
    return #text > MAX_PREVIEW_BYTES and (text:sub(1, MAX_PREVIEW_BYTES) .. "...") or text
end

local function SafeMethod(value, methodName)
    if IsSecret(value) then return nil end
    local methodSucceeded, method = pcall(function() return value[methodName] end)
    if not methodSucceeded or type(method) ~= "function" then return nil end
    local succeeded, result = pcall(method, value)
    if not succeeded or IsSecret(result) then return nil end
    return result
end

local function SafeMethodResults(value, methodName)
    if IsSecret(value) then return nil end
    local methodSucceeded, method = pcall(function() return value[methodName] end)
    if not methodSucceeded or type(method) ~= "function" then return nil end
    local packed = { pcall(method, value) }
    if not packed[1] then return nil end
    local results = {}
    for index = 2, #packed do
        if packed[index] ~= nil and not IsSecret(packed[index]) then
            results[#results + 1] = packed[index]
        end
    end
    return results
end

local function IsScriptRegion(value)
    if IsSecret(value) then return false end
    local succeeded, method = pcall(function() return value.GetObjectType end)
    return succeeded and type(method) == "function"
end

local function BuildInspection(value, label)
    if IsSecret(value) then return nil, ns.L.SECRET_VALUE_BLOCKED end
    local isFrame = IsScriptRegion(value)
    return {
        label = label,
        valueType = type(value),
        value = value,
        preview = SafePreview(value),
        isFrame = isFrame,
        text = (label or "") .. "  ·  " .. type(value) .. "\n" .. ns.Serialize(value),
        tree = ns.CreateValueTree({ n = 1, value }),
    }
end

local function GetMouseFocus()
    local foci = GetMouseFoci()
    if IsSecret(foci) or type(foci) ~= "table" or not foci[1] or IsSecret(foci[1]) then
        return nil
    end
    return foci[1]
end

function inspector.ResolvePath(path)
    local tokens, errorMessage = ParsePath(path)
    if not tokens then return false, nil, errorMessage end
    return ResolveTokens(tokens)
end

function inspector.ResolveFunctionTarget(path)
    local tokens, errorMessage = ParsePath(path)
    if not tokens then return false, nil, nil, errorMessage end
    if #tokens == 0 or type(tokens[#tokens]) ~= "string" then
        return false, nil, nil, ns.L.FUNCTION_PATH_INVALID
    end
    local key = tokens[#tokens]
    local succeeded, owner, resolveError = ResolveTokens(tokens, #tokens - 1)
    if not succeeded then return false, nil, nil, resolveError end
    return true, owner, key
end

function inspector.InspectPath(path)
    if ns.IsCombatBlocked() then return false, nil, ns.L.COMBAT_BLOCKED end
    local succeeded, value, errorMessage = inspector.ResolvePath(path)
    if not succeeded then return false, nil, errorMessage end
    local inspection, inspectError = BuildInspection(value, Trim(path))
    return inspection ~= nil, inspection, inspectError
end

function inspector.InspectValue(value, label)
    if ns.IsCombatBlocked() then return false, nil, ns.L.COMBAT_BLOCKED end
    local inspection, inspectError = BuildInspection(value, Trim(label))
    return inspection ~= nil, inspection, inspectError
end

function inspector.CaptureMouseFocus()
    if ns.IsCombatBlocked() then return false, nil, ns.L.COMBAT_BLOCKED end
    local focus = GetMouseFocus()
    if not focus then
        return false, nil, ns.L.NO_MOUSE_FOCUS
    end
    local inspection, errorMessage = BuildInspection(focus, SafeMethod(focus, "GetName") or SafePreview(focus))
    return inspection ~= nil, inspection, errorMessage
end

function inspector.GetMouseFocusLabel()
    if ns.IsCombatBlocked() then return false, nil, nil, ns.L.COMBAT_BLOCKED end
    local focus = GetMouseFocus()
    if not focus then return false, nil, nil, ns.L.NO_MOUSE_FOCUS end
    return true,
        SafeMethod(focus, "GetName") or SafePreview(focus),
        SafeMethod(focus, "GetObjectType") or type(focus)
end

function inspector.SearchValue(value, query, limit, maxDepth)
    if ns.IsCombatBlocked() then return false, nil, ns.L.COMBAT_BLOCKED end
    query = Trim(query):lower()
    if query == "" then return false, nil, ns.L.SEARCH_TERM_REQUIRED end
    if (type(value) ~= "table" and not IsScriptRegion(value)) or IsSecret(value) then
        return false, nil, ns.L.SEARCH_REQUIRES_TABLE
    end

    limit = math.max(1, math.floor(tonumber(limit) or MAX_SEARCH_RESULTS))
    maxDepth = math.max(0, math.floor(tonumber(maxDepth) or MAX_SEARCH_DEPTH))
    local exact, prefixes, contains = {}, {}, {}
    local matchCount = 0
    local scanned = 0
    local visitedCount = 0
    local seen = {}
    local stack = { { value = value, path = "", depth = 0 } }

    local function ChildPath(path, key)
        local segment
        if type(key) == "string" and key:match("^[_%a][_%w]*$") then
            segment = key
            return path == "" and segment or (path .. "." .. segment)
        end
        segment = "[" .. SafePreview(key) .. "]"
        return path .. segment
    end

    local function Push(childValue, path, depth)
        if depth <= maxDepth and (type(childValue) == "table" or IsScriptRegion(childValue))
            and not IsSecret(childValue) and not seen[childValue] then
            stack[#stack + 1] = { value = childValue, path = path, depth = depth }
        end
    end

    local function AddMatch(key, childValue, path)
        if type(key) ~= "string" or IsSecret(key) then return end
        local lowerKey = key:lower()
        local matchStart = lowerKey:find(query, 1, true)
        if not matchStart then return end
        matchCount = matchCount + 1
        local bucket = lowerKey == query and exact or (matchStart == 1 and prefixes or contains)
        if #exact + #prefixes + #contains < limit and not IsSecret(childValue) then
            bucket[#bucket + 1] = {
                key = key,
                path = path,
                value = childValue,
                valueType = type(childValue),
                preview = SafePreview(childValue),
            }
        end
    end

    while #stack > 0 and scanned < MAX_SEARCH_SCANS and visitedCount < MAX_SEARCH_NODES do
        local item = stack[#stack]
        stack[#stack] = nil
        local current = item.value
        if not seen[current] then
            seen[current] = true
            visitedCount = visitedCount + 1

            if type(current) == "table" then
                local cursor
                while scanned < MAX_SEARCH_SCANS do
                    local succeeded, key, childValue = pcall(next, current, cursor)
                    if not succeeded or key == nil then break end
                    cursor = key
                    scanned = scanned + 1
                    if not IsSecret(key) then
                        local path = ChildPath(item.path, key)
                        AddMatch(key, childValue, path)
                        Push(childValue, path, item.depth + 1)
                    end
                end
            end

            if item.depth < maxDepth and IsScriptRegion(current) then
                local childFrames = SafeMethodResults(current, "GetChildren") or {}
                for index = 1, #childFrames do
                    Push(childFrames[index], ChildPath(item.path, ns.L.FRAME_CHILDREN) .. "[" .. index .. "]", item.depth + 1)
                end
                local regions = SafeMethodResults(current, "GetRegions") or {}
                for index = 1, #regions do
                    Push(regions[index], ChildPath(item.path, ns.L.FRAME_REGIONS) .. "[" .. index .. "]", item.depth + 1)
                end
            end

            local metaSucceeded, metatable = pcall(getmetatable, current)
            if metaSucceeded and type(metatable) == "table" then
                local indexTable = rawget(metatable, "__index")
                if type(indexTable) == "table" then
                    Push(indexTable, item.path, item.depth + 1)
                end
            end
        end
    end
    table.sort(prefixes, function(left, right) return left.key < right.key end)
    table.sort(contains, function(left, right) return left.key < right.key end)
    local results = {}
    local function Append(bucket)
        for index = 1, #bucket do
            if #results >= limit then return end
            results[#results + 1] = bucket[index]
        end
    end
    Append(exact) Append(prefixes) Append(contains)
    return true, {
        results = results,
        totalMatches = matchCount,
        truncated = matchCount > #results or scanned >= MAX_SEARCH_SCANS or visitedCount >= MAX_SEARCH_NODES,
    }
end

function inspector.SearchGlobal(query, limit)
    return inspector.SearchValue(_G, query, limit, 0)
end

function inspector.SearchPath(path, query, limit)
    if ns.IsCombatBlocked() then return false, nil, ns.L.COMBAT_BLOCKED end
    local succeeded, value, errorMessage = inspector.ResolvePath(path)
    if not succeeded then return false, nil, errorMessage end
    return inspector.SearchValue(value, query, limit)
end

ns.ObjectInspector = inspector
