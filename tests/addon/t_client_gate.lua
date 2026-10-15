local Env = ...
local original = GetBuildInfo
for _, profile in ipairs({
    { "retail", Env.testBuilds.retail },
    { "retail", { "12.1.0", "69933", "Sep 2026", 120100 } },
    { "classic-era", Env.testBuilds["classic-era"] },
    { "anniversary", Env.testBuilds.anniversary },
}) do
    local client, expected = profile[1], profile[2]
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
