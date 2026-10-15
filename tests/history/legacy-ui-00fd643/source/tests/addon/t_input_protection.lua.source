local Env=...
local ns=Env.LoadWorkbench()
local now,ready=0,true
ns.Compat.MonotonicSeconds=function()return now end
ns.Platform={ObserveInputState=function()return ready end}
local protecting=false
ns.ActivityView={Receiving=function(value)protecting=value end}
local timers={}
C_Timer.NewTimer=function(seconds,callback)
    local t={seconds=seconds,callback=callback}
    function t:Cancel()self.cancelled=true end
    timers[#timers+1]=t;return t
end
local created={}
local originalCreate=CreateFrame
CreateFrame=function(...)
    local f=originalCreate(...);created[#created+1]=f;return f
end
Env.LoadAddon("Bridge/InputProtection.lua",ns)
Env.LoadAddon("Bridge/ProbeExecution.lua",ns)
local completed,outcome,metadata=0
local function done(ok,value,meta)completed=completed+1;outcome={ok,value};metadata=meta end
assert(#created==0 and #timers==0,"passive protection allocated resources")
ns.ProbeExecution.Run(function(p)assert(not p:IsInputProtected());return 42 end,10,done)
assert(outcome[1] and #created==0 and #timers==0,"ordinary query acquired input")
local callback
ns.ProbeExecution.Run(function(p)
    assert(p:Async(10));callback=assert(p:Callback(function()p:Finish(true)end))
end,10,done)
assert(not ns.InputProtection.IsActive() and #created==0,"async waiting acquired input")
callback();assert(outcome[1])

ns.ProbeExecution.Run(function(p)
    assert(not p:ProtectInput(11));assert(not p:ProtectInput(0/0));assert(not p:ProtectInput(math.huge))
    assert(p:ProtectInput(2) and p:IsInputProtected() and protecting)
    assert(not p:ProtectInput(1),"nested lease")
    assert(p:ReleaseInput() and not protecting)
    assert(not p:ProtectInput(1),"repeated leases bypassed hard budget")
    return true
end,10,done)
assert(outcome[1] and not ns.InputProtection.IsActive())
ns.ProbeExecution.Run(function(p)
    assert(p:ProtectInput(90),"arbitrary five-second limit retained")
    return true
end,120,done)
assert(outcome[1] and not protecting)
local oldTimer=timers[#timers]
ns.ProbeExecution.Run(function(p)
    assert(p:Async(10));assert(p:ProtectInput(2))
    oldTimer.callback();assert(p:IsInputProtected(),"old timer released new lease")
    p:OnCleanup(function()assert(not ns.InputProtection.IsActive());error("cleanup failure")end)
    callback=assert(p:Callback(function()error("probe failure")end))
end,10,done)
callback();assert(not outcome[1] and not metadata.resourcesReleased and not protecting)

for _,kind in ipairs({"timeout","cancel","combat","world"}) do
    ns.ProbeExecution.Run(function(p)
        assert(p:Async(10));assert(p:ProtectInput(2))
        callback=assert(p:Callback(function()error("late callback executed")end))
    end,10,done)
    local count=completed
    if kind=="timeout" then timers[#timers].callback()
    elseif kind=="cancel" then ns.InputProtection.Cancel()
    else created[1]:GetScript("OnEvent")(created[1],kind=="combat" and "PLAYER_REGEN_DISABLED" or "PLAYER_LEAVING_WORLD") end
    assert(completed==count+1 and not outcome[1] and not protecting and not ns.InputProtection.IsActive())
    assert(not callback() and completed==count+1)
end
ready=false
ns.ProbeExecution.Run(function(p)assert(not p:ProtectInput(1));return true end,10,done)
assert(outcome[1] and not protecting);ready=true

local scene=true
ns.ProbeExecution.Run(function(p)
    assert(p:Guard(function()return scene end,{"PLAYER_TARGET_CHANGED"},"scene_changed"))
    assert(p:Async(10));assert(p:ProtectInput(2))
    callback=assert(p:Callback(function()error("invalid scene entered")end))
end,10,done)
scene=false
local watcher=created[#created]
watcher:GetScript("OnEvent")(watcher,"PLAYER_TARGET_CHANGED")
assert(not outcome[1] and outcome[2]=="scene_changed" and not protecting)
assert(not watcher:GetScript("OnEvent") and not callback(),"guard survived completion")
scene=true
ns.ProbeExecution.Run(function(p)
    assert(p:Guard(function()return scene end,{},"callback_scene_changed"))
    assert(p:Async(10));callback=assert(p:Callback(function()error("invalid callback")end))
end,10,done)
scene=false;callback();assert(outcome[2]=="callback_scene_changed")
ns.ProbeExecution.Run(function(p)
    p:Guard(function()return Env.MakeSecret()end,{},"secret_scene")
end,10,done)
assert(not outcome[1] and outcome[2]=="secret_scene")
scene=true
ns.ProbeExecution.Run(function(p)
    assert(p:Guard(function()return scene end,{},"return_scene_changed"))
    scene=false;return "must not succeed"
end,10,done)
assert(not outcome[1] and outcome[2]=="return_scene_changed")
-- Native propagation uses a detached effective profile, never literal F12 or
-- a mutable caller/current table. Other protected queries still propagate none.
local selected
ns.ReceiverBindings={Current=function()return selected end}
ns.Compat.ReceiverChord=function(key)return key end
local shield=created[1]
shield.SetPropagateKeyboardInput=function(self,value)self.propagated=value end
for _,terminal in ipairs({"F12","F11"}) do
    selected={wake="ALT-CTRL-"..terminal,submit="ALT-CTRL-SHIFT-"..terminal,close="ALT-CTRL-["}
    local wake,submit=selected.wake,selected.submit
    local lease=assert(ns.InputProtection.Acquire(2,nil,true))
    selected.wake="ALT-CTRL-F1" -- Must not mutate the live lease's keys.
    local keydown=shield:GetScript("OnKeyDown")
    keydown(shield,wake);assert(shield.propagated==true,"effective wake did not propagate")
    keydown(shield,submit);assert(shield.propagated==true,"effective submit did not propagate")
    keydown(shield,terminal=="F12" and "ALT-CTRL-F11" or "ALT-CTRL-F12")
    assert(shield.propagated==false,"other native profile propagated")
    keydown(shield,"ALT-CTRL-F1");assert(shield.propagated==false,"mutable profile changed lease")
    keydown(shield,"ALT-CTRL-[");assert(not ns.InputProtection.IsActive() and shield.propagated==false)
    assert(not ns.InputProtection.Release(lease))
end
for _,profile in ipairs({{}, {wake="ALT-CTRL-F11",submit="ALT-CTRL-SHIFT-F12",close="ALT-CTRL-["},
    {wake="ALT-CTRL-F1",submit="ALT-CTRL-SHIFT-F1",close="ALT-CTRL-["}, Env.MakeSecret()}) do
    selected=profile
    local frames,timerCount=#created,#timers
    assert(not ns.InputProtection.Acquire(2,nil,true),"invalid native profile acquired lease")
    assert(#created==frames and #timers==timerCount and not ns.InputProtection.IsActive(),"invalid profile created work")
end
CreateFrame=originalCreate
print("explicit input protection and scene guards ok")
