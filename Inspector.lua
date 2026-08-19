local ADDON_NAME, ns = ...

local PAGE_SIZE = 200
local MAX_SCANS_PER_PAGE = PAGE_SIZE * 4

local function IsSecret(value)
    return issecretvalue and issecretvalue(value)
end

local function SafeToString(value)
    if IsSecret(value) then
        return ns.L.TREE_SECRET
    end

    local valueType = type(value)
    if valueType == "string" then
        return string.format("%q", value)
    elseif valueType == "number" or valueType == "boolean" or valueType == "nil" then
        return tostring(value)
    elseif valueType == "table" then
        return ns.L.TREE_TABLE
    end

    local succeeded, text = pcall(tostring, value)
    if succeeded then
        return "<" .. valueType .. ": " .. text .. ">"
    end
    return "<" .. valueType .. ">"
end

local function SortKey(value)
    if IsSecret(value) then
        return "0:<secret>"
    end

    local valueType = type(value)
    if valueType == "number" then
        return "1:" .. string.format("%020.6f", value)
    elseif valueType == "string" then
        return "2:" .. value
    elseif valueType == "boolean" then
        return value and "3:1" or "3:0"
    end
    return "4:" .. valueType .. ":" .. SafeToString(value)
end

local function FormatKey(key)
    if type(key) == "string" and key:match("^[%a_][%w_]*$") then
        return key
    end
    return "[" .. SafeToString(key) .. "]"
end

local function MarkerNode(label, text)
    return {
        label = label,
        kind = "marker",
        value = text,
    }
end

local function HasAncestorTable(parent, value)
    while parent do
        if parent.source == value then
            return true
        end
        parent = parent.parent
    end
    return false
end

local function GetMethod(value, methodName)
    if IsSecret(value) then
        return nil
    end
    local succeeded, method = pcall(function()
        return value[methodName]
    end)
    return succeeded and type(method) == "function" and method or nil
end

local function IsUIObject(value)
    return GetMethod(value, "GetObjectType") ~= nil
end

local function CallMethod(value, methodName)
    local method = GetMethod(value, methodName)
    if not method then
        return nil
    end
    local succeeded, result = pcall(method, value)
    if not succeeded or IsSecret(result) then
        return nil
    end
    return result
end

