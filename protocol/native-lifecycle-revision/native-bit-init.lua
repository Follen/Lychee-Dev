-- Standalone Lua51 fixture's real native word provider. This is not an addon
-- loader or a claim about any client's availability, binding or input safety.
assert(_VERSION == 'Lua 5.1')
bit = require('bit')
assert(bit.bxor(0xffffffff, 0) == -1)
assert(bit.band(0xffffffff, 0x12345678) == 0x12345678)
assert(bit.ror(0x12345678, 8) % 4294967296 == 0x78123456)
