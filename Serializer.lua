local ADDON_NAME, ns = ...

local MAX_DEPTH = 6
local MAX_ENTRIES = 200
local MAX_OUTPUT_BYTES = 44000

local function SortKey(value)
    if issecretvalue and issecretvalue(value) then
        return "0:<secret>"
    end

    local valueType = type(value)
    if valueType == "number" then
        return "1:" .. tostring(value)
    elseif valueType == "string" then
        return "2:" .. value
    elseif valueType == "boolean" then
        return value and "3:1" or "3:0"
    end
    return "4:" .. valueType .. ":" .. tostring(value)
end

local function Append(state, text)
    if state.truncated then
        return
    end

    if state.length + #text > MAX_OUTPUT_BYTES then
        state.parts[#state.parts + 1] = "\n... <output truncated>"
        state.truncated = true
        return
    end

    state.parts[#state.parts + 1] = text
    state.length = state.length + #text
end

local function SerializeValue(value, state, depth, indent)
    if state.truncated then
        return
    end

    if issecretvalue and issecretvalue(value) then
        Append(state, "<secret>")
        return
    end

    local valueType = type(value)

    if valueType == "string" then
        Append(state, string.format("%q", value))
    elseif valueType == "number" or valueType == "boolean" or valueType == "nil" then
        Append(state, tostring(value))
    elseif valueType ~= "table" then
        Append(state, "<" .. valueType .. ": " .. tostring(value) .. ">")
    elseif state.seen[value] then
        Append(state, "<cycle>")
    elseif depth >= MAX_DEPTH then
        Append(state, "<max depth>")
    else
        state.seen[value] = true

        local keys = {}
        local secretKeyCount = 0
        for key in pairs(value) do
            if issecretvalue and issecretvalue(key) then
                secretKeyCount = secretKeyCount + 1
            else
                keys[#keys + 1] = key
            end
        end
        table.sort(keys, function(left, right)
            return SortKey(left) < SortKey(right)
        end)

        if #keys == 0 and secretKeyCount == 0 then
            Append(state, "{}")
        else
            Append(state, "{")
            local count = math.min(#keys, MAX_ENTRIES)
            for index = 1, count do
                local key = keys[index]
                Append(state, "\n" .. indent .. "  [")
                SerializeValue(key, state, depth + 1, indent .. "  ")
                Append(state, "] = ")
                SerializeValue(value[key], state, depth + 1, indent .. "  ")
                Append(state, ",")
            end
            if #keys > MAX_ENTRIES then
                Append(state, "\n" .. indent .. "  ... <" .. (#keys - MAX_ENTRIES) .. " entries omitted>")
            end
            if secretKeyCount > 0 then
                Append(state, "\n" .. indent .. "  ... <" .. secretKeyCount .. " secret keys omitted>")
            end
            Append(state, "\n" .. indent .. "}")
        end

        state.seen[value] = nil
    end
end

function ns.Serialize(value)
    local state = {
        parts = {},
        length = 0,
        seen = {},
        truncated = false,
    }
    SerializeValue(value, state, 0, "")
    return table.concat(state.parts)
end

function ns.SerializeValues(values)
    local lines = {}
    for index = 1, values.n do
        lines[index] = "[" .. index .. "] = " .. ns.Serialize(values[index])
    end
    return table.concat(lines, "\n")
end
