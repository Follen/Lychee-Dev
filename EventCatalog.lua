local ADDON_NAME, ns = ...

local data = ns.EventCatalogData
local L = ns.L
local ALL_INDEX = 0
local catalog = {}
local Selection = {}
Selection.__index = Selection

local function NormalizeQuery(query)
    query = type(query) == "string" and query or ""
    query = query:match("^%s*(.-)%s*$"):upper()
    if query == "\229\133\168\233\131\168" or query == "\229\133\168\233\131\168\228\186\139\228\187\182" then
        return "ALL"
    end
    return query
end

function catalog.GetCount()
    return #data / 2
end

function catalog.Get(index)
    if index == ALL_INDEX then
        return "ALL", "*"
    end
    local offset = index * 2 - 1
    return data[offset], data[offset + 1]
end

function catalog.Find(eventName)
    eventName = NormalizeQuery(eventName)
    if eventName == "" then
        return nil
    elseif eventName == "ALL" then
        return ALL_INDEX
    end

    local low = 1
    local high = catalog.GetCount()
    while low <= high do
        local middle = math.floor((low + high) / 2)
        local name = data[middle * 2 - 1]
        if name == eventName then
            return middle
        elseif name < eventName then
            low = middle + 1
        else
            high = middle - 1
        end
    end
    return nil
end

function catalog.Search(query, limit, results)
    query = NormalizeQuery(query)
    results = results or {}
    for index = #results, 1, -1 do
        results[index] = nil
    end
    if query == "" then
        return results
    end

    limit = math.max(1, math.floor(tonumber(limit) or 8))
    local exactIndex = catalog.Find(query)
    if exactIndex then
        results[1] = exactIndex
        if #results >= limit then
            return results
        end
    end

    local queryLength = #query
    local count = catalog.GetCount()
    for index = 1, count do
        local name = data[index * 2 - 1]
        if name ~= query and name:sub(1, queryLength) == query then
            results[#results + 1] = index
            if #results >= limit then
                return results
            end
        end
    end

    for index = 1, count do
        local name = data[index * 2 - 1]
        local matchStart = name:find(query, 1, true)
        if matchStart and matchStart > 1 then
            results[#results + 1] = index
            if #results >= limit then
                return results
            end
        end
    end
    return results
end

function Selection:Add(catalogIndex)
    local eventName = catalog.Get(catalogIndex)
    if not eventName then
        return false, L.UNKNOWN_EVENT
    elseif self.selected[eventName] then
        return false, L.EVENT_ALREADY_SELECTED
    end

    if eventName == "ALL" then
        for index = #self.indices, 1, -1 do
            self.indices[index] = nil
        end
        for name in pairs(self.selected) do
            self.selected[name] = nil
        end
    elseif self.selected.ALL then
        self.indices[1] = nil
        self.selected.ALL = nil
    end

    self.indices[#self.indices + 1] = catalogIndex
    self.selected[eventName] = true
    return true
end

function Selection:AddName(eventName)
    local catalogIndex = catalog.Find(eventName)
    if not catalogIndex then
        return false, L.NO_DOCUMENTED_EVENT
    end
    return self:Add(catalogIndex)
end

function Selection:Remove(eventName)
    eventName = NormalizeQuery(eventName)
    if not self.selected[eventName] then
        return false
    end

    for index = 1, #self.indices do
        local selectedName = catalog.Get(self.indices[index])
        if selectedName == eventName then
            table.remove(self.indices, index)
            break
        end
    end
    self.selected[eventName] = nil
    return true
end

function Selection:Contains(eventName)
    return self.selected[NormalizeQuery(eventName)] and true or false
end

function Selection:GetCount()
    return #self.indices
end

function Selection:Get(index)
    local catalogIndex = self.indices[index]
    if not catalogIndex then
        return nil
    end
    local eventName, signature = catalog.Get(catalogIndex)
    return eventName, signature, catalogIndex
end

function Selection:GetNames()
    local names = {}
    for index = 1, #self.indices do
        names[index] = catalog.Get(self.indices[index])
    end
    return names
end

function catalog.CreateSelection(initialNames)
    local selection = setmetatable({ indices = {}, selected = {} }, Selection)
    if type(initialNames) == "table" then
        for index = 1, #initialNames do
            selection:AddName(initialNames[index])
        end
    end
    return selection
end

ns.EventCatalog = catalog
