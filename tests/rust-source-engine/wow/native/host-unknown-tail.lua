local savedUse = Use
savedUse(UnknownCall())
savedUse((UnknownCall()))
local function Relay(a, b) return a, b end
savedUse(Relay(UnknownCall()))
