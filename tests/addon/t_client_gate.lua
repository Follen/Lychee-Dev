local Env = ...
local original = GetBuildInfo
for _, profile in ipairs({
    { "retail", Env.testBuilds.retail },
    { "retail", { "12.1.0", "69933", "Sep 2026", 120100 } },
    { "classic-era", Env.testBuilds["classic-era"] },
    { "anniversary", Env.testBuilds.anniversary },
    { "forever", Env.testBuilds.forever },
    { "forever", { "1.60.1", "70707", "fixture date", 16001 } },
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
-- One admitted profile still observes the full build afresh at each publication.
GetBuildInfo = function() return "1.60.1", "70658", "fixture date", 16001 end
local formal = {}
local frames = Env.framesCreated
Env.LoadAddon("Core/ClientGate.lua", formal)
Env.LoadAddon("Core/Platform.lua", formal)
GetBuildInfo = function() return "1.60.1", "70707", "fixture date", 16001 end
assert(formal.Platform.ObserveBuild().build == "1.60.1.70707", "Forever reused the earlier full build")
GetBuildInfo = function() return "1.60.1", "70707", "fixture date", 16002 end
local unsupported, reason = formal.Platform.ObserveBuild()
assert(unsupported == nil and reason == "platform_unsupported_build", "Forever guessed an unsupported Interface")
assert(Env.framesCreated == frames, "Forever build observations created an idle frame")
GetBuildInfo = original
