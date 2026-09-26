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

	local deflateLib
	if LibStub and type(LibStub.GetLibrary) == "function" then
		local okLib, lib = pcall(LibStub.GetLibrary, LibStub, "LibDeflate", true)
		if okLib and type(lib) == "table" and type(lib.CompressDeflate) == "function" then
			deflateLib = lib
		end
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
        if deflateLib and #result > 96 then
		local okCompress, compressed = pcall(deflateLib.CompressDeflate, deflateLib, result)
		if okCompress and type(compressed) == "string" and #compressed + 8 < #result then
			return string.char(31) .. compressed
		end
        end
        return result
    end,
    DigestBytes = digestBytes,
}
