local Env = ...
local original = GetBuildInfo
GetBuildInfo = function() return "1.15.9", "70003", "", 11509 end
local ns = {}
local frames = Env.framesCreated
Env.LoadAddon("Core/ClientGate.lua", ns)
Env.LoadAddon("Core/Platform.lua", ns)
assert(ns.Client == "classic-era", "Era runtime profile is missing")
local build = assert(ns.Platform.ObserveBuild())
assert(build.product == "classic-era" and build.interface == 11509 and build.build == "1.15.9.70003")
assert(Env.framesCreated == frames, "client selection created an idle frame")
GetBuildInfo = original
