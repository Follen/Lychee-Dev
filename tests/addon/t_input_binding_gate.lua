local Env,client=...
local ns=Env.LoadWorkbench()
ns.Startup={ready=true}
local base,foreign={},{}
local combat,active,loggedIn=false,false,true
local saves,writes=0,0
local bindingSet,savedSet,failKey,changeSetOnWrite=2,nil,nil,false
local waitingFrames={}
local createFrame=CreateFrame
CreateFrame=function(...)
    local frame=createFrame(...)
    local register,unregister=frame.RegisterEvent,frame.UnregisterAllEvents
    frame.events={}
    function frame:RegisterEvent(event) register(self,event);self.events[event]=true end
    function frame:UnregisterAllEvents() unregister(self);self.events={} end
    waitingFrames[#waitingFrames+1]=frame
    return frame
end
local function emit(event)
    for _,frame in ipairs(waitingFrames)do
        if frame.events[event] then Env.FireScript(frame,"OnEvent",event) end
    end
end
InCombatLockdown=function()return combat end
IsLoggedIn=function()return loggedIn end
GetCurrentBindingSet=function()return bindingSet end
SaveBindings=function(set)
    assert(set==1 or set==2,"Usage: SaveBindings(1|2)")
    saves=saves+1;savedSet=set
    assert(saves<5,"binding event synchronously reentered default installation")
    emit("UPDATE_BINDINGS")
end
GetBindingAction=function(key,overrides)return overrides and (foreign[key] or base[key] or "") or (base[key] or "")end
GetBindingKey=function(action)
    local keys={}
    for key,bound in pairs(base)do if bound==action then keys[#keys+1]=key end end
    table.sort(keys);return unpack(keys)
end
SetBinding=function(key,action)
    if key==failKey then return false end
    writes=writes+1;base[key]=action
    if changeSetOnWrite then bindingSet=0 end
    emit("UPDATE_BINDINGS")
    return true
end
SetOverrideBinding=function()error("formal bindings must not install overrides")end
ns.InputProtection={IsActive=function()return active end}
Env.LoadAddon("Bridge/ReceiverBindings.lua",ns)
local woke,reloaded=0,0
local callbacks={wake=function()woke=woke+1 end,reload=function()reloaded=reloaded+1 end}
base["ALT-CTRL-F12"]="PLAYER_ACTION"
assert(ns.ReceiverBindings.Register(callbacks))
assert(base["ALT-CTRL-F12"]=="PLAYER_ACTION")
assert(GetBindingKey("LYCHEEDEV_WAKE")=="ALT-CTRL-F10")
assert(GetBindingKey("LYCHEEDEV_RELOAD")=="ALT-CTRL-F11")
assert(saves==1 and writes==2,"defaults were not saved once")
local profile=assert(ns.ReceiverBindings.Current())
assert(profile.wake=="ALT-CTRL-F10" and profile.reload=="ALT-CTRL-F11")
ns.ReceiverBindings.Dispatch("wake");ns.ReceiverBindings.Dispatch("reload")
assert(woke==1 and reloaded==1)
assert(not ns.ReceiverBindings.Dispatch("submit") and not ns.ReceiverBindings.Dispatch("close"))
assert(ns.ReceiverBindings.Register(callbacks));assert(writes==2 and saves==1)
-- Settings changes are authoritative; reset only fills missing bindings.
base["ALT-CTRL-F10"]=nil;base["ALT-CTRL-F5"]="LYCHEEDEV_WAKE"
assert(ns.ReceiverBindings.Current().wake=="ALT-CTRL-F5")
assert(ns.ReceiverBindings.Reset());assert(writes==2 and saves==1)
foreign["ALT-CTRL-F5"]="OTHER_ADDON"
assert(not ns.ReceiverBindings.Current());assert(not ns.ReceiverBindings.Reset())
assert(writes==2 and saves==1 and foreign["ALT-CTRL-F5"]=="OTHER_ADDON")
foreign["ALT-CTRL-F5"]=nil
combat=true;assert(not ns.ReceiverBindings.Reset());combat=false
active=true;assert(not ns.ReceiverBindings.Reset());active=false
assert(writes==2 and saves==1)
-- Occupied defaults are preserved, with a clear unavailable result.
base["ALT-CTRL-F5"]=nil;base["ALT-CTRL-F10"]="SECOND_PLAYER_ACTION"
assert(not ns.ReceiverBindings.Reset())
assert(base["ALT-CTRL-F10"]=="SECOND_PLAYER_ACTION" and writes==2)
base["ALT-CTRL-F10"]=nil
assert(ns.ReceiverBindings.Reset());assert(saves==2 and writes==3)

local function fresh(player)
    ns=Env.LoadWorkbench()
    ns.Startup={ready=true}
    ns.InputProtection={IsActive=function()return active end}
    base,foreign=player or {},{}
    combat,active,loggedIn=false,false,true
    bindingSet,savedSet,failKey,changeSetOnWrite=2,nil,nil,false
    saves,writes,waitingFrames=0,0,{}
    Env.LoadAddon("Bridge/ReceiverBindings.lua",ns)
    return ns
end
local function idle()
    for _,frame in ipairs(waitingFrames)do
        assert(next(frame.events)==nil and not frame:GetScript("OnEvent") and not frame:GetScript("OnUpdate"))
    end
end

-- Startup deferral remains a one-shot listener and does not create buttons or
-- timers. Login and combat may be encountered in either order.
for _,scenario in ipairs({"ready","login","combat","combat-login"})do
    fresh()
    combat=scenario=="combat" or scenario=="combat-login"
    loggedIn=scenario=="ready" or scenario=="combat"
    assert(#waitingFrames==0 and ns.ReceiverBindings.Register(callbacks))
    if not loggedIn then
        assert(#waitingFrames==1 and waitingFrames[1].events.PLAYER_LOGIN and saves==0)
        loggedIn=true;emit("PLAYER_LOGIN")
    end
    if combat then
        assert(#waitingFrames==1 and waitingFrames[1].events.PLAYER_REGEN_ENABLED and saves==0)
        combat=false;emit("PLAYER_REGEN_ENABLED")
    end
    local selected=assert(ns.ReceiverBindings.Current())
    assert(selected.wake=="ALT-CTRL-F12" and selected.reload=="ALT-CTRL-F11")
    assert(saves==1 and savedSet==2 and #waitingFrames==(scenario=="ready" and 0 or 1))
    idle()
    assert(ns.ReceiverBindings.Register(callbacks) and saves==1)
end

-- Rejecting the second key rolls back only this attempt's first key.
fresh({F1="PLAYER_ACTION"})
failKey="ALT-CTRL-F11"
assert(not ns.ReceiverBindings.Register(callbacks))
assert(base.F1=="PLAYER_ACTION" and not base["ALT-CTRL-F12"] and saves==0)
failKey=nil
assert(ns.ReceiverBindings.Reset() and saves==1)

-- A binding scheme may be unavailable at login. No default can be written
-- until a current account/character scheme is known; intervening edits win.
for _,invalid in ipairs({{value=0},{},{value=3},{value="2"},{value=Env.MakeSecret()}})do
    for _,event in ipairs({"BINDINGS_LOADED","UPDATE_BINDINGS"})do
        fresh({F1="PLAYER_ACTION"})
        loggedIn,bindingSet=false,invalid.value
        local published=0
        ns.Bootstrap={Publish=function()published=published+1 end}
        assert(ns.ReceiverBindings.Register(callbacks))
        loggedIn=true;emit("PLAYER_LOGIN")
        assert(writes==0 and saves==0 and base.F1=="PLAYER_ACTION")
        assert(waitingFrames[1].events.BINDINGS_LOADED and waitingFrames[1].events.UPDATE_BINDINGS)
        emit(event)
        assert(writes==0 and saves==0 and #waitingFrames==1)
        assert(ns.Startup.receiverFailure=="receiver_binding_set_unavailable")
        base["ALT-CTRL-F5"]="LYCHEEDEV_WAKE"
        bindingSet=event=="BINDINGS_LOADED" and 1 or 2
        local selected=bindingSet
        changeSetOnWrite=true;emit(event);changeSetOnWrite=false
        assert(writes==1 and saves==1 and savedSet==selected)
        assert(base["ALT-CTRL-F5"]=="LYCHEEDEV_WAKE" and not base["ALT-CTRL-F12"])
        assert(base["ALT-CTRL-F11"]=="LYCHEEDEV_RELOAD" and base.F1=="PLAYER_ACTION")
        assert(not ns.Startup.receiverFailure and published==3)
        idle();emit(event)
        assert(saves==1 and writes==1)
    end
end

-- Binding readiness and combat may alternate without allowing a partial save.
fresh();bindingSet=0
assert(ns.ReceiverBindings.Register(callbacks))
combat,bindingSet=true,2;emit("BINDINGS_LOADED")
assert(waitingFrames[1].events.PLAYER_REGEN_ENABLED and not waitingFrames[1].events.UPDATE_BINDINGS)
combat,bindingSet=false,0;emit("PLAYER_REGEN_ENABLED")
assert(waitingFrames[1].events.BINDINGS_LOADED and writes==0 and saves==0)
bindingSet=2;emit("UPDATE_BINDINGS")
assert(writes==2 and saves==1 and savedSet==2);idle()

-- A successful manual reset retires the outstanding startup listener before
-- synchronous binding events, and clears its old diagnostic.
fresh();bindingSet=0
local reset,reason=ns.ReceiverBindings.Reset()
assert(not reset and reason=="receiver_binding_set_unavailable" and writes==0 and saves==0)
assert(ns.ReceiverBindings.Register(callbacks));emit("UPDATE_BINDINGS")
assert(ns.Startup.receiverFailure=="receiver_binding_set_unavailable")
bindingSet=1;assert(ns.ReceiverBindings.Reset())
assert(writes==2 and saves==1 and savedSet==1 and not ns.Startup.receiverFailure);idle()
bindingSet=0;assert(ns.ReceiverBindings.Reset())
assert(writes==2 and saves==1)
print("formal bindings: visible saved defaults, scheme readiness, deferred lifecycle, settings and rollback")
