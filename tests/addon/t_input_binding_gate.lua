local Env,client=...
local ns=Env.LoadWorkbench()
ns.Startup={ready=true}
local base,foreign={},{}
local combat,active=false,false
local saves,writes=0,0
InCombatLockdown=function()return combat end
IsLoggedIn=function()return true end
GetCurrentBindingSet=function()return 2 end
SaveBindings=function(set)assert(set==2);saves=saves+1 end
GetBindingAction=function(key,overrides)return overrides and (foreign[key] or base[key] or "") or (base[key] or "")end
GetBindingKey=function(action)
    local keys={}
    for key,bound in pairs(base)do if bound==action then keys[#keys+1]=key end end
    table.sort(keys);return unpack(keys)
end
SetBinding=function(key,action)writes=writes+1;base[key]=action;return true end
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
print("formal bindings: visible saved defaults, conflicts, settings edits, idempotence")
