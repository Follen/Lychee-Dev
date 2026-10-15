local Env=...
local ns=Env.LoadWorkbench()
C_Timer.NewTimer=function(_,callback)
    local timer={cancelled=false,Cancel=function(self)self.cancelled=true end}
    Env.pendingTimers[#Env.pendingTimers+1]=function()if not timer.cancelled then callback() end end
    return timer
end
Env.LoadAddon("Bridge/ProbeExecution.lua",ns)
local completions,outcome,meta=0
local function done(ok,value,metadata) completions=completions+1;outcome={ok,value};meta=metadata end
local order={}
ns.ProbeExecution.Run(function(api)
    api:OnCleanup(function()order[#order+1]=1 end)
    api:OnCleanup(function()order[#order+1]=2 end)
    api:Finish("early")
    error("failure after Finish")
end,10,done)
assert(completions==1 and not outcome[1] and meta.resourcesReleased)
assert(order[1]==2 and order[2]==1,"cleanup not LIFO")
local cb,api
api=ns.ProbeExecution.Run(function(p)
    assert(p:Async(10))
    cb=assert(p:Callback(function()p:Finish("result")end))
end,10,done)
assert(completions==1)
cb();assert(completions==2 and outcome[1] and outcome[2]=="result")
assert(not cb());Env.PumpTimers();assert(completions==2,"late timer settled twice")
ns.ProbeExecution.Run(function(p)assert(p:Async(1))end,1,done)
Env.PumpTimers();assert(completions==3 and outcome[2]=="probe_timeout")
ns.ProbeExecution.Run(function(p)
    p:OnCleanup(function()error("resource still present")end)
    return 42
end,1,done)
assert(completions==4 and outcome[1] and not meta.resourcesReleased)
ns.ProbeExecution.Run(function(p)
    for i=1,16 do assert(p:OnCleanup(function()end)) end
    assert(not p:OnCleanup(function()end))
    assert(not p:Async(2))
    p:Log("safe",Env.MakeSecret())
    return true
end,1,done)
assert(completions==5 and meta.logs[1]=="safe  <secret>")
print("transport-independent probe execution ok")
