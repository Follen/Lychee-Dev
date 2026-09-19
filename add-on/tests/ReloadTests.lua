local now, combat, reloads = 1000, false, 0
local frames, timers, shown = {}, {}, {}
SlashCmdList = {LYCHEEDEV=function() end}
function time() return now end
function UnitName() return "Player" end
function GetRealmName() return "Realm" end
function GetBuildInfo() return "12.1.0", "69875" end
function ReloadUI() reloads=reloads+1 end
function CreateFrame()
    local f={events={}}
    function f:RegisterEvent(e) self.events[e]=true end
    function f:UnregisterEvent(e) self.events[e]=nil end
    function f:UnregisterAllEvents() self.events={} end
    function f:SetScript(_,fn) self.callback=fn end
    function f:Fire(e,...) if self.events[e] then self.callback(self,e,...) end end
    frames[#frames+1]=f
    return f
end
C_Timer={NewTimer=function(_,fn)
    local t={callback=fn,Cancel=function(self) self.cancelled=true end}
    timers[#timers+1]=t;return t
end}
local function boot()
    local ns={Client={id=os.getenv("LYCHEE_TEST_CLIENT") or "retail"},Automation={},
        InitializeDatabase=function() LycheeDevDB=LycheeDevDB or {} end,
        EnsureSafety=function() end,IsCombatBlocked=function() return combat end,
        AutomationOverlay={ShowIdentity=function(json) shown[#shown+1]=json;return true end,
            HideIdentity=function() shown.hidden=(shown.hidden or 0)+1 end}}
    function ns.RegisterCombatShutdown(fn) ns.shutdown=fn end
    assert(loadfile("Modules/Automation/Reload.lua"))("Lychee Dev",ns)
    return ns,frames[#frames]
end
local function request(nonce)
    return {schema=1,nonce=nonce,requestedAt=now,character="Player",realm="Realm"}
end
LycheeDevDB=nil
local ns,f=boot()
assert(#timers==0 and #shown==0,"disabled startup creates no timer/overlay")
f:Fire("ADDON_LOADED","Other")
assert(f.events.ADDON_LOADED)
f:Fire("ADDON_LOADED","Lychee Dev")
assert(not next(f.events) and LycheeDevDB==nil,"disabled startup did not release listener")
assert(not ns.AutomationReload.Request("bad nonce"))
combat=true;assert(not ns.AutomationReload.Request("combat"));combat=false
assert(reloads==0)
assert(ns.AutomationReload.Request("fresh"))
assert(reloads==1 and #shown==0,"old Lua session published ready")
assert(not ns.AutomationReload.Request("another"),"duplicate reload sent in same session")
assert(LycheeDevDB.reloadHandshake.nonce=="fresh")
ns,f=boot()
f:Fire("ADDON_LOADED","Lychee Dev")
assert(LycheeDevDB.reloadHandshake==nil,"pending request not consumed")
f:Fire("PLAYER_ENTERING_WORLD",false,true)
assert(#shown==0,"ready before loading screen gone")
f:Fire("LOADING_SCREEN_DISABLED")
assert(#shown==1 and shown[1]:find('"run":"fresh"',1,true))
assert(not ns.AutomationReload.Clear("stale"),"stale cleanup hid new marker")
assert(ns.AutomationReload.Clear("fresh"))
assert(not next(f.events) and timers[#timers].cancelled)
local count=#shown
LycheeDevDB.reloadHandshake=request("reverse")
ns,f=boot();f:Fire("ADDON_LOADED","Lychee Dev")
f:Fire("LOADING_SCREEN_DISABLED")
assert(#shown==count)
f:Fire("PLAYER_ENTERING_WORLD",false,true)
assert(#shown==count+1,"reversed event order failed")
f:Fire("LOADING_SCREEN_ENABLED")
assert(not next(f.events),"new loading screen retained ready marker")
count=#shown
LycheeDevDB.reloadHandshake=request("login")
ns,f=boot();f:Fire("ADDON_LOADED","Lychee Dev")
f:Fire("PLAYER_ENTERING_WORLD",true,false);f:Fire("LOADING_SCREEN_DISABLED")
assert(#shown==count and not next(f.events),"ordinary login accepted as reload")
LycheeDevDB.reloadHandshake=request("expired");LycheeDevDB.reloadHandshake.requestedAt=now-121
ns,f=boot();f:Fire("ADDON_LOADED","Lychee Dev")
assert(not next(f.events) and LycheeDevDB.reloadHandshake==nil)
LycheeDevDB.reloadHandshake=request("wrong-character");LycheeDevDB.reloadHandshake.character="Other"
ns,f=boot();f:Fire("ADDON_LOADED","Lychee Dev")
f:Fire("PLAYER_ENTERING_WORLD",false,true);f:Fire("LOADING_SCREEN_DISABLED")
assert(#shown==count and not next(f.events),"different character accepted")
LycheeDevDB.reloadHandshake={schema=2,nonce="future"}
ns,f=boot();f:Fire("ADDON_LOADED","Lychee Dev")
assert(LycheeDevDB.reloadHandshake.schema==2 and not ns.AutomationReload.Request("replace"))
LycheeDevDB.reloadHandshake=request("timeout")
ns,f=boot();f:Fire("ADDON_LOADED","Lychee Dev")
timers[#timers].callback()
assert(not next(f.events),"timeout left event listeners")
LycheeDevDB.reloadHandshake=request("combat")
ns,f=boot();f:Fire("ADDON_LOADED","Lychee Dev")
ns.shutdown();assert(not next(f.events),"combat left handshake active")
LycheeDevDB.reloadHandshake=nil
ns,f=boot();f:Fire("ADDON_LOADED","Lychee Dev")
ReloadUI=function() error("blocked") end
assert(not ns.AutomationReload.Request("error") and LycheeDevDB.reloadHandshake==nil)
print("Reload handshake PASS: startup, new session, both event orders, nonce cleanup, expiry, login, combat, failure")
