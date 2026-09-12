local ADDON_NAME, ns = ...

-- Automation report normalization and serialization.
-- This module owns the complete report body: strict JSON encoding, size
-- budgeting and the Adler-32 content checksum. It never touches QR transport.

local REPORT_SCHEMA = "lychee.automation.result.v1"
-- Keep in sync with MAX_AUTOMATION_REPORT_BYTES in Core/Database.lua.
local MAX_REPORT_CONTENT_BYTES = 1024 * 1024
local MAX_LIFECYCLE_LOG_ENTRIES = 128
local MAX_LIFECYCLE_LOG_BYTES = 16 * 1024
local MAX_ENCODE_DEPTH = 32

local BASE64_ALPHABET = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"

local CONTROL_ESCAPES = {
    [0x08] = "\\b",
    [0x09] = "\\t",
    [0x0A] = "\\n",
    [0x0C] = "\\f",
    [0x0D] = "\\r",
}

-- --- checksums and encodings -------------------------------------------------

function ns.AutomationAdler32(text)
    local a, b = 1, 0
    local length = #text
    local index = 1
    -- 5552 is the largest n so that 255*n*(n+1)/2 + (n+1)*(65520) stays below 2^32.
    while index <= length do
        local chunkEnd = math.min(index + 5551, length)
        for position = index, chunkEnd do
            a = a + string.byte(text, position)
            b = b + a
        end
        a = a % 65521
        b = b % 65521
        index = chunkEnd + 1
    end
    return string.format("%08x", b * 65536 + a)
end