local function CollectMethodResults(value, methodName)
    local method = GetMethod(value, methodName)
    if not method then
        return nil
    end
    local packed = { pcall(method, value) }
    if not packed[1] then
        return nil
    end
    local results = {}
    for index = 2, #packed do
        if packed[index] ~= nil and not IsSecret(packed[index]) then
            results[#results + 1] = packed[index]
        end
    end
    return results
end

local function BuildUIObjectOverview(value)
    local overview = {
        [ns.L.FRAME_LUA_TYPE] = type(value),
        [ns.L.FRAME_OBJECT_TYPE] = CallMethod(value, "GetObjectType"),
        [ns.L.FRAME_NAME] = CallMethod(value, "GetName"),
        [ns.L.FRAME_SHOWN] = CallMethod(value, "IsShown"),
        [ns.L.FRAME_WIDTH] = CallMethod(value, "GetWidth"),
        [ns.L.FRAME_HEIGHT] = CallMethod(value, "GetHeight"),
        [ns.L.FRAME_SCALE] = CallMethod(value, "GetEffectiveScale"),
        [ns.L.FRAME_LEVEL] = CallMethod(value, "GetFrameLevel"),
        [ns.L.FRAME_STRATA] = CallMethod(value, "GetFrameStrata"),
    }
    local parent = CallMethod(value, "GetParent")
    if parent ~= nil then
        overview[ns.L.FRAME_PARENT] = CallMethod(parent, "GetName") or SafeToString(parent)
    end
    return overview
end

local function BuildNode(label, value, parent)
    if IsSecret(value) then
        return MarkerNode(label, ns.L.TREE_SECRET)
    end

    local isUIObject = IsUIObject(value)
    if type(value) ~= "table" and not isUIObject then
        return {
            label = label,
            kind = type(value),
            value = SafeToString(value),
        }
    elseif HasAncestorTable(parent, value) then
        return MarkerNode(label, ns.L.TREE_CYCLE)
    end

    return {
        label = label,
        kind = "table",
        value = ns.L.TREE_TABLE,
        children = {},
        expanded = parent == nil,
        source = value,
        isUIObject = isUIObject,
        parent = parent,
        cursor = nil,
        loadedCount = 0,
        secretKeyCount = 0,
        hasMore = true,
        loaded = false,
    }
end

local function AddUIObjectNodes(node)
    if not node.isUIObject then
        return
    end

    node.children[#node.children + 1] = BuildNode(ns.L.FRAME_OVERVIEW, BuildUIObjectOverview(node.source), node)
    node.loadedCount = node.loadedCount + 1

    local children = CollectMethodResults(node.source, "GetChildren")
    if children and #children > 0 then
        node.children[#node.children + 1] = BuildNode(ns.L.FRAME_CHILDREN, children, node)
        node.loadedCount = node.loadedCount + 1
    end

    local regions = CollectMethodResults(node.source, "GetRegions")
    if regions and #regions > 0 then
        node.children[#node.children + 1] = BuildNode(ns.L.FRAME_REGIONS, regions, node)
        node.loadedCount = node.loadedCount + 1
    end
end

local function RemoveLoadMoreNode(node)
    local last = node.children[#node.children]
    if last and last.kind == "load_more" then
        node.children[#node.children] = nil
    end
end

local function UpdateTableValue(node)
    if node.hasMore then
        node.value = string.format(ns.L.TREE_TABLE_MORE, node.loadedCount)
    else
        node.value = string.format(ns.L.TREE_TABLE_COUNT, node.loadedCount)
    end
end

function ns.LoadMoreValueTreeNode(node)
    if ns.IsCombatBlocked and ns.IsCombatBlocked() then
        return false, ns.L.COMBAT_BLOCKED
    elseif type(node) ~= "table" or node.kind ~= "table"
        or (type(node.source) ~= "table" and not node.isUIObject) then
        return false
    elseif not node.hasMore then
        return true
    end

    RemoveLoadMoreNode(node)
    if not node.loaded then
        AddUIObjectNodes(node)
    end
    local entries = {}
    local cursor = node.cursor
    local scanned = 0

    while type(node.source) == "table" and #entries < PAGE_SIZE and scanned < MAX_SCANS_PER_PAGE do
        local succeeded, key, value = pcall(next, node.source, cursor)
        if not succeeded then
            node.hasMore = false
            node.children[#node.children + 1] = MarkerNode("...", ns.L.TREE_TABLE_CHANGED)
            break
        elseif key == nil then
            node.hasMore = false
            break
        end

        cursor = key
        scanned = scanned + 1
        if IsSecret(key) then
            node.secretKeyCount = node.secretKeyCount + 1
        else
            entries[#entries + 1] = { key = key, value = value }
        end
    end
    if type(node.source) ~= "table" then
        node.hasMore = false
    end

    table.sort(entries, function(left, right)
        return SortKey(left.key) < SortKey(right.key)
    end)
    for index = 1, #entries do
        local entry = entries[index]
        node.children[#node.children + 1] = BuildNode(FormatKey(entry.key), entry.value, node)
        node.loadedCount = node.loadedCount + 1
    end

    node.cursor = cursor
    node.loaded = true
    if node.hasMore then
        node.children[#node.children + 1] = {
            label = ns.L.TREE_LOAD_MORE,
            kind = "load_more",
            value = ns.L.TREE_LOAD_NEXT,
            owner = node,
        }
    elseif node.secretKeyCount > 0 then
        node.children[#node.children + 1] = MarkerNode("...", string.format(ns.L.TREE_SECRET_KEYS, node.secretKeyCount))
    end
    UpdateTableValue(node)
    return true
end

function ns.CreateValueTree(values)
    if type(values) ~= "table" or type(values.n) ~= "number" or values.n <= 0 then
        return nil
    end

    local roots = {}
    for index = 1, values.n do
        local node = BuildNode("[" .. index .. "]", values[index], nil)
        roots[#roots + 1] = node
        if node.kind == "table" then
            ns.LoadMoreValueTreeNode(node)
        end
    end

    return {
        roots = roots,
        truncated = false,
    }
end
