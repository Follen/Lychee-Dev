local Env,client,addonRoot=...
local created={}
local create=CreateFrame
CreateFrame=function(...)local frame=create(...);created[#created+1]=frame;return frame end
local overrides={}
GetBindingAction=function(key)return overrides[key]or""end
ClearOverrideBindings=function()overrides={}end
SetOverrideBindingClick=function(_,_,key,button)overrides[key]="CLICK "..button..":LeftButton";return true end
IsLoggedIn=function()return true end
UnitFullName=function()return"Tester","Realm"end
local actorReady=false
UnitGUID=function()return actorReady and "Player-fixture" or nil end
GetRealmName=function()return"Realm"end
LycheeToolkitDB={schema=1,options={bridgeEnabled=false}}
LycheeToolkitBridgeDB={schema=1,reports={}}
local ns={}
local manifest=assert(io.open(addonRoot.."/Lychee Dev Rust.toc","r"))
local loaded={}
for line in manifest:lines()do
    if line:match("%.lua$")then local path=line:gsub("\\","/");Env.LoadAddon(path,ns);loaded[path]=true end
end
manifest:close()
assert(loaded["Bridge/NativeAdapter.lua"] and loaded["Bridge/NativeInputState.lua"] and loaded["Bridge/NativeWire.lua"])
assert(not loaded["Bridge/SlotRuntime.lua"] and not loaded["Bridge/SlotProtocol.lua"] and not loaded["Bridge/MemoryProtocol.lua"])
assert(#created==1,"disabled closure exceeded one-shot loader budget")
local loader=created[1]
loader:GetScript("OnEvent")(loader,"ADDON_LOADED","Lychee Dev")
assert(ns.Startup.ready and not ns.Startup.receiverFailure,tostring(ns.Startup.receiverFailure))
assert(#created==5,"disabled startup exceeded loader+owner+3binding buttons")
actorReady=true;loader:GetScript("OnEvent")(loader,"PLAYER_LOGIN")
assert(loader:GetScript("OnEvent")==nil,"disabled candidate retained loader subscription")
for _,frame in ipairs(created)do
    assert(not frame:GetScript("OnUpdate"),"disabled candidate retained OnUpdate")
    assert(not frame:GetScript("OnEvent"),"disabled candidate retained event callback")
end
assert(#Env.pendingTimers==0,"disabled closure created recurring work")
if ns.Mailbox then assert(ns.Mailbox.input:find('"activationOnly":true',1,true),"disabled closure advertised execution INPUT")end
print("candidate complete TOC: disabled loader/owner/buttons budget, no idle events/timers/OnUpdate, v4-only transport")
