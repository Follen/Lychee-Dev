local Env = ...
local original = GetBuildInfo
for _, client in ipairs({ "classic-era", "anniversary" }) do
    local expected = Env.testBuilds[client]
    GetBuildInfo = function() return unpack(expected) end
    local ns = {}
    local frames = Env.framesCreated
    Env.LoadAddon("Core/ClientGate.lua", ns)
    Env.LoadAddon("Core/Platform.lua", ns)
    assert(ns.Client == client, client .. " runtime profile is missing")
    local build = assert(ns.Platform.ObserveBuild())
    assert(build.product == client and build.interface == expected[4] and build.build == expected[1] .. "." .. expected[2])
    assert(Env.framesCreated == frames, "client selection created an idle frame")
end
GetBuildInfo = original
