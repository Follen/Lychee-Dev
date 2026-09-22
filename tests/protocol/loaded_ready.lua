local root = assert(arg[1])
local ns = {Release="2.0.0-dev",Startup={ready=true,identity={product="retail",build="12.1.0.12345"}}}
local eligible = false
ns.Platform = {
    ObserveActor=function() return {character="Paladin",realm="Realm",guid="Player-1-123"} end,
    ObserveInputState=function() if eligible then return true end; return false,"input_keyboard_focus" end,
}
CreateFrame=function() error("unexpected frame") end
for _,name in ipairs({"Core/Persistence.lua","Bridge/Session.lua","Bridge/CaptureWriter.lua","Bridge/ReportStore.lua","Bridge/ProbeRunner.lua","Core/Controls.lua"}) do
    assert(loadfile(root.."/"..name))("Lychee Dev",ns)
end
assert(ns.Persistence.Load())
LycheeToolkitDB.options.bridgeEnabled=true
assert(ns.Session.Bind(string.rep("a",32)))
local callbacks={}
EventRegistry={
    RegisterCallback=function(self,event,callback,owner) callbacks[owner]=callback end,
    UnregisterCallback=function(self,event,owner) callbacks[owner]=nil end,
}
local shown,auxiliary,failDisplay
ns.ReceiptView={Hide=function() shown,auxiliary=nil,nil end,Show=function(value,ready) if failDisplay then return nil,"fixture_display_failed" end;shown,auxiliary=value,ready;return true end}
local code="readyExecuted = (readyExecuted or 0) + 1; return 42"
ns.ProbeQueue={Load=function(id) return ns.ProbeRunner.Load(id,code,string.rep("b",32)) end}
local initial=assert(ns.Controls.Handle("bridge load Ready-A"))
assert(shown==initial and readyExecuted==nil)
local owner,callback=next(callbacks)
assert(callback)
eligible=true;callback(owner)
assert(next(callbacks)==nil and readyExecuted==nil)
local refreshed=assert(shown)
assert(refreshed~=initial)
callback(owner);assert(shown==refreshed and readyExecuted==nil)
local reported=assert(ns.Controls.Handle("bridge run Ready-A"))
assert(readyExecuted==1 and shown==reported)
local reportReady=assert(auxiliary)
assert(ns.ProbeRunner.RefreshLoaded("Ready-A")==nil)
-- Chat focus can clear after dispatch returns. Keep the exact report while
-- the one-shot readiness callback adds a separate signal, without rerunning.
assert(ns.Controls.Handle("bridge load Ready-Deferred"))
local dispatch=ns.ProbeRunner.Dispatch
ns.ProbeRunner.Dispatch=function(id)
    local value,reason=dispatch(id)
    eligible=false
    return value,reason
end
local deferred=assert(ns.Controls.Handle("bridge run Ready-Deferred"))
assert(shown==deferred and auxiliary==nil and readyExecuted==2)
owner,callback=next(callbacks);assert(callback)
eligible=true;callback(owner)
assert(shown==deferred and auxiliary and next(callbacks)==nil and readyExecuted==2)
local deferredReady=auxiliary
callback(owner);assert(auxiliary==deferredReady and shown==deferred)
ns.ProbeRunner.Dispatch=dispatch
-- Opt-out prevents a previously captured callback from republishing readiness.
eligible=false
assert(ns.Controls.Handle("bridge load Ready-B"))
owner,callback=next(callbacks);assert(callback)
assert(ns.Controls.Handle("bridge off"))
eligible=true;callback(owner)
assert(next(callbacks)==nil and shown==nil and readyExecuted==2)
-- A QR failure cannot leave the previous ready signal displayed.
assert(ns.Controls.Handle("bridge on"))
assert(ns.Session.Bind(string.rep("a",32)))
eligible=false
assert(ns.Controls.Handle("bridge load Ready-C"))
owner,callback=next(callbacks);assert(callback)
eligible=true;failDisplay=true
print=function() end
callback(owner)
assert(shown==nil and next(callbacks)==nil and readyExecuted==2)
failDisplay=false;eligible=false
local readyInitial=assert(ns.Controls.Handle("bridge ready"))
owner,callback=next(callbacks);assert(callback)
eligible=true;callback(owner)
local sessionReady=assert(shown)
assert(next(callbacks)==nil and readyExecuted==2 and sessionReady~=readyInitial)
assert(ns.Controls.Handle("bridge unbind"))
assert(ns.Controls.Handle("bridge ready")==nil and shown==nil)
io.write(assert(ns.CaptureWriter.Encode({initial=initial,refreshed=refreshed,reported=reported,reportReady=reportReady,readyInitial=readyInitial,sessionReady=sessionReady})))
