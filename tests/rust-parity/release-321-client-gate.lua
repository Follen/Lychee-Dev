-- Standalone Lua 5.1 fixture for the Go 3.2.1 client-gate increment.
-- Execute only through the controlled Lua runner; this file is not game input.
local addon = assert(arg[1], "addon root required")
local original = GetBuildInfo
local created = 0
CreateFrame = function() created = created + 1; error("client selection must be idle") end
for _, row in ipairs({
    { "12.1.5", "70391", 120105, "retail" },
    { "12.1.0", "69933", 120100, "retail" },
    { "5.5.4", "70032", 50504, "classic" },
    { "1.15.9", "70003", 11509, "classic-era" },
    { "2.5.6", "70006", 20506, "anniversary" },
    { "3.80.2", "69874", 38002, "titan" },
    { "1.60.1", "70124", 16001, "forever" },
}) do
    GetBuildInfo = function() return row[1], row[2], "fixture", row[3] end
    local ns = {}
    assert(loadfile(addon .. "/Core/ClientGate.lua"))("Lychee Dev", ns)
    assert(ns.Client == row[4])
    assert(ns.PlatformProfile.interface == row[3])
    assert(ns.PlatformProfile.version == row[1])
end
for _, unsupported in ipairs({ 120106, 0, -1, "120105" }) do
    GetBuildInfo = function() return "12.1.5", "70391", "fixture", unsupported end
    local ns = {}
    assert(loadfile(addon .. "/Core/ClientGate.lua"))("Lychee Dev", ns)
    assert(ns.Client == nil and ns.PlatformProfile == nil)
end
GetBuildInfo = nil
local ns = {}
assert(loadfile(addon .. "/Core/ClientGate.lua"))("Lychee Dev", ns)
assert(ns.Client == nil and ns.PlatformProfile == nil)
assert(created == 0)
GetBuildInfo = original
print("release-3.2.1 client gate: passed")
