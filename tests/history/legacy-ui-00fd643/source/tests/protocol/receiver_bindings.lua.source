local root = assert(arg[1])
local ns = { Startup = { ready = true } }
local frames, overrides, player = {}, {}, {}
local higherPriority = {}
local combat, active, failKey, ineffectiveKey, ineffectiveAction = false, false, nil, nil, nil
UIParent = {}
LycheeToolkitDB = { schema = 1, options = {} }
ns.Persistence = { Current = function() return LycheeToolkitDB end }
ns.Receiver = { IsActive = function() return active end }
InCombatLockdown = function() return combat end
GetBindingAction = function(chord, checkOverride)
    if checkOverride and higherPriority[chord] then return higherPriority[chord] end
    if checkOverride and overrides[chord] and chord == ineffectiveKey then return ineffectiveAction or "CLICK OtherAddon:LeftButton" end
    if checkOverride and overrides[chord] then return "CLICK " .. overrides[chord] .. ":LeftButton" end
    return player[chord] or ""
end
ClearOverrideBindings = function() overrides = {} end
SetOverrideBindingClick = function(_, priority, chord, button)
    assert(priority == false)
    if chord == failKey then error("simulated registration failure") end
    overrides[chord] = button
    return true
end
SaveBindings = function() error("must not save account bindings") end
CreateFrame = function(kind, name)
    local frame = { kind = kind, name = name, events = {} }
    function frame:RegisterForClicks() end
    function frame:SetScript(event, callback) self[event] = callback end
    function frame:GetName() return self.name end
    function frame:RegisterEvent(event) self.events[event] = true end
    function frame:UnregisterAllEvents() self.events = {} end
    frames[#frames + 1] = frame
    return frame
end
assert(loadfile(root .. "/Bridge/ReceiverBindings.lua"))("Lychee Dev", ns)
local bindings = ns.ReceiverBindings
assert(#frames == 0, "module load created frames")
assert(bindings.Register({ wake = function() end, submit = function() end, close = function() end }))
assert(#frames == 4 and overrides["ALT-CTRL-]"], "default registration")
local current = assert(bindings.Current())
assert(current.wake == "ALT-CTRL-]" and current.submit == "ALT-CTRL-SHIFT-]"
    and current.close == "ALT-CTRL-[", "default chords")
current.wake = "tamper"
assert(bindings.Current().wake == "ALT-CTRL-]", "current must be a copy")
assert(bindings.Display("close") == "Ctrl+Alt+[", "display uses effective chord")
assert(bindings.MatchKey("]", "submit") and not bindings.MatchKey("[", "submit"), "focused terminal")

player["ALT-CTRL-F1"] = "PLAYER_ACTION"
local result, reason = bindings.Configure("wake", "ALT-CTRL-F1")
assert(result == nil and reason == "receiver_binding_conflict:wake:ALT-CTRL-F1", "player conflict visible")
assert(bindings.Current().wake == "ALT-CTRL-]" and LycheeToolkitDB.options.receiverBindings == nil,
    "conflict changed effective or saved binding")
player["ALT-CTRL-F1"] = nil
assert(bindings.Configure("wake", "ALT-CTRL-F1"))
assert(bindings.Configure("submit", "ALT-CTRL-SHIFT-F2"))
assert(bindings.Configure("close", "ALT-CTRL-F3"))
assert(bindings.Current().close == "ALT-CTRL-F3" and bindings.MatchKey("F3", "close"),
    "custom profile ineffective")
assert(overrides["ALT-CTRL-F1"] and not overrides["ALT-CTRL-]"], "old override retained")
assert(LycheeToolkitDB.options.receiverBindings.close == "ALT-CTRL-F3", "profile not persisted")

for _, bad in ipairs({ "ALT-CTRL-A", "CTRL-ALT-F4", "ESCAPE", "ALT-CTRL-F13" }) do
    result, reason = bindings.Configure("wake", bad)
    assert(result == nil and reason == "receiver_binding_invalid", "accepted unsupported chord: " .. bad)
end
result, reason = bindings.Configure("close", "ALT-CTRL-SHIFT-F1")
assert(result == nil and reason == "receiver_close_key_conflict", "close terminal shares wake")
combat = true
result, reason = bindings.Configure("wake", "ALT-CTRL-F4")
assert(result == nil and reason == "receiver_combat", "combat rebind")
combat = false
active = true
result, reason = bindings.Configure("wake", "ALT-CTRL-F4")
assert(result == nil and reason == "receiver_active", "active transaction rebind")
active = false
failKey = "ALT-CTRL-F4"
result, reason = bindings.Configure("wake", failKey)
assert(result == nil and reason == "receiver_bindings_unavailable", "failed registration result")
assert(bindings.Current().wake == "ALT-CTRL-F1" and LycheeToolkitDB.options.receiverBindings.wake == "ALT-CTRL-F1"
    and overrides["ALT-CTRL-F1"], "failed registration not rolled back")
failKey = nil
ineffectiveKey = "ALT-CTRL-F5"
result, reason = bindings.Configure("wake", ineffectiveKey)
assert(result == nil and reason == "receiver_binding_ineffective:wake:ALT-CTRL-F5",
    "masked override was advertised effective")
assert(bindings.Current().wake == "ALT-CTRL-F1" and overrides["ALT-CTRL-F1"],
    "masked override failed rollback")
ineffectiveKey = nil
local priorProfile=assert(bindings.Current())
failKey="ALT-CTRL-F6"
result,reason=bindings.ConfigureProfile({wake="ALT-CTRL-F4",submit="ALT-CTRL-SHIFT-F5",close=failKey})
assert(result==nil and bindings.Current().wake==priorProfile.wake and bindings.Current().submit==priorProfile.submit,
    "multi-key update was partially applied")
assert(LycheeToolkitDB.options.receiverBindings.wake==priorProfile.wake,"failed save changed persistence")
failKey=nil
ineffectiveKey = "ALT-CTRL-F1"
result, reason = bindings.Current()
assert(result == nil and reason == "receiver_binding_ineffective:wake:ALT-CTRL-F1",
    "post-install binding change was advertised effective")
ineffectiveKey = nil
assert(bindings.Reset())
assert(bindings.Current().wake == "ALT-CTRL-]" and LycheeToolkitDB.options.receiverBindings == nil,
    "reset defaults")
assert(loadfile(root .. "/Core/Controls.lua"))("Lychee Dev", ns)
local confirmation = assert(ns.Controls.Handle("receiver bind wake ALT-CTRL-F1"))
assert(confirmation == "receiver wake = ALT-CTRL-F1" and bindings.Current().wake == "ALT-CTRL-F1",
    "manual command did not configure effective wake")
result, reason = ns.Controls.Handle("receiver bind close ALT-CTRL-SHIFT-F1")
assert(result == nil and reason == "receiver_close_key_conflict", "manual command bypassed validation")
assert(ns.Controls.Handle("receiver reset") == "receiver bindings reset"
    and bindings.Current().wake == "ALT-CTRL-]", "manual reset")
print("receiver bindings: conflict, rollback and effective profile")

-- Login and combat install only the bootstrap owner until the single event
-- allows registration. Successful registration leaves no idle event or timer.
for _, scenario in ipairs({"login", "combat-login", "conflict"}) do
    frames, overrides, player = {}, {}, {}
    combat = scenario == "combat-login"
    IsLoggedIn = function() return false end
    assert(loadfile(root .. "/Bridge/ReceiverBindings.lua"))("Lychee Dev", ns)
    assert(ns.ReceiverBindings.Register({wake=function() end,submit=function() end,close=function() end}))
    assert(#frames == 1 and frames[1].events.PLAYER_LOGIN and not frames[1].OnUpdate)
    if scenario == "conflict" then player["ALT-CTRL-]"] = "PLAYER_ACTION" end
    IsLoggedIn = function() return true end
    frames[1].OnEvent(frames[1], "PLAYER_LOGIN")
    if combat then
        assert(#frames == 1 and frames[1].events.PLAYER_REGEN_ENABLED and not frames[1].events.PLAYER_LOGIN)
        combat = false
        frames[1].OnEvent(frames[1], "PLAYER_REGEN_ENABLED")
    end
    assert(next(frames[1].events) == nil and frames[1].OnEvent == nil and not frames[1].OnUpdate)
    if scenario == "conflict" then
        assert(#frames == 1 and ns.ReceiverBindings.Current() == nil and next(overrides) == nil)
    else
        assert(#frames == 4 and ns.ReceiverBindings.Current())
    end
end
-- Native slots ignore saved optical key profiles without rewriting user data.
frames,overrides,player={},{},{}
combat,failKey,ineffectiveKey=false,nil,nil
IsLoggedIn=function()return true end
ns.SlotRuntime={}
local saved={wake="ALT-CTRL-F1",submit="ALT-CTRL-F2",close="ALT-CTRL-F3"}
LycheeToolkitDB.options.receiverBindings=saved
assert(loadfile(root.."/Bridge/ReceiverBindings.lua"))("Lychee Dev",ns)
assert(ns.ReceiverBindings.Register({wake=function()end,submit=function()end,close=function()end}))
assert(ns.ReceiverBindings.Current().wake=="ALT-CTRL-F12" and not overrides["ALT-CTRL-]"])
assert(LycheeToolkitDB.options.receiverBindings==saved)
assert(not ns.ReceiverBindings.Configure("wake","ALT-CTRL-F1"))

-- Each bootstrap tries only the two complete native profiles. A failed lookup
-- never falls back, and lookalike CLICK text is another owner's binding.
local originalPrint=print
for _,scenario in ipairs({"wake","submit","ineffective","both","close","api","lookalike","suffix","wrong_action","wrong_mouse","installed_lookalike","installed_suffix","installed_action","installed_mouse","login","combat_login"}) do
    frames,overrides,player={},{},{}
    combat,active,failKey,ineffectiveKey,ineffectiveAction=false,false,nil,nil,nil
    local blocked=scenario=="submit" and "ALT-CTRL-SHIFT-F12" or "ALT-CTRL-F12"
    local foreign="TOGGLELYCHEE"
    if scenario=="lookalike" or scenario=="installed_lookalike" then foreign="CLICK OtherLycheeDevReceiverWake:LeftButton"
    elseif scenario=="suffix" or scenario=="installed_suffix" then foreign="CLICK LycheeDevReceiverWakeExtra:LeftButton"
    elseif scenario=="wrong_action" or scenario=="installed_action" then foreign="CLICK LycheeDevReceiverSubmit:LeftButton"
    elseif scenario=="wrong_mouse" or scenario=="installed_mouse" then foreign="CLICK LycheeDevReceiverWake:RightButton" end
    local installed=scenario:find("installed_",1,true)==1
    local deferred=scenario=="login" or scenario=="combat_login"
    local loggedIn=not deferred
    IsLoggedIn=function()return loggedIn end
    if scenario=="ineffective" or installed then ineffectiveKey="ALT-CTRL-F12";ineffectiveAction=installed and foreign or nil
    elseif scenario=="api" then failKey="ALT-CTRL-F12"
    elseif scenario=="close" then player["ALT-CTRL-["]=foreign
    else player[blocked]=foreign end
    if scenario=="both" then player["ALT-CTRL-F11"]="FALLBACK_PLAYER_ACTION" end
    local notices={}
    print=function(text)notices[#notices+1]=text end
    assert(loadfile(root.."/Bridge/ReceiverBindings.lua"))("Lychee Dev",ns)
    local callbacks={wake=function()end,submit=function()end,close=function()end}
    local ok,reason=ns.ReceiverBindings.Register(callbacks)
    if deferred then
        assert(ok and #frames==1 and frames[1].events.PLAYER_LOGIN and #notices==0,"deferred bootstrap bound early")
        assert(not ns.ReceiverBindings.Register(callbacks) and #frames==1 and frames[1].events.PLAYER_LOGIN,
            "pending repeated registration claimed effective bindings or changed event")
        loggedIn=true;combat=scenario=="combat_login"
        frames[1].OnEvent(frames[1],"PLAYER_LOGIN")
        if combat then
            assert(#frames==1 and frames[1].events.PLAYER_REGEN_ENABLED and #notices==0)
            combat=false;frames[1].OnEvent(frames[1],"PLAYER_REGEN_ENABLED")
        end
        ok,reason=ns.ReceiverBindings.Register(callbacks)
    end
    local fails=scenario=="both" or scenario=="close" or scenario=="api"
    assert((ok~=nil)==not fails,"unexpected bounded profile selection: "..scenario)
    if fails then
        assert(ns.ReceiverBindings.Current()==nil and next(overrides)==nil,"occupied profile granted readiness")
        assert(not ns.ReceiverBindings.Register(callbacks),"failed bootstrap repeated registration claimed success")
    else
        local profile=assert(ns.ReceiverBindings.Current())
        assert(profile.wake=="ALT-CTRL-F11" and profile.submit=="ALT-CTRL-SHIFT-F11" and profile.close=="ALT-CTRL-[")
        assert(ns.ReceiverBindings.Register(callbacks),"effective fallback repeated registration failed")
        assert(notices[1]:find("F12",1,true) and notices[1]:find("F11",1,true),"fallback warning omitted profiles")
        if installed then assert(notices[1]:find(foreign,1,true),"ineffective lookup warning hid actual action") end
        if scenario~="ineffective" and not installed then
            assert(player[blocked]==foreign and notices[1]:find(foreign,1,true),"foreign binding changed or warning hid actual action")
        end
        for _,spoof in ipairs({"CLICK OtherLycheeDevReceiverWake:LeftButton","CLICK LycheeDevReceiverWakeExtra:LeftButton",
            "CLICK LycheeDevReceiverSubmit:LeftButton","CLICK LycheeDevReceiverWake:RightButton"}) do
            ineffectiveKey,ineffectiveAction="ALT-CTRL-F11",spoof
            assert(ns.ReceiverBindings.Current()==nil,"lookalike current binding advertised ready")
            local before=overrides["ALT-CTRL-F11"]
            assert(not ns.ReceiverBindings.Register(callbacks) and overrides["ALT-CTRL-F11"]==before,
                "ineffective repeated registration changed overrides")
        end
        ineffectiveKey,ineffectiveAction=nil,nil
    end
    assert(#notices==(scenario=="api" and 0 or 1),"bootstrap warning repeated or generic failure selected fallback")
    assert(#frames<=4,"fallback allocated a second binding foundation")
    for _,frame in ipairs(frames) do assert(next(frame.events)==nil and not frame.OnUpdate,"fallback added idle work") end
    assert(LycheeToolkitDB.options.receiverBindings==saved,"native fallback changed saved profile")
    print=originalPrint
end
print("native receiver profiles: bounded fallback, truthful registration and exact action ownership")

-- A runtime can start with primary keys and meet its first conflict later.
-- Explicit reset must warn on selecting fallback while preserving the foreign
-- override and unrelated account bindings; subsequent resets stay quiet.
frames,overrides,player,higherPriority={},{},{},{}
combat,active,failKey,ineffectiveKey,ineffectiveAction=false,false,nil,nil,nil
IsLoggedIn=function()return true end
ns.Receiver=nil
ns.SlotRuntime={IsActive=function()return active end}
player["ALT-CTRL-F1"]="SAVED_PLAYER_ACTION"
local notices={}
print=function(text)notices[#notices+1]=text end
assert(loadfile(root.."/Bridge/ReceiverBindings.lua"))("Lychee Dev",ns)
local callbacks={wake=function()end,submit=function()end,close=function()end}
assert(ns.ReceiverBindings.Register(callbacks))
assert(ns.ReceiverBindings.Current().wake=="ALT-CTRL-F12" and #notices==0,"primary startup warned without conflict")
local foreignAction="CLICK FixtureForeignWake:LeftButton"
higherPriority["ALT-CTRL-F12"]=foreignAction
assert(ns.ReceiverBindings.Current()==nil,"later foreign takeover advertised readiness")
local ownOverrides=overrides
combat=true
local ok,reason=ns.ReceiverBindings.Reset()
assert(ok==nil and reason=="receiver_combat" and overrides==ownOverrides and #notices==0)
combat=false;active=true
ok,reason=ns.ReceiverBindings.Reset()
assert(ok==nil and reason=="receiver_active" and overrides==ownOverrides and #notices==0)
active=false
assert(ns.ReceiverBindings.Reset())
assert(ns.ReceiverBindings.Current().wake=="ALT-CTRL-F11","late conflict reset did not select fallback")
assert(#notices==1 and notices[1]:find(foreignAction,1,true)
    and notices[1]:find("F12",1,true) and notices[1]:find("F11",1,true),"first late conflict reset did not warn")
assert(higherPriority["ALT-CTRL-F12"]==foreignAction and player["ALT-CTRL-F1"]=="SAVED_PLAYER_ACTION",
    "late conflict reset changed foreign or account bindings")
assert(ns.ReceiverBindings.Reset() and #notices==1,"repeated fallback reset spammed warning")
assert(LycheeToolkitDB.options.receiverBindings==saved and #frames==4,"late reset changed saved profile or frame budget")
for _,frame in ipairs(frames) do assert(next(frame.events)==nil and not frame.OnUpdate,"late reset added idle work") end
print=originalPrint
print("native reset: first late conflict warns once and preserves other bindings")
