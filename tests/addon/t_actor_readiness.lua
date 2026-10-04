local Env=...
local ns=Env.LoadWorkbench()
local loggedIn,inWorld=true,true
IsLoggedIn=function()return loggedIn end
IsPlayerInWorld=function()return inWorld end
UnitFullName=function()return "Character","Realm" end
UnitGUID=function()return "Player-1-1" end
Env.LoadAddon("Core/Platform.lua",ns)
assert(ns.Platform.ObserveActor().guid=="Player-1-1")
loggedIn=false
assert(select(2,ns.Platform.ObserveActor())=="actor_not_logged_in")
loggedIn=true;inWorld=false
assert(select(2,ns.Platform.ObserveActor())=="actor_not_in_world")
inWorld=true;UnitGUID=function()error("unavailable")end
assert(select(2,ns.Platform.ObserveActor())=="actor_observation_failed")
UnitGUID=nil
assert(select(2,ns.Platform.ObserveActor())=="actor_observation_unavailable")
print("retained identity cannot establish current actor/world readiness")
