local ADDON_NAME, ns = ...

local PAGE_SIZE = 200
local MAX_SCANS_PER_PAGE = PAGE_SIZE * 4
local STORED_MAX_DEPTH = 8
local STORED_MAX_NODES = 4000
local STORED_MAX_ENTRIES = 200
local STORED_MAX_VALUE_BYTES = 512

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
            source = value,
            exportable = true,
            parent = parent,
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
        exportable = true,
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

local function TrimStoredText(value)
    value = tostring(value or "")
    if #value <= STORED_MAX_VALUE_BYTES then
        return value
    end
    return value:sub(1, STORED_MAX_VALUE_BYTES) .. "..."
end

local function BuildStoredNode(label, value, context, depth)
    if context.count >= STORED_MAX_NODES then
        context.truncated = true
        return nil
    end
    context.count = context.count + 1

    if IsSecret(value) then
        return MarkerNode(label, ns.L.TREE_SECRET)
    end

    local isUIObject = IsUIObject(value)
    if type(value) ~= "table" and not isUIObject then
        return {
            label = label,
            kind = type(value),
            value = TrimStoredText(SafeToString(value)),
        }
    elseif context.ancestors[value] then
        return MarkerNode(label, ns.L.TREE_CYCLE)
    end

    local node = {
        label = label,
        kind = "table",
        value = ns.L.TREE_TABLE,
        children = {},
        expanded = depth == 0,
        loaded = true,
        hasMore = false,
    }
    if depth >= STORED_MAX_DEPTH then
        context.truncated = true
        node.children[1] = MarkerNode("...", ns.L.TREE_STORED_TRUNCATED)
        node.value = string.format(ns.L.TREE_TABLE_COUNT, 0)
        return node
    end

    context.ancestors[value] = true
    local entries = {}
    if isUIObject then
        entries[1] = { key = ns.L.FRAME_OVERVIEW, value = BuildUIObjectOverview(value) }
    end
    if type(value) == "table" then
        local cursor
        while #entries < STORED_MAX_ENTRIES and context.count + #entries < STORED_MAX_NODES do
            local succeeded, key, childValue = pcall(next, value, cursor)
            if not succeeded or key == nil then
                break
            end
            cursor = key
            if not IsSecret(key) then
                entries[#entries + 1] = { key = key, value = childValue }
            end
        end
        if cursor ~= nil then
            local succeeded, nextKey = pcall(next, value, cursor)
            if succeeded and nextKey ~= nil then
                context.truncated = true
            end
        end
    end

    table.sort(entries, function(left, right)
        return SortKey(left.key) < SortKey(right.key)
    end)
    for index = 1, #entries do
        local entry = entries[index]
        local child = BuildStoredNode(FormatKey(entry.key), entry.value, context, depth + 1)
        if not child then
            context.truncated = true
            break
        end
        node.children[#node.children + 1] = child
    end
    context.ancestors[value] = nil

    if context.truncated and #node.children < STORED_MAX_ENTRIES
        and context.count < STORED_MAX_NODES then
        node.children[#node.children + 1] = MarkerNode("...", ns.L.TREE_STORED_TRUNCATED)
    end
    node.value = string.format(ns.L.TREE_TABLE_COUNT, #node.children)
    return node
end

function ns.CreateStoredValueTree(values)
    if type(values) ~= "table" or type(values.n) ~= "number" or values.n <= 0 then
        return nil
    end

    local context = {
        count = 0,
        truncated = false,
        ancestors = {},
    }
    local roots = {}
    for index = 1, values.n do
        local node = BuildStoredNode("[" .. index .. "]", values[index], context, 0)
        if not node then
            break
        end
        roots[#roots + 1] = node
    end
    if context.truncated and #roots == 0 then
        roots[1] = MarkerNode("...", ns.L.TREE_STORED_TRUNCATED)
    end
    return {
        roots = roots,
        truncated = context.truncated,
    }
end

local function SkipSerializedWhitespace(text, position)
    while position <= #text do
        local character = text:sub(position, position)
        if character ~= " " and character ~= "\t" and character ~= "\r" and character ~= "\n" then
            break
        end
        position = position + 1
    end
    return position
end

local function TrimSerializedToken(text)
    return text:match("^%s*(.-)%s*$") or ""
end

local function ScanSerializedQuoted(text, position)
    local quote = text:sub(position, position)
    local cursor = position + 1
    while cursor <= #text do
        local character = text:sub(cursor, cursor)
        if character == "\\" then
            cursor = cursor + 2
        elseif character == quote then
            return text:sub(position, cursor), cursor + 1
        else
            cursor = cursor + 1
        end
    end
    return nil
end

local function ScanSerializedKey(text, position)
    local startPosition = position
    local braceDepth = 0
    local bracketDepth = 0
    while position <= #text do
        local character = text:sub(position, position)
        if character == "\"" or character == "'" then
            local _, nextPosition = ScanSerializedQuoted(text, position)
            if not nextPosition then
                return nil
            end
            position = nextPosition
        elseif character == "{" then
            braceDepth = braceDepth + 1
            position = position + 1
        elseif character == "}" then
            braceDepth = math.max(0, braceDepth - 1)
            position = position + 1
        elseif character == "[" then
            bracketDepth = bracketDepth + 1
            position = position + 1
        elseif character == "]" then
            if braceDepth == 0 and bracketDepth == 0 then
                return TrimSerializedToken(text:sub(startPosition, position - 1)), position + 1
            end
            bracketDepth = math.max(0, bracketDepth - 1)
            position = position + 1
        else
            position = position + 1
        end
    end
    return nil
end

local function FormatSerializedKey(token)
    local identifier = token:match('^"([_%a][_%w]*)"$') or token:match("^'([_%a][_%w]*)'$")
    if identifier then
        return identifier
    end
    return "[" .. token .. "]"
end

local function CreateSerializedScalarNode(label, token)
    local kind
    if token == "nil" then
        kind = "nil"
    elseif token == "true" or token == "false" then
        kind = "boolean"
    elseif tonumber(token) ~= nil then
        kind = "number"
    elseif token:sub(1, 1) == "<" then
        kind = "marker"
    else
        kind = "string"
    end
    return {
        label = label,
        kind = kind,
        value = TrimStoredText(token),
    }
end

local ParseSerializedNode
ParseSerializedNode = function(text, position, label, context, depth)
    if context.count >= STORED_MAX_NODES then
        return nil
    end
    context.count = context.count + 1
    position = SkipSerializedWhitespace(text, position)
    local character = text:sub(position, position)

    if character == "\"" or character == "'" then
        local token, nextPosition = ScanSerializedQuoted(text, position)
        if not token then
            return nil
        end
        return CreateSerializedScalarNode(label, token), nextPosition
    elseif character ~= "{" then
        local startPosition = position
        while position <= #text do
            character = text:sub(position, position)
            if character == "," or character == "\r" or character == "\n" or character == "}" then
                break
            end
            position = position + 1
        end
        local token = TrimSerializedToken(text:sub(startPosition, position - 1))
        if token == "" then
            return nil
        end
        return CreateSerializedScalarNode(label, token), position
    end

    local node = {
        label = label,
        kind = "table",
        value = ns.L.TREE_TABLE,
        children = {},
        expanded = depth == 0,
        loaded = true,
        hasMore = false,
    }
    position = position + 1
    while position <= #text do
        position = SkipSerializedWhitespace(text, position)
        character = text:sub(position, position)
        if character == "}" then
            node.value = string.format(ns.L.TREE_TABLE_COUNT, #node.children)
            return node, position + 1
        elseif text:sub(position, position + 2) == "..." then
            local lineEnd = text:find("[\r\n]", position)
            local marker = TrimSerializedToken(text:sub(position, (lineEnd or (#text + 1)) - 1))
            node.children[#node.children + 1] = MarkerNode("...", marker)
            position = lineEnd or (#text + 1)
        elseif character ~= "[" then
            return nil
        else
            local keyToken, nextPosition = ScanSerializedKey(text, position + 1)
            if not keyToken then
                return nil
            end
            position = SkipSerializedWhitespace(text, nextPosition)
            if text:sub(position, position) ~= "=" then
                return nil
            end
            local child
            child, position = ParseSerializedNode(text, position + 1,
                FormatSerializedKey(keyToken), context, depth + 1)
            if not child then
                return nil
            end
            node.children[#node.children + 1] = child
            position = SkipSerializedWhitespace(text, position)
            if text:sub(position, position) == "," then
                position = position + 1
            end
        end
    end
    return nil
end

local function ParseSerializedRootHeader(text, position, expectedIndex)
    if text:sub(position, position) ~= "[" then
        return nil
    end
    local closing = text:find("]", position + 1, true)
    if not closing or tonumber(text:sub(position + 1, closing - 1)) ~= expectedIndex then
        return nil
    end
    position = SkipSerializedWhitespace(text, closing + 1)
    if text:sub(position, position) ~= "=" then
        return nil
    end
    return position + 1
end

function ns.CreateStoredTreeFromSerialized(text)
    if type(text) ~= "string" or text == "" then
        return nil
    end

    local position = text:find("^%[1%]%s*=")
    if not position then
        local lineStart = text:find("\n%[1%]%s*=")
        position = lineStart and lineStart + 1 or nil
    end
    if not position then
        return nil
    end

    local context = { count = 0 }
    local roots = {}
    local rootIndex = 1
    while position and position <= #text do
        position = ParseSerializedRootHeader(text, position, rootIndex)
        if not position then
            break
        end
        local node
        node, position = ParseSerializedNode(text, position, "[" .. rootIndex .. "]", context, 0)
        if not node then
            return nil
        end
        roots[#roots + 1] = node
        rootIndex = rootIndex + 1
        position = SkipSerializedWhitespace(text, position)
        if text:sub(position, position) ~= "[" then
            break
        end
    end

    if #roots == 0 then
        return nil
    end
    return {
        roots = roots,
        truncated = false,
    }
end
