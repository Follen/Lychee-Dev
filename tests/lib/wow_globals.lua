-- WoW global aliases missing from a plain Lua 5.1 interpreter.
--
-- The client exposes short aliases for several standard library functions
-- (strmatch, strfind, strsub, ...) as globals. Addon source and vendored
-- libraries may legitimately call them, so every offline Lua 5.1 harness must
-- provide the same surface before loading addon code. Require this file from
-- the harness entry points; it never touches addon state itself.

if type(strmatch) ~= "function" then
    strmatch = string.match
end
if type(strfind) ~= "function" then
    strfind = string.find
end
if type(strsub) ~= "function" then
    strsub = string.sub
end
if type(strrep) ~= "function" then
    strrep = string.rep
end
if type(strlen) ~= "function" then
    strlen = string.len
end
if type(strlower) ~= "function" then
    strlower = string.lower
end
if type(strupper) ~= "function" then
    strupper = string.upper
end
if type(strbyte) ~= "function" then
    strbyte = string.byte
end
if type(strchar) ~= "function" then
    strchar = string.char
end
if type(strformat) ~= "function" then
    strformat = string.format
end
if type(strgsub) ~= "function" then
    strgsub = string.gsub
end
if type(strjoin) ~= "function" then
    strjoin = function(separator, ...)
        local parts = {}
        for index = 1, select("#", ...) do
            parts[index] = tostring(select(index, ...))
        end
        return table.concat(parts, separator)
    end
end
if type(strsplit) ~= "function" then
    strsplit = function(separator, text, limit)
        local parts = {}
        if text == nil then
            return unpack(parts)
        end
        local pattern = "([^" .. separator .. "]+)"
        local count = 0
        for piece in string.gmatch(text, pattern) do
            count = count + 1
            parts[count] = piece
            if limit and count == limit then
                break
            end
        end
        return unpack(parts)
    end
end
