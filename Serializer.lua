local ADDON_NAME, ns = ...

local DEFAULT_MAX_DEPTH = 6
local DEFAULT_MAX_ENTRIES = 200
local DEFAULT_MAX_OUTPUT_BYTES = 44000
local STREAM_CHUNK_BYTES = 44000
local STREAM_YIELD_BYTES = 4096
local STREAM_MAX_DEPTH = 32
local STREAM_MAX_ENTRIES = 10000
local STREAM_MAX_OUTPUT_BYTES = 16 * 1024 * 1024

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

    if state.length + #text > state.maxOutputBytes then
        state.parts[#state.parts + 1] = "\n... <output truncated>"
        state.truncated = true
        state.incomplete = true
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
    elseif depth >= state.maxDepth then
        Append(state, "<max depth>")
        state.incomplete = true
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
            local count = math.min(#keys, state.maxEntries)
            for index = 1, count do
                local key = keys[index]
                Append(state, "\n" .. indent .. "  [")
                SerializeValue(key, state, depth + 1, indent .. "  ")
                Append(state, "] = ")
                SerializeValue(value[key], state, depth + 1, indent .. "  ")
                Append(state, ",")
            end
            if #keys > state.maxEntries then
                Append(state, "\n" .. indent .. "  ... <" .. (#keys - state.maxEntries) .. " entries omitted>")
                state.incomplete = true
            end
            if secretKeyCount > 0 then
                Append(state, "\n" .. indent .. "  ... <" .. secretKeyCount .. " secret keys omitted>")
            end
            Append(state, "\n" .. indent .. "}")
        end

        state.seen[value] = nil
    end
end

local function SerializeWithLimits(value, maxDepth, maxEntries, maxOutputBytes)
    local state = {
        parts = {},
        length = 0,
        seen = {},
        truncated = false,
        incomplete = false,
        maxDepth = maxDepth,
        maxEntries = maxEntries,
        maxOutputBytes = maxOutputBytes,
    }
    SerializeValue(value, state, 0, "")
    return table.concat(state.parts), state.incomplete
end

function ns.Serialize(value)
    return SerializeWithLimits(value, DEFAULT_MAX_DEPTH, DEFAULT_MAX_ENTRIES, DEFAULT_MAX_OUTPUT_BYTES)
end

function ns.SerializeValues(values)
    local lines = {}
    for index = 1, values.n do
        lines[index] = "[" .. index .. "] = " .. ns.Serialize(values[index])
    end
    return table.concat(lines, "\n")
end

local function CreateStreamCoroutine(value, streamState)
    return coroutine.create(function()
        local parts = {}
        local length = 0
        local emitted = 0
        local seen = {}
        local stopped = false

        local function Flush()
            if length == 0 then
                return
            end
            local text = table.concat(parts)
            parts = {}
            length = 0
            emitted = emitted + #text
            coroutine.yield(text)
        end

        local function Emit(text)
            if stopped then
                return false
            end

            local remaining = STREAM_MAX_OUTPUT_BYTES - emitted - length
            if #text > remaining then
                local marker = "\n... <output limit reached>"
                local available = math.max(0, remaining - #marker)
                if available > 0 then
                    parts[#parts + 1] = text:sub(1, available)
                    length = length + available
                end
                if remaining >= #marker then
                    parts[#parts + 1] = marker
                    length = length + #marker
                end
                streamState.limited = true
                stopped = true
                return false
            end

            parts[#parts + 1] = text
            length = length + #text
            if length >= STREAM_YIELD_BYTES then
                Flush()
            end
            return true
        end

        local SerializeStreamValue
        SerializeStreamValue = function(current, depth, indent)
            if stopped then
                return
            elseif issecretvalue and issecretvalue(current) then
                Emit("<secret>")
                return
            end

            local valueType = type(current)
            if valueType == "string" then
                Emit(string.format("%q", current))
            elseif valueType == "number" or valueType == "boolean" or valueType == "nil" then
                Emit(tostring(current))
            elseif valueType ~= "table" then
                Emit("<" .. valueType .. ": " .. tostring(current) .. ">")
            elseif seen[current] then
                Emit("<cycle>")
            elseif depth >= STREAM_MAX_DEPTH then
                Emit("<max depth>")
                streamState.limited = true
            else
                seen[current] = true
                local keys = {}
                local secretKeyCount = 0
                for key in pairs(current) do
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
                    Emit("{}")
                else
                    Emit("{")
                    local count = math.min(#keys, STREAM_MAX_ENTRIES)
                    for index = 1, count do
                        if stopped then
                            break
                        end
                        local key = keys[index]
                        Emit("\n" .. indent .. "  [")
                        SerializeStreamValue(key, depth + 1, indent .. "  ")
                        Emit("] = ")
                        SerializeStreamValue(current[key], depth + 1, indent .. "  ")
                        Emit(",")
                    end
                    if not stopped and #keys > STREAM_MAX_ENTRIES then
                        Emit("\n" .. indent .. "  ... <"
                            .. (#keys - STREAM_MAX_ENTRIES) .. " entries omitted>")
                        streamState.limited = true
                    end
                    if not stopped and secretKeyCount > 0 then
                        Emit("\n" .. indent .. "  ... <"
                            .. secretKeyCount .. " secret keys omitted>")
                    end
                    if not stopped then
                        Emit("\n" .. indent .. "}")
                    end
                end
                seen[current] = nil
            end
        end

        SerializeStreamValue(value, 0, "")
        Flush()
    end)
end

function ns.CreateSerializationStream(value)
    local state = { limited = false }
    local thread = CreateStreamCoroutine(value, state)
    local stream = {
        finished = false,
        pending = "",
        state = state,
    }

    function stream:ReadChunk(maxBytes)
        maxBytes = math.max(1, math.floor(tonumber(maxBytes) or STREAM_CHUNK_BYTES))
        local output = {}
        local outputLength = 0

        while outputLength < maxBytes and (self.pending ~= "" or not self.finished) do
            if self.pending == "" then
                local succeeded, segment = coroutine.resume(thread)
                if not succeeded then
                    self.finished = true
                    self.state.limited = true
                    self.errorMessage = tostring(segment)
                    break
                elseif coroutine.status(thread) == "dead" then
                    self.finished = true
                    break
                end
                self.pending = segment or ""
            end

            if self.pending ~= "" then
                local take = math.min(#self.pending, maxBytes - outputLength)
                output[#output + 1] = self.pending:sub(1, take)
                self.pending = self.pending:sub(take + 1)
                outputLength = outputLength + take
            end
        end

        return table.concat(output), self.finished and self.pending == ""
    end

    function stream:IsFinished()
        return self.finished and self.pending == ""
    end

    function stream:WasLimited()
        return self.state.limited
    end

    return stream
end
