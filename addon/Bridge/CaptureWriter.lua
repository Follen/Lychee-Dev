local ADDON_NAME, ns = ...

local MAX_REPORT_BYTES = 512 * 1024
local MAX_DEPTH = 32
local MAX_ENTRIES = 32768
local escapes = {
    ["\""] = "\\\"", ["\\"] = "\\\\", ["\b"] = "\\b", ["\f"] = "\\f",
    ["\n"] = "\\n", ["\r"] = "\\r", ["\t"] = "\\t",
}

local function rejectRestricted(value)
    if issecretvalue and issecretvalue(value) then
        error("report_secret_value", 0)
    end
end

local function escapeControl(character)
    return escapes[character] or string.format("\\u%04x", string.byte(character))
end

local function isUTF8(text)
    local offset, size = 1, #text
    while offset <= size do
        local first = string.byte(text, offset)
        local count, minimum, maximum = 0, 128, 191
        if first < 128 then
            count = 0
        elseif first >= 194 and first <= 223 then
            count = 1
        elseif first >= 224 and first <= 239 then
            count = 2
            if first == 224 then minimum = 160 end
            if first == 237 then maximum = 159 end
        elseif first >= 240 and first <= 244 then
            count = 3
            if first == 240 then minimum = 144 end
            if first == 244 then maximum = 143 end
        else
            return false
        end
        if offset + count > size then return false end
        for index = 1, count do
            local byte = string.byte(text, offset + index)
            if index == 1 then
                if byte < minimum or byte > maximum then return false end
            elseif byte < 128 or byte > 191 then
                return false
            end
        end
        offset = offset + count + 1
    end
    return true
end

