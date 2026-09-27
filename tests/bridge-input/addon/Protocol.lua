local _, ns = ...
local function digest(text)
    local a, b = 1, 0
    for i = 1, #text do a = (a + string.byte(text, i)) % 65521; b = (b + a) % 65521 end
    return string.format("%08x", b * 65536 + a)
end
ns.Protocol = { Digest = digest }
function ns.Protocol.Parse(text, nonce)
    if type(text) ~= "string" or #text > 128 then return nil, "capacity" end
    local received, body, checksum = string.match(text, "^LDIL1:([0-9a-f]+):([A-Za-z0-9_-]+):([0-9a-f]+)$")
    if not received or #received ~= 16 or #body > 48 or #checksum ~= 8 then return nil, "format" end
    if received ~= nonce then return nil, "nonce" end
    if digest("LDIL1:" .. received .. ":" .. body) ~= checksum then return nil, "checksum" end
    return body
end
