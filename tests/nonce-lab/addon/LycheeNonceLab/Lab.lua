-- Independent experiment: no frames, hooks, timers or startup publication.
local retained = {}
local function u32(n)
    return string.char(n % 256, math.floor(n / 256) % 256,
        math.floor(n / 65536) % 256, math.floor(n / 16777216) % 256)
end
local function adler(s)
    local a, b = 1, 0
    for i = 1, #s do a = (a + s:byte(i)) % 65521; b = (b + a) % 65521 end
    return b * 65536 + a
end
local function fromHex(s)
    assert(type(s) == "string" and #s == 32 and not s:find("[^0-9a-fA-F]"), "nonce must be 32 hex digits")
    return (s:gsub("..", function(x) return string.char(tonumber(x, 16)) end))
end
local function encode(nonce, run, epoch, ticket, payload)
    assert(#nonce == 16 and #run == 8 and #ticket == 8)
    assert(#payload <= 524288 and epoch >= 0 and epoch < 4294967296)
    local head = "LYCHN001" .. nonce .. run .. u32(epoch) .. ticket
        .. string.char(3, 0, 0, 0) .. u32(64) .. u32(#payload) .. u32(adler(payload))
    return head .. u32(adler(head)) .. payload .. "LYCHEND1" .. nonce
end
local function publish(hex)
    local nonce = fromHex(hex)
    if retained[hex] then print("NONCELAB duplicate nonce refused"); return end
    if #retained >= 8 then print("NONCELAB capacity reached; clear first"); return end
    local run = u32(GetServerTime()) .. u32(math.floor(GetTime() * 1000) % 4294967296)
    local payload = string.rep("nonce-lab payload;", 8192)
    local record = encode(nonce, run, 1, "TICKET01", payload)
    retained[#retained + 1] = hex
    retained[hex] = record
    _G.LycheeNonceLabRecords = retained
    local runHex = (run:gsub(".", function(c) return string.format("%02x", c:byte()) end))
    print("NONCELAB published nonce=" .. hex .. " bytes=" .. #payload)
    print("NONCELAB run=" .. runHex .. " epoch=1 ticket=TICKET01")
end
SLASH_LYCHEENONCELAB1 = "/noncelab"
SlashCmdList.LYCHEENONCELAB = function(input)
    if input == "clear" then retained = {}; _G.LycheeNonceLabRecords = nil; print("NONCELAB cleared"); return end
    local hex = input:match("^publish (%x+)$")
    if not hex then print("NONCELAB publish <32 hex nonce> | clear"); return end
    local ok, reason = pcall(publish, hex)
    if not ok then print("NONCELAB " .. tostring(reason)) end
end
-- Fixture hook exists only in the offline Lua harness.
if _G.LycheeNonceLabFixture then _G.LycheeNonceLabFixture(encode, fromHex) end