local function encodeDocument(value, limit)
    local parts, seen, used, entries = {}, {}, 0, 0
    local function append(text)
        if used + #text > limit then error("report_byte_limit", 0) end
        used = used + #text
        parts[#parts + 1] = text
    end
    local function quoted(text)
        if #text > limit - used then error("report_byte_limit", 0) end
        if not isUTF8(text) then error("report_invalid_utf8", 0) end
        append("\"" .. string.gsub(text, '[%z\1-\31\\"]', escapeControl) .. "\"")
    end
    local encode
    encode = function(child, depth)
        rejectRestricted(child)
        if depth > MAX_DEPTH then error("report_depth_limit", 0) end
        local kind = type(child)
        if kind == "nil" then append("null")
        elseif kind == "boolean" then append(child and "true" or "false")
        elseif kind == "string" then quoted(child)
        elseif kind == "number" then
            if child ~= child or child == math.huge or child == -math.huge then
                error("report_nonfinite_number", 0)
            end
            if child % 1 == 0 and math.abs(child) <= 9007199254740991 then
                append(string.format("%.0f", child))
            else append(tostring(child)) end
        elseif kind == "table" then
            if getmetatable(child) then error("report_metatable", 0) end
            if seen[child] then error("report_cycle", 0) end
            seen[child] = true
            local keys, count, highest, array = {}, 0, 0, true
            for key in pairs(child) do
                rejectRestricted(key)
                count = count + 1
                entries = entries + 1
                if entries > MAX_ENTRIES then error("report_entry_limit", 0) end
                keys[count] = key
                if type(key) ~= "number" or key < 1 or key % 1 ~= 0 then
                    array = false
                elseif key > highest then highest = key end
            end
            if array and count > 0 and highest == count then
                append("[")
                for index = 1, count do
                    if index > 1 then append(",") end
                    encode(child[index], depth + 1)
                end
                append("]")
            else
                for _, key in ipairs(keys) do
                    if type(key) ~= "string" then error("report_nonstring_key", 0) end
                end
                table.sort(keys)
                append("{")
                for index, key in ipairs(keys) do
                    if index > 1 then append(",") end
                    quoted(key)
                    append(":")
                    encode(child[key], depth + 1)
                end
                append("}")
            end
            seen[child] = nil
        else error("report_unsupported_value", 0) end
    end
    encode(value, 0)
    return table.concat(parts)
end

local function digestBytes(text)
    rejectRestricted(text)
    if type(text) ~= "string" then return nil, "checksum_requires_bytes" end
    if #text > MAX_REPORT_BYTES then return nil, "checksum_byte_limit" end
    local low, high, offset = 1, 0, 1
    while offset <= #text do
        local last = math.min(offset + 5551, #text)
        for index = offset, last do
            low = low + string.byte(text, index)
            high = high + low
        end
        low, high = low % 65521, high % 65521
        offset = last + 1
    end
    return string.format("%04x%04x", high, low)
end

-- Every signal is drawn into a QR symbol, so its size decides whether the host
-- can sample the module grid at all. A retained session already proved the actor
-- and the build, so repeating them in each receipt only spends modules: the host
-- fills them back from its baseline and still refuses a value that contradicts
-- it. Identity, reset and cleared markers are the exception, because they
-- establish or re-establish the actor.
--
-- The session nonce is different and must NOT be dropped blindly: the ready and
-- loaded receipts are what establish it, so the host has nothing to fill from
-- until one of them has been read. Only the receipts that echo an
-- already-established session may omit it.
local WIRE_FIELDS = {
    schema = true, kind = true, sessionNonce = true, requestId = true,
    release = true, product = true, build = true,
    reloadNonce = true, cleanupNonce = true, probeNonce = true,
    actorState = true, inputReason = true, sequence = true, runtimeEpoch = true,
    inputReady = true, codeBytes = true, codeAdler32 = true,
    reportBytes = true, reportAdler32 = true,
}
local WIRE_MARKER_FIELDS = { character = true, realm = true, guid = true }
-- A standalone ready can establish a read-only live bind without a preceding
-- identity probe. Keep its actor on the wire; paired readiness is compacted
-- only after the host already owns a request-scoped session.
local WIRE_MARKERS = { identity = true, reset = true, cleared = true, ready = true }
-- Kinds whose session nonce is redundant because the session provably exists on
-- the host already: they answer a request the host itself created for that
-- nonce, and the host compares the nonce it filled in.
local WIRE_ECHOES_SESSION = { reported = true, acknowledged = true, cancelled = true }

-- wireDocument projects a signal onto its transmitted form. Fields outside the
-- profile are dropped rather than rejected: the projection is what the host
-- compares against the archived receipt, so an unlisted field simply stops
-- matching and never lets a caller believe a value was carried when it was not.
local function wireDocument(value)
    rejectRestricted(value)
    if type(value) ~= "table" or getmetatable(value) ~= nil then
        return value
    end
    rejectRestricted(value.schema)
    if value.schema ~= "lycheedev.signal.v1" then return value end
    rejectRestricted(value.kind)
    local marker = WIRE_MARKERS[value.kind] == true
    local echoes = WIRE_ECHOES_SESSION[value.kind] == true
    local filtered = {}
    for key, child in pairs(value) do
        rejectRestricted(key)
        local carried = WIRE_FIELDS[key] or (marker and WIRE_MARKER_FIELDS[key])
        if carried and not (key == "sessionNonce" and echoes) then
            filtered[key] = child
        end
    end
    return filtered
end

ns.CaptureWriter = {
    Encode = function(value, limit)
        if issecretvalue and issecretvalue(limit) then return nil, "report_invalid_budget" end
        limit = limit or MAX_REPORT_BYTES
        if type(limit) ~= "number" or limit < 1 or limit > MAX_REPORT_BYTES or limit % 1 ~= 0 then
            return nil, "report_invalid_budget"
        end
        local ok, result = pcall(encodeDocument, value, limit)
        if not ok then return nil, result end
        -- Every encoded value is the exact UTF-8 JSON text that is both drawn
        -- into the QR symbol and stored in SavedVariables. Transport
        -- compression is not applied here: a raw DEFLATE stream is not valid
        -- UTF-8, and SavedVariables must stay valid UTF-8 for the host to read
        -- any of the toolkit state at all.
        return result
    end,
    -- EncodeSignal is the only entry point that produces a machine-readable
    -- receipt. Encode stays byte-exact for whatever a caller passes. A rejected
    -- projection reports the exact offending field so a caller can tell an
    -- unrecognized field from a malformed value.
    EncodeSignal = function(value, limit)
        local ok, projected = pcall(wireDocument, value)
        if not ok then
            if type(projected) == "string" and #projected > 0 and #projected <= 128 then
                return nil, projected
            end
            return nil, "report_invalid_document"
        end
        local encoded, failure = ns.CaptureWriter.Encode(projected, limit)
        return encoded, failure, encoded and projected or nil
    end,
    -- Optical-only envelope: the stored receipt stays byte-exact. Readiness is
    -- a fresh, independently sequenced observation of the same session. The
    -- host expands this tuple into a second signal, never into report content.
    EncodeReceiptPair = function(receipt, ready)
        local ok, suffix = pcall(function()
            rejectRestricted(receipt); rejectRestricted(ready)
            if type(receipt) ~= "string" or #receipt > 2048 or not isUTF8(receipt)
                or type(ready) ~= "table" or getmetatable(ready) then error("receipt_invalid_pair", 0) end
            -- Traverse before branching, including any secret-valued fields.
            encodeDocument(ready, 4096)
            if ready.kind ~= "ready" or ready.inputReady ~= true or ready.requestId ~= ""
                or type(ready.sessionNonce) ~= "string" or #ready.sessionNonce ~= 32
                or not string.match(ready.sessionNonce, "^[0-9a-f]+$")
                or type(ready.sequence) ~= "number" or type(ready.runtimeEpoch) ~= "number" then
                error("receipt_invalid_pair", 0)
            end
            return encodeDocument({ready.sessionNonce, ready.sequence, ready.runtimeEpoch}, 256)
        end)
        if not ok then return nil, "receipt_invalid_pair" end
        local result = '{"schema":"lycheedev.receipt.v1","receipt":' .. receipt .. ',"ready":' .. suffix .. '}'
        if #result > 4096 then return nil, "receipt_invalid_pair" end
        return result
    end,
    DigestBytes = digestBytes,
}
