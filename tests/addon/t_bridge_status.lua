-- R4-04: /dev status and the real INPUT lifecycle agree on default/on/off.
local Env, client = ...
LycheeToolkitDB, LycheeToolkitBridgeDB = nil, nil
local ns = Env.LoadWorkbench()
ns.Release = "test"
ns.Startup = {}
ns.Platform = {
    ObserveBuild = function() return { build = "70000", product = client } end,
    ObserveActor = function() return { guid = "Player-1-1", character = "Tester", realm = "Realm" } end,
    ObserveInputState = function() return true end,
}
ns.ReceiverBindings = { Current = function() return { wake = "ALT-CTRL-F12", reload = "ALT-CTRL-F11" } end }
ns.ActivityView = { Stop = function() end }
IsPlayerInWorld = function() return true end
ns.Compat.GetAddOnMetadata = function(name, key)
    if key == "Version" then return ns.Release end
    if key == "X-Lychee-Transport" then return "Lycheedev mailbox protocol v1" end
    return tostring(tonumber(name:match("(%d+)$")))
end
Env.LoadAddon("Modules/AutomationHistory.lua", ns)
for _, file in ipairs({ "MemoryProtocol", "CaptureWriter", "SlotProtocol", "InputProtection", "ProbeExecution", "InputState", "SlotRuntime" }) do
    Env.LoadAddon("Bridge/" .. file .. ".lua", ns)
end
Env.LoadAddon("Core/Controls.lua", ns)
Env.LoadAddon("Core/Startup.lua", ns)
assert(Env.framesCreated == 0 and #Env.pendingTimers == 0 and not ns.Mailbox,
    "unstarted runtime created recurring work")
assert(not ns.Persistence.BridgeEnabled(), "unloaded persistence claimed the bridge was enabled")

local sampler
local createFrame = CreateFrame
CreateFrame = function(...)
    sampler = createFrame(...)
    return sampler
end
assert(ns.StartRuntime())
CreateFrame = createFrame
assert(LycheeToolkitDB.options.bridgeEnabled == nil, "startup unexpectedly rewrote the default option")
local mailbox = assert(ns.Mailbox)
local status = ns.Controls.Handle("status")
assert(status.transport == "Lycheedev mailbox protocol v1" and status.enabled == true
    and type(mailbox.input) == "string" and mailbox.input == ns.InputState.Snapshot(),
    "new-account status disagreed with the enabled INPUT publication")
local frames = Env.framesCreated
assert(frames == 1 and #Env.pendingTimers == 0, "default startup created work beyond the enabled INPUT sampler")
assert(ns.Controls.Handle("bridge off"))
assert(LycheeToolkitDB.options.bridgeEnabled == false and ns.Controls.Handle("status").enabled == false
    and not mailbox.input and not ns.InputState.Snapshot(), "explicit off retained enabled status or INPUT")
assert(not sampler:GetScript("OnUpdate") and not sampler:GetScript("OnEvent") and not sampler.registeredEvent,
    "explicit off retained sampling work")
assert(ns.StartRuntime() and Env.framesCreated == frames and not mailbox.input,
    "restarting an explicitly disabled bridge revived INPUT or allocated a frame")
assert(ns.Controls.Handle("bridge on"))
assert(LycheeToolkitDB.options.bridgeEnabled == true and ns.Controls.Handle("status").enabled == true
    and type(mailbox.input) == "string" and Env.framesCreated == frames,
    "explicit on did not restore status and INPUT using the existing sampler")
assert(ns.Controls.Handle("bridge off"))

-- Simulate another addon runtime loading the saved explicit-off option. Its
-- startup remains frameless and leaves no public mailbox or sampling authority.
Env.LoadAddon("Bridge/InputState.lua", ns)
Env.LoadAddon("Bridge/SlotRuntime.lua", ns)
Env.LoadAddon("Core/Startup.lua", ns)
assert(ns.StartRuntime() and Env.framesCreated == frames and not ns.Mailbox and not ns.InputState.Snapshot()
    and ns.Controls.Handle("status").enabled == false, "saved explicit-off startup did not stay disabled")
print("bridge default, explicit-off and explicit-on production startup/status ok")
