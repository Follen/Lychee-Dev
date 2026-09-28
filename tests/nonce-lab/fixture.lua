SlashCmdList = {}
local out = assert(arg[1])
LycheeNonceLabFixture = function(encode, fromHex)
    local record = encode(fromHex("00112233445566778899aabbccddeeff"), "RUN00001", 1, "TICKET01", "hello\000world")
    local file = assert(io.open(out, "wb")); file:write(record); file:close()
end
dofile("tests/nonce-lab/addon/LycheeNonceLab/Lab.lua")