local function EncodeBase64(text)
    local parts = {}
    local length = #text
    local index = 1
    while index <= length do
        local b1 = string.byte(text, index)
        local b2 = string.byte(text, index + 1)
        local b3 = string.byte(text, index + 2)
        local group = b1 * 65536 + (b2 or 0) * 256 + (b3 or 0)
        local chars = {
            BASE64_ALPHABET:sub(math.floor(group / 262144) % 64 + 1, math.floor(group / 262144) % 64 + 1),
            BASE64_ALPHABET:sub(math.floor(group / 4096) % 64 + 1, math.floor(group / 4096) % 64 + 1),
            b2 and BASE64_ALPHABET:sub(math.floor(group / 64) % 64 + 1, math.floor(group / 64) % 64 + 1) or "=",
            b3 and BASE64_ALPHABET:sub(group % 64 + 1, group % 64 + 1) or "=",
        }
        parts[#parts + 1] = table.concat(chars)
        index = index + 3
    end
    return table.concat(parts)
end

local function IsUtf8Continuation(byte)
    return byte ~= nil and byte >= 0x80 and byte <= 0xBF
end

-- Strict UTF-8: every sequence must carry exactly as many continuation bytes
-- as its leading byte announces. The leading byte ranges already exclude
-- C0/C1 (overlong) and F5..FF (out of range); the second byte must then rule
-- out overlong forms (E0 80..9F, F0 80..8F), surrogates (ED A0..BF) and code
-- points above U+10FFFF (F4 90..BF).
local function ValidateUtf8(text)
    local index = 1
    local length = #text
    while index <= length do
        local byte = string.byte(text, index)
        local count
        if byte < 0x80 then
            count = 1
        elseif byte >= 0xC2 and byte <= 0xDF then
            count = 2
        elseif byte >= 0xE0 and byte <= 0xEF then
            count = 3
        elseif byte >= 0xF0 and byte <= 0xF4 then
            count = 4
        else
            return false
        end
        if index + count - 1 > length then
            return false
        end
        local second, third, fourth = string.byte(text, index + 1, index + count - 1)
        if count == 2 then
            if not IsUtf8Continuation(second) then
                return false
            end
        elseif count == 3 then
            if not IsUtf8Continuation(second) or not IsUtf8Continuation(third) then
                return false
            end
            if byte == 0xE0 and second < 0xA0 then return false end
            if byte == 0xED and second > 0x9F then return false end
        elseif count == 4 then
            if not IsUtf8Continuation(second) or not IsUtf8Continuation(third)
                or not IsUtf8Continuation(fourth) then
                return false
            end
            if byte == 0xF0 and second < 0x90 then return false end
            if byte == 0xF4 and second > 0x8F then return false end
        end
        index = index + count
    end
    return true
end

-- --- strict JSON encoding ----------------------------------------------------

local EncodeState = {}
EncodeState.__index = EncodeState

function EncodeState:new()
    return setmetatable({
        parts = {},
        bytes = 0,
        incomplete = false,
        incompleteReasons = {},
        overflow = false,
    }, self)
end

function EncodeState:MarkIncomplete(reason)
    self.incomplete = true
    if #self.incompleteReasons < 64 then
        self.incompleteReasons[#self.incompleteReasons + 1] = reason
    end
end

function EncodeState:Append(text)
    if self.overflow then
        return
    end
    if self.bytes + #text > MAX_REPORT_CONTENT_BYTES then
        self.overflow = true
        return
    end
    self.parts[#self.parts + 1] = text
    self.bytes = self.bytes + #text
end

local function EncodeJSONString(state, text)
    if not ValidateUtf8(text) then
        -- Binary payloads travel as explicitly marked Base64, never transcoded.
        state:Append('{"$base64":true,"data":"')
        state:Append(EncodeBase64(text))
        state:Append('"}')
        state:MarkIncomplete("binary_string")
        return
    end

    state:Append('"')
    local index = 1
    local length = #text
    while index <= length do
        local byte = string.byte(text, index)
        local escape = CONTROL_ESCAPES[byte]
        if escape then
            state:Append(escape)
            index = index + 1
        elseif byte == 0x22 then
            state:Append('\\"')
            index = index + 1
        elseif byte == 0x5C then
            state:Append("\\\\")
            index = index + 1
        elseif byte < 0x20 then
            state:Append(string.format("\\u%04x", byte))
            index = index + 1
        else
            local start = index
            repeat
                index = index + 1
                byte = index <= length and string.byte(text, index) or 0
            until byte < 0x20 or byte == 0x22 or byte == 0x5C
            state:Append(text:sub(start, index - 1))
        end
        if state.overflow then
            return
        end
    end
    state:Append('"')
end

local function EncodeJSONNumber(state, value)
    if value ~= value or value == math.huge or value == -math.huge then
        state:Append("null")
        state:MarkIncomplete("non_finite_number")
        return
    end
    state:Append(tostring(value))
end

local function IsPlainArray(value)
    -- pairs() order is undefined, so array detection must not depend on it.
    local maximum = 0
    for key, _ in pairs(value) do
        if type(key) ~= "number" or key % 1 ~= 0 or key < 1 then
            return false
        end
        if key > maximum then
            maximum = key
        end
    end
    for index = 1, maximum do
        if value[index] == nil then
            return false
        end
    end
    return true, maximum
end

local EncodeJSONValue

local function EncodeJSONTable(state, value, depth, seen)
    if depth > MAX_ENCODE_DEPTH then
        state:Append('"<max depth>"')
        state:MarkIncomplete("max_depth")
        return
    end
    if seen[value] then
        state:Append('"<cycle>"')
        state:MarkIncomplete("cycle")
        return
    end
    seen[value] = true

    local isArray, count = IsPlainArray(value)
    if isArray and count > 0 then
        state:Append("[")
        for index = 1, count do
            if index > 1 then
                state:Append(",")
            end
            EncodeJSONValue(state, value[index], depth + 1, seen)
            if state.overflow then
                seen[value] = nil
                return
            end
        end
        state:Append("]")
    else
        state:Append("{")
        local first = true
        for key, child in pairs(value) do
            if not first then
                state:Append(",")
            end
            first = false
            local keyType = type(key)
            if keyType == "number" then
                EncodeJSONString(state, tostring(key))
                state:Append(":")
            elseif keyType == "string" then
                EncodeJSONString(state, key)
                state:Append(":")
            elseif keyType == "boolean" then
                EncodeJSONString(state, tostring(key))
                state:Append(":")
            else
                EncodeJSONString(state, "<unsupported key: " .. keyType .. ">")
                state:Append(":")
                state:MarkIncomplete("unsupported_key")
            end
            EncodeJSONValue(state, child, depth + 1, seen)
            if state.overflow then
                seen[value] = nil
                return
            end
        end
        state:Append("}")
    end
    seen[value] = nil
end

EncodeJSONValue = function(state, value, depth, seen)
    local valueType = type(value)
    if issecretvalue and issecretvalue(value) then
        state:Append('"<secret>"')
        state:MarkIncomplete("secret_value")
    elseif valueType == "nil" then
        state:Append("null")
    elseif valueType == "boolean" then
        state:Append(value and "true" or "false")
    elseif valueType == "number" then
        EncodeJSONNumber(state, value)
    elseif valueType == "string" then
        EncodeJSONString(state, value)
    elseif valueType == "table" then
        EncodeJSONTable(state, value, depth, seen)
    else
        state:Append('"' .. valueType .. '"')
        state:MarkIncomplete("unsupported_type")
    end
end

-- Encode one value as strict normalized JSON.
-- Returns json text (or nil on overflow), complete flag, reasons array.
local function EncodeJSON(value)
    local state = EncodeState:new()
    EncodeJSONValue(state, value, 0, {})
    if state.overflow then
        return nil, false, { "output_limit" }
    end
    return table.concat(state.parts), not state.incomplete, state.incompleteReasons
end

-- --- lifecycle log -----------------------------------------------------------

local function ClampLifecycleLog(log)
    if type(log) ~= "table" then
        return {}, false
    end
    local entries = {}
    for index = 1, math.min(#log, MAX_LIFECYCLE_LOG_ENTRIES) do
        entries[#entries + 1] = log[index]
    end
    local limited = #log > MAX_LIFECYCLE_LOG_ENTRIES
    local bytes = 0
    for index = 1, #entries do
        local message = entries[index]
        bytes = bytes + (type(message) == "table" and #(message.message or "") or #tostring(message)) + 24
    end
    while bytes > MAX_LIFECYCLE_LOG_BYTES and #entries > 1 do
        local dropped = table.remove(entries, 1)
        bytes = bytes - (#(type(dropped) == "table" and dropped.message or tostring(dropped)) + 24)
        limited = true
    end
    return entries, limited
end

-- --- report building ---------------------------------------------------------

-- Build the complete automation report body.
-- Returns: content(string), metadata(table), errorCode(string|nil)
-- metadata: { complete = bool, contentChecksum = "xxxxxxxx" }
-- On overflow the caller receives nil and a stable errorCode.
function ns.AutomationReportBuild(spec)
    spec = type(spec) == "table" and spec or {}

    local log, logLimited = ClampLifecycleLog(spec.lifecycleLog)
    local report = {
        schema = REPORT_SCHEMA,
        taskId = spec.taskId,
        executionId = spec.executionId,
        revision = spec.revision,
        kind = spec.kind,
        status = spec.status,
        startedAt = spec.startedAt,
        finishedAt = spec.finishedAt,
        requestType = spec.requestType or "task",
        source = spec.source,
        params = spec.params,
        stdout = spec.stdout,
        outputTruncated = spec.outputTruncated and true or false,
        returns = spec.returns,
        error = spec.error,
        lifecycleLog = log,
        lifecycleLogLimited = logLimited,
        bugSnapshot = spec.bugSnapshot,
        environment = spec.environment,
        complete = spec.complete and true or false,
        incompleteReasons = spec.incompleteReasons,
    }

    local json, encodeComplete, reasons = EncodeJSON(report)
    if not json then
        return nil, nil, reasons and reasons[1] or "output_limit"
    end
    if spec.complete and not encodeComplete then
        -- Encoding limits (cycles, secrets, unsupported types) make the report
        -- incomplete; fold the encode reasons in and re-encode once.
        local allReasons = {}
        local specReasons = type(spec.incompleteReasons) == "table" and spec.incompleteReasons or {}
        for index = 1, #specReasons do
            allReasons[#allReasons + 1] = specReasons[index]
        end
        for index = 1, #reasons do
            allReasons[#allReasons + 1] = "encode:" .. reasons[index]
        end
        report.incompleteReasons = allReasons
        report.complete = false
        json, encodeComplete, reasons = EncodeJSON(report)
        if not json then
            return nil, nil, "output_limit"
        end
    end

    return json, {
        complete = report.complete and true or false,
        contentChecksum = ns.AutomationAdler32(json),
    }, nil
end

ns.AutomationReport = {
    Encode = EncodeJSON,
    Adler32 = ns.AutomationAdler32,
    MAX_REPORT_CONTENT_BYTES = MAX_REPORT_CONTENT_BYTES,
}
